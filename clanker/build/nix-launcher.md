# The Nix launcher: library path from the closure, not from the shell

**Keywords:** nix, flake, launcher, LD_LIBRARY_PATH, RPATH, patchelf, devshell, issue #104

## Symptom

A cargo build made inside `nix develop` cannot start outside the shell:

```
$ target/release/iris --help
iris: error while loading shared libraries: libasound.so.2: cannot open shared object file
```

`nix develop` sets `NIX_LDFLAGS`, `NIX_CFLAGS_COMPILE` and `buildInputs` for the
*compiler*, and no `LD_LIBRARY_PATH` for the *loader*. So the binary links
store paths that the loader never searches, and every run needed a
hand-copied list of `-L` entries plus the gcc lib directory — the derivation
in `test/journal/run.sh` before #104, and in shell history before that. A store
path change (nixpkgs bump, GC) breaks the list with no obvious cause.

Only two libraries actually go missing:

```
$ readelf -d target/release/iris | grep NEEDED
libasound.so.2      (cpal)
libstdc++.so.6      (a C dependency's unwinder)
libgcc_s.so.1
libm.so.6, libc.so.6
```

The X11/Wayland/GL libraries are dlopen'd, so `ldd` never shows them; they
still belong in `runtimeLibs` because a `hostgl` build links libGL.

## What the flake does now

`flake.nix` has one `runtimeLibs` list feeding three consumers:

- `devShell.buildInputs` — compiler and linker search paths, as before.
- `target/launcher/bin/{iris,iris-ci}` — a `writeShellScript` wrapper installed
  by `shellHook` on every `nix develop`. It resolves the binary relative to
  itself (`$0` → `readlink -f` → `../../release/iris`, then `../../debug/iris`,
  `IRIS_LAUNCHER_BIN` overrides) and prepends `lib.makeLibraryPath runtimeLibs`
  to `LD_LIBRARY_PATH`. No checkout path is baked in, so it moves with the
  tree; nothing comes from the caller's environment, so `env -i` works.
- `nix build --impure .#launcher` — copies the cargo binaries into the store
  and `patchelf --set-rpath` the same closure, for an artefact that needs no
  wrapper at all (`result/bin/iris`).

`nix flake check` builds `checks.x86_64-linux.launcher-rpath`: a stub linked
against ALSA with its RPATH stripped, packaged by the same `mkIris` function.
The check fails if the stub starts *without* the RPATH (the test would be
vacuous) or if the packaged copy does not start from `env -i`.

## Gotchas found while building it

- **Pure flake evaluation cannot see `target/`.** A git flake source contains
  only tracked files, so `./target/release/iris` inside `flake.nix` resolves to
  the store copy and does not exist; even `builtins.pathExists /abs/path`
  returns false in pure mode. Hence `nix build --impure` plus
  `builtins.getEnv "IRIS_BIN"` / `$PWD`. The launcher *script* needs none of
  this — it is generated from the flake alone and finds the binary at run time.
- **Do not `throw` for "no cargo build yet".** `nix flake check` and
  `nix flake show` evaluate `packages`, so a `throw` in `packages.default`
  fails them on a fresh checkout. Emit a `runCommand` that fails at *build*
  time with instructions instead; `flake check` does not build packages
  (verified on Lix 2.95.3).
- **cc-wrapper adds an RPATH at link time, and `fixupPhase` shrinks it.** A
  negative test needs `dontFixup = true` *and* `patchelf --remove-rpath`, or the
  "unpatched" stub already finds its library and the check proves nothing.
- **`--impure` is load-bearing twice:** the binary path (`getEnv`) and
  `builtins.path` of a file outside the store. Without it you get
  `iris-unbuilt` and a clear message.
- **`env -i` has no `PATH`**, so the launcher has an absolute store-path
  shebang (`writeShellScript`) and calls `${pkgs.coreutils}/bin/readlink` by
  absolute path. No `dirname`, no `install`, no `git` in the script.
- **Do not name a shell variable `out` in a derivation.** It is the
  derivation's own output variable; assigning it made the builder "fail to
  produce output path" with an empty log.
- **New flake files must be `git add`ed before `nix run`/`flake check` see
  them** — the git flake source only carries tracked and staged files.
- **A GC of the devshell closure breaks the installed launcher** until
  `nix develop` is entered again: the copy under `target/launcher/bin` is a
  plain file whose `LD_LIBRARY_PATH` names store paths. That is the same
  lifetime as any nix-built binary; re-enter the shell.
- **The rig's remote builders may be unreachable** (`ssh-ng://…localdomain`).
  Builds fall back to the local machine; the warnings are noise, not failure.

## Verification (2026-10-24, Lix 2.95.3, nixpkgs 26.11)

```
nix flake check                                                   # green, ~5 s
nix develop -c bash -c 'ls target/launcher/bin'                   # iris, iris-ci
env -i LD_LIBRARY_PATH= target/launcher/bin/iris --help           # exit 0
env -i LD_LIBRARY_PATH= target/release/iris --help                # exit 127, libasound
nix build --impure .#launcher                                     # result/bin/iris
env -i LD_LIBRARY_PATH= result/bin/iris --help                    # exit 0, ldd resolves
nix run --impure .#smoke                                          # PASS
./test/journal/run.sh                                             # PASS via the launcher
```
