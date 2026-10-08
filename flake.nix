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
        aurelia = pkgs.rustPlatform.buildRustPackage {
          pname = "aurelia";
          version = "0.1.0";
          src = pkgs.lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "-p" "aurelia" ];
          # Unit tests run in the dev shell; the crate graph is large.
          doCheck = false;

          nativeBuildInputs = nativeBuildInputs ++ [ pkgs.makeWrapper ];
          inherit buildInputs;

          postInstall = ''
            install -Dm644 crates/aurelia/assets/linux/dev.aurelia.Aurelia.desktop \
              $out/share/applications/dev.aurelia.Aurelia.desktop
            install -Dm644 crates/aurelia/assets/linux/dev.aurelia.Aurelia.svg \
              $out/share/icons/hicolor/scalable/apps/dev.aurelia.Aurelia.svg
          '';

          postFixup = ''
            wrapProgram $out/bin/aurelia \
              --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath runtimeLibs} \
              --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.mpv ]}
          '';

          meta = {
            description = "A cinematic Jellyfin desktop client built with GPUI";
            mainProgram = "aurelia";
            platforms = pkgs.lib.platforms.linux;
          };
        };
      in
      {
        packages.default = aurelia;

        apps.default = {
          type = "app";
          program = "${aurelia}/bin/aurelia";
        };

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
