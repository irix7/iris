{
  description = "IRIS (SGI Indy emulator) build environment";

  # Resolves via the local nixpkgs registry entry (flake:nixpkgs). Pin to a
  # specific revision here if you need reproducibility across machines.
  inputs.nixpkgs.url = "nixpkgs";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      lib = pkgs.lib;

      # Every native library iris links or dlopens at run time, in one list.
      # The dev shell compiles and links against these, the launcher in
      # `target/launcher/bin` puts them on the loader's search path, and the
      # packaged binary bakes them into its RPATH — so a dependency is added
      # once, not in three places.
      #
      # `ldd target/release/iris` needs only libasound (cpal), libstdc++ and
      # libgcc_s (a C dependency's unwinder) and glibc; the X11, Wayland and GL
      # entries are dlopen'd at run time and so never appear there.
      runtimeLibs = with pkgs; [
        alsa-lib
        stdenv.cc.cc.lib # libstdc++.so.6, libgcc_s.so.1
        libv4l
        libxkbcommon
        wayland
        libx11
        libxcursor
        libxrandr
        libxi
        libxxf86vm
        libGL
        libGLU
        fontconfig
        zlib
        openssl
        libiconv
      ];

      # Dev-shell-only entries: kernel headers and a Wayland code generator the
      # build tools want. Nothing loads these at run time, so they stay out of
      # `runtimeLibs` and out of the launcher's search path.
      devShellInputs = runtimeLibs ++ (with pkgs; [ linuxHeaders wayland-scanner ]);

      libraryPath = lib.makeLibraryPath runtimeLibs;

      # The launcher (issue #104): a script that finds the cargo-built binary
      # relative to itself and exports `libraryPath` as LD_LIBRARY_PATH. The
      # copy installed by `nix develop` under `target/launcher/bin` therefore
      # runs from a clean environment — no PATH, no HOME and no hand-maintained
      # LD_LIBRARY_PATH — and moves with the checkout, because nothing about
      # the checkout is baked in.
      launcher = pkgs.writeShellScript "iris-launcher" ''
        set -euo pipefail

        # The copy under target/launcher/bin is rewritten on every `nix
        # develop`; edit this file (flake.nix), not that copy.
        prog=''${0##*/}

        # `readlink -f` by absolute path: `env -i` leaves PATH empty, and the
        # store path is stable for the life of this closure.
        self=$(${pkgs.coreutils}/bin/readlink -f -- "$0")
        root=''${self%/*}
        root=''${root%/*}

        # First existing candidate wins: an explicit override (a build in
        # another target dir), then the checkout's cargo output. Release comes
        # first because it is the documented build; debug follows because it
        # is what `cargo build` alone produces.
        bin=
        for candidate in "''${IRIS_LAUNCHER_BIN:-}" "$root/../release/$prog" "$root/../debug/$prog"; do
          if [ -n "$candidate" ] && [ -x "$candidate" ]; then
            bin=$candidate
            break
          fi
        done

        if [ -z "$bin" ]; then
          echo "iris-launcher: no $prog in $root/../release or $root/../debug" >&2
          echo "iris-launcher: build it first, e.g. cargo build --release --features lightning,gpujit" >&2
          exit 127
        fi

        # Prepend, so `LD_LIBRARY_PATH=/tmp/stage/lib` still works for library
        # testing. iris itself reads no environment variable for this.
        export LD_LIBRARY_PATH="${libraryPath}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
        exec "$bin" "$@"
      '';

      # Shared by `packages.default` and the hermetic check below: copy the
      # binaries into a store path and point their RPATH at `runtimeLibs`.
      # The result runs with no environment at all (`env -i result/bin/iris
      # --help`) and stays valid as long as the store paths do.
      mkIris = { srcIris, srcCi ? null, version }: pkgs.stdenv.mkDerivation {
        pname = "iris";
        inherit version;
        dontUnpack = true;
        nativeBuildInputs = [ pkgs.patchelf ];
        installPhase = ''
          runHook preInstall
          mkdir -p $out/bin
          install -m755 ${srcIris} $out/bin/iris
          ${lib.optionalString (srcCi != null) "install -m755 ${srcCi} $out/bin/iris-ci"}
          for b in $out/bin/iris ${lib.optionalString (srcCi != null) "$out/bin/iris-ci"}; do
            patchelf --set-rpath "${libraryPath}" "$b"
          done
          runHook postInstall
        '';
      };

      # Flake evaluation is pure, so it cannot see the checkout's target/:
      # `builtins.getEnv` returns "" and `builtins.pathExists` is false there.
      # `nix build --impure .#` therefore picks up the cargo build named by
      # $IRIS_BIN, or release/iris under the invoking directory.
      cargoBinary =
        let
          explicit = builtins.getEnv "IRIS_BIN";
          cwd = builtins.getEnv "PWD";
          candidate = if explicit != "" then explicit else "${cwd}/target/release/iris";
        in
        if candidate != "" && builtins.pathExists candidate then candidate else null;

      unbuilt = pkgs.runCommand "iris-unbuilt" { } ''
        echo "iris: no cargo build found." >&2
        echo "  Build one:   cargo build --release --features lightning,gpujit" >&2
        echo "  Then:        nix build --impure .#launcher" >&2
        echo "  Or point IRIS_BIN at a binary, still with --impure." >&2
        exit 1
      '';

      # Failing at build time rather than at evaluation time is deliberate:
      # `nix flake check` and `nix flake show` evaluate packages, and a `throw`
      # here would make them fail on a checkout that has not been built yet.
      launcherPackage =
        if cargoBinary == null then unbuilt
        else
          let
            binDir = builtins.dirOf cargoBinary;
            ciBinary = "${binDir}/iris-ci";
            hasCi = builtins.pathExists ciBinary;
          in
          mkIris {
            srcIris = builtins.path { path = cargoBinary; name = "iris"; };
            srcCi = if hasCi then builtins.path { path = ciBinary; name = "iris-ci"; } else null;
            version = (lib.importTOML ./Cargo.toml).package.version;
          };

      # `nix run .#smoke` runs tools/iris-launcher-smoke.sh against the launcher
      # in the current directory, under `env -i` with an empty LD_LIBRARY_PATH.
      smoke = pkgs.writeShellScriptBin "iris-launcher-smoke" ''
        exec ${pkgs.bash}/bin/bash ${self}/tools/iris-launcher-smoke.sh "$@"
      '';

      # A binary that needs ALSA but cannot find it by itself: exactly the
      # failure the launcher and the packaged RPATH exist to fix. cc-wrapper
      # gives the link an RPATH to every build input, so strip it — otherwise
      # the stub starts on its own and the checks below prove nothing.
      stubBinary = pkgs.stdenv.mkDerivation {
        pname = "iris-stub";
        version = "0";
        dontUnpack = true;
        buildInputs = [ pkgs.alsa-lib ];
        nativeBuildInputs = [ pkgs.patchelf ];
        buildPhase = ''
          cat > stub.c <<'EOF'
          #include <stdio.h>
          #include <alsa/asoundlib.h>
          int main(void) {
            printf("usage: iris [OPTIONS]\nalsa %s\n", snd_asoundlib_version());
            return 0;
          }
          EOF
          $CC stub.c -o iris -lasound
        '';
        dontFixup = true;
        installPhase = ''
          install -Dm755 iris $out/bin/iris
          patchelf --remove-rpath $out/bin/iris
          ! patchelf --print-rpath $out/bin/iris | grep -q .
        '';
      };

      # Run a check under the same conditions the smoke test uses: no
      # environment at all, an empty search path, and a binary that cannot
      # start without help. `got`, not `out`: `out` is the derivation's own
      # output variable.
      cleanRun = bin: ''
        if env -i ${stubBinary}/bin/iris >/dev/null 2>&1; then
          echo "check is not meaningful: the stub loads libasound without help" >&2
          exit 1
        fi
        got=$(env -i LD_LIBRARY_PATH= ${bin}) || {
          echo "does not run from an empty environment: ${bin}" >&2
          exit 1
        }
        case "$got" in
          *"usage: iris"*) ;;
          *) echo "unexpected output from ${bin}: $got" >&2; exit 1 ;;
        esac
        mkdir -p $out
      '';
    in
    {
      devShells.x86_64-linux.default =
        pkgs.mkShell {
          # Build-script tooling: C compiler, pkg-config, and clang/libclang for
          # bindgen (v4l2-sys-mit / nokhwa).
          nativeBuildInputs = with pkgs; [
            pkg-config
            clang
            rustup
            gnumake
          ];

          buildInputs = devShellInputs;

          LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";

          # Use the rig's pinned Rust toolchain rather than downloading a fresh
          # one: point rustup at the shared toolchain and cargo home. These paths
          # are host-specific (the "rig" scratch tree); on another machine, drop
          # this shellHook and let rustup honour rust-toolchain.toml instead.
          shellHook = ''
            export RUSTUP_HOME=/mnt/europa/sgi-toolchain-scratch/rig/tools/rustup
            export CARGO_HOME=/mnt/europa/sgi-toolchain-scratch/rig/tools/cargo
            export RUSTUP_TOOLCHAIN=nightly-x86_64-unknown-linux-gnu

            # Install the launcher. `--ci` runs and the test scripts use this
            # path; the cargo-built binary stays where it is for debugging.
            repo=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
            mkdir -p "$repo/target/launcher/bin"
            for b in iris iris-ci; do
              install -m 0755 ${launcher} "$repo/target/launcher/bin/$b"
            done
            export IRIS_LAUNCHER="$repo/target/launcher/bin/iris"
          '';
        };

      packages.x86_64-linux = {
        # `nix build --impure .#` — see `cargoBinary` above for why.
        default = launcherPackage;
        launcher = launcherPackage;
      };

      apps.x86_64-linux = {
        default = {
          type = "app";
          program = "${launcherPackage}/bin/iris";
        };
        smoke = {
          type = "app";
          program = "${smoke}/bin/iris-launcher-smoke";
        };
      };

      # Both checks are hermetic — no cargo build, so `nix flake check` stays
      # fast — and guard the two halves of the same problem: the launcher
      # script must supply the search path, and the packaged binary must carry
      # it in its RPATH.
      checks.x86_64-linux = {
        # Lay out what `nix develop` installs: <root>/launcher/bin/iris next to
        # <root>/release/iris, then run the real launcher script from an empty
        # environment. This is the script the smoke test uses, without needing
        # a built iris.
        launcher-script = pkgs.runCommand "iris-launcher-script-check" { } (''
          root=$NIX_BUILD_TOP/checkout
          mkdir -p $root/release $root/launcher/bin
          cp ${stubBinary}/bin/iris $root/release/iris
          cp ${launcher} $root/launcher/bin/iris
          chmod +x $root/launcher/bin/iris
        '' + cleanRun "$root/launcher/bin/iris");

        launcher-rpath = pkgs.runCommand "iris-launcher-rpath-check" { } (
          cleanRun "${mkIris { srcIris = "${stubBinary}/bin/iris"; version = "0"; }}/bin/iris");
      };
    };
}
