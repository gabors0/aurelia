{
  description = "Aurelia — a cinematic Jellyfin desktop client built with GPUI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
    }:
    flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (
      system:
      let
        pkgs = import nixpkgs { inherit system; };

        # Libraries GPUI loads at runtime (wgpu/Vulkan, Wayland, X11, fonts).
        runtimeLibs = with pkgs; [
          vulkan-loader
          libGL
          wayland
          libxkbcommon
          libx11
          libxcb
          libxcursor
          libxi
          libxrandr
          fontconfig
          freetype
        ];

        buildInputs =
          runtimeLibs
          ++ (with pkgs; [
            zstd
            expat
          ]);

        nativeBuildInputs = with pkgs; [
          pkg-config
          cmake
          python3
        ];
      in
      {
        devShells.default = pkgs.mkShell {
          inherit buildInputs;
          nativeBuildInputs =
            nativeBuildInputs
            ++ (with pkgs; [
              rustc
              cargo
              clippy
              rustfmt
              rust-analyzer
              mpv
            ]);
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
          RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
        };
      }
    );
}
