{
  description = "IRIS (SGI Indy emulator) build environment";

  # Resolves via the local nixpkgs registry entry (flake:nixpkgs). Pin to a
  # specific revision here if you need reproducibility across machines.
  inputs.nixpkgs.url = "nixpkgs";

  outputs = { self, nixpkgs }: {
    devShells.x86_64-linux.default =
      let
        pkgs = import nixpkgs { system = "x86_64-linux"; };
      in
      pkgs.mkShell {
        # Build-script tooling: C compiler, pkg-config, and clang/libclang for
        # bindgen (v4l2-sys-mit / nokhwa).
        nativeBuildInputs = with pkgs; [
          pkg-config
          clang
          rustup
          gnumake
        ];

        # Link-time native libraries (cpal/alsa, winit X11+wayland, glow/GL,
        # nokhwa/v4l, image/png).
        buildInputs = with pkgs; [
          alsa-lib
          libv4l
          linuxHeaders
          libxkbcommon
          wayland
          wayland-scanner
          xorg.libX11
          xorg.libXcursor
          xorg.libXrandr
          xorg.libXi
          xorg.libXxf86vm
          libGL
          libGLU
          fontconfig
          zlib
          openssl
          libiconv
        ];

        LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";

        # Use the rig's pinned Rust toolchain rather than downloading a fresh
        # one: point rustup at the shared toolchain and cargo home. These paths
        # are host-specific (the "rig" scratch tree); on another machine, drop
        # this shellHook and let rustup honour rust-toolchain.toml instead.
        shellHook = ''
          export RUSTUP_HOME=/mnt/europa/sgi-toolchain-scratch/rig/tools/rustup
          export CARGO_HOME=/mnt/europa/sgi-toolchain-scratch/rig/tools/cargo
          export RUSTUP_TOOLCHAIN=nightly-x86_64-unknown-linux-gnu
        '';
      };
  };
}
