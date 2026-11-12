{
  description = "Contrast dev shell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };
      rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
    in {
      devShells.${system}.default = pkgs.mkShell {
        packages = [
          rust
          pkgs.cargo-mutants
          pkgs.cargo-semver-checks
          pkgs.cargo-deny
          # リンクを速く、メモリを少なくする
          pkgs.mold
          # nested プロトタイプ（contrast / contrast-compositor）のビルドと実行用
          pkgs.pkg-config
          pkgs.wayland
          pkgs.libxkbcommon
          pkgs.libGL
          pkgs.foot
          pkgs.wayland-utils
          # DRM バックエンド（libseat / udev / drm の sys クレートが pkg-config で探す）
          pkgs.seatd
          pkgs.libdrm
          pkgs.libinput
          pkgs.systemdLibs
          pkgs.libgbm
        ];
        # winit / EGL は libwayland-client・libEGL・libxkbcommon を dlopen するので実行時に見えるようにする
        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
          pkgs.wayland
          pkgs.libxkbcommon
          pkgs.libGL
          pkgs.seatd
          pkgs.systemdLibs
          pkgs.libgbm
        ];
        # 既存の RUSTFLAGS があれば、末尾に足す
        shellHook = ''
          export RUSTFLAGS="''${RUSTFLAGS:+$RUSTFLAGS }-C link-arg=-fuse-ld=mold"
        '';
      };
    };
}