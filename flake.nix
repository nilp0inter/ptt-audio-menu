{
  description = "Screenless audio menu for a Bluetooth remote speaker microphone";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs, ... }:
    let
      lib = nixpkgs.lib;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      forAllSystems = lib.genAttrs systems;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.callPackage ./nix/package.nix { };
          ptt-audio-menu = self.packages.${system}.default;
        });

      checks = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          dummyPackage = pkgs.writeShellScriptBin "ptt-audio-menu" ''
            echo "ptt-audio-menu module check"
          '';
          nixosEval = lib.nixosSystem {
            inherit system;
            modules = [
              ./nix/nixos-module.nix
              {
                services.ptt-audio-menu = {
                  enable = true;
                  package = dummyPackage;
                  configPath = "/etc/ptt-audio-menu/config.toml";
                };
              }
            ];
          };
          homeEval = lib.evalModules {
            modules = [
              ({ lib, ... }: {
                options.home = {
                  packages = lib.mkOption {
                    type = lib.types.listOf lib.types.package;
                    default = [ ];
                  };
                  stateVersion = lib.mkOption {
                    type = lib.types.str;
                    default = "25.11";
                  };
                };
                options.systemd.user.services = lib.mkOption {
                  type = lib.types.attrsOf lib.types.anything;
                  default = { };
                };
              })
              ./nix/home-manager-module.nix
              {
                programs.ptt-audio-menu = {
                  enable = true;
                  package = dummyPackage;
                  configPath = "/home/alice/.config/ptt-audio-menu/config.toml";
                  service.enable = true;
                };
                systemd.user.services = lib.mkDefault { };
              }
            ];
            specialArgs = { inherit pkgs; };
          };
        in
        {
          package = self.packages.${system}.default;
          nixos-module = pkgs.runCommand "ptt-audio-menu-nixos-module-check"
            {
              execStart = nixosEval.config.systemd.services.ptt-audio-menu.serviceConfig.ExecStart;
            }
            ''
              test -n "$execStart"
              touch "$out"
            '';
          home-manager-module = pkgs.runCommand "ptt-audio-menu-home-manager-module-check"
            {
              execStart = homeEval.config.systemd.user.services.ptt-audio-menu.Service.ExecStart;
            }
            ''
              test -n "$execStart"
              touch "$out"
            '';
        });

      nixosModules.default = ./nix/nixos-module.nix;
      homeManagerModules.default = ./nix/home-manager-module.nix;

      devShells = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              cmake
              rustc
              rustfmt
              clippy
              gcc
              pkg-config
              alsa-lib.dev
              dbus.dev
              espeak-ng
              llvmPackages.libclang.lib
              openssl.dev
            ];

            PKG_CONFIG_PATH = "${pkgs.alsa-lib.dev}/lib/pkgconfig:${pkgs.dbus.dev}/lib/pkgconfig:${pkgs.openssl.dev}/lib/pkgconfig";
            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
            BINDGEN_EXTRA_CLANG_ARGS = "-I${pkgs.glibc.dev}/include";
            PIPER_ESPEAKNG_DATA_DIRECTORY = "${pkgs.espeak-ng}/share/espeak-ng-data";
          };
        });
    };
}
