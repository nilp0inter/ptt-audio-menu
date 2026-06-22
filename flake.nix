{
  description = "Screenless audio menu for a Bluetooth remote speaker microphone";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs, ... }:
    let
      lib = nixpkgs.lib;
      linuxSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      darwinSystems = [
        "aarch64-darwin"
        "x86_64-darwin"
      ];
      allSystems = linuxSystems ++ darwinSystems;

      forAllSystems = lib.genAttrs allSystems;
      forLinuxSystems = lib.genAttrs linuxSystems;
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

      checks = forLinuxSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          dummyPackage = pkgs.writeShellScriptBin "ptt-audio-menu" ''
            mkdir -p /run/ptt-audio-menu
            printf '%s\n' "$@" > /run/ptt-audio-menu/args
            printf '%s\n' "RUST_LOG=$RUST_LOG" > /run/ptt-audio-menu/env
            printf '%s\n' "PIPER_ESPEAKNG_DATA_DIRECTORY=$PIPER_ESPEAKNG_DATA_DIRECTORY" >> /run/ptt-audio-menu/env
            touch /run/ptt-audio-menu/started
            exec ${pkgs.coreutils}/bin/sleep infinity
          '';
          dummyConfig = pkgs.writeText "ptt-audio-menu-config.toml" "";
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
          nixosRealHelpEval = lib.nixosSystem {
            inherit system;
            modules = [
              ./nix/nixos-module.nix
              {
                services.ptt-audio-menu = {
                  enable = true;
                  package = self.packages.${system}.default;
                  extraArgs = [ "--help" ];
                };

                system.stateVersion = "25.11";
              }
            ];
          };
          nixosRealConfigEval = lib.nixosSystem {
            inherit system;
            modules = [
              ./nix/nixos-module.nix
              {
                services.ptt-audio-menu = {
                  enable = true;
                  package = self.packages.${system}.default;
                  configPath = "${self}/examples/config.validation.toml";
                  extraArgs = [ "--check-config" ];
                };

                system.stateVersion = "25.11";
              }
            ];
          };
          nixosServiceTest = pkgs.testers.nixosTest {
            name = "ptt-audio-menu-service";
            nodes.machine = {
              imports = [ ./nix/nixos-module.nix ];

              services.ptt-audio-menu = {
                enable = true;
                package = dummyPackage;
                configPath = dummyConfig;
                logLevel = "ptt_audio_menu=debug,info";
                extraArgs = [ "--module-smoke" ];
              };

              users.groups.audio = { };
              users.groups.bluetooth = { };

              system.stateVersion = "25.11";
            };

            testScript = ''
              machine.wait_for_unit("multi-user.target")
              machine.wait_for_unit("ptt-audio-menu.service")
              machine.succeed("systemctl is-active --quiet ptt-audio-menu.service")
              machine.succeed("test -e /run/ptt-audio-menu/started")
              machine.succeed("grep -Fx -- '--config' /run/ptt-audio-menu/args")
              machine.succeed("grep -Fx -- '${dummyConfig}' /run/ptt-audio-menu/args")
              machine.succeed("grep -Fx -- '--module-smoke' /run/ptt-audio-menu/args")
              machine.succeed("grep -Fx -- 'RUST_LOG=ptt_audio_menu=debug,info' /run/ptt-audio-menu/env")
              machine.succeed("grep -E '^PIPER_ESPEAKNG_DATA_DIRECTORY=.+/share$' /run/ptt-audio-menu/env")
            '';
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
          homeRealHelpEval = lib.evalModules {
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
                  package = self.packages.${system}.default;
                  service.enable = true;
                  extraArgs = [ "--help" ];
                };
                systemd.user.services = lib.mkDefault { };
              }
            ];
            specialArgs = { inherit pkgs; };
          };
          homeRealConfigEval = lib.evalModules {
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
                  package = self.packages.${system}.default;
                  configPath = "${self}/examples/config.validation.toml";
                  service.enable = true;
                  extraArgs = [ "--check-config" ];
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
              packages = lib.concatMapStringsSep "\n" toString homeEval.config.home.packages;
            }
            ''
              test -n "$execStart"
              grep -Fx '${dummyPackage}' <<< "$packages"
              touch "$out"
            '';
          home-manager-real-package-help = pkgs.runCommand "ptt-audio-menu-home-manager-real-package-help-check"
            {
              execStart = homeRealHelpEval.config.systemd.user.services.ptt-audio-menu.Service.ExecStart;
              packages = lib.concatMapStringsSep "\n" toString homeRealHelpEval.config.home.packages;
            }
            ''
              grep -Fx '${self.packages.${system}.default}' <<< "$packages"
              $execStart > help
              grep -F "Usage:" help
              grep -F -- "--config" help
              grep -F -- "--check-config" help
              touch "$out"
            '';
          home-manager-real-package-config = pkgs.runCommand "ptt-audio-menu-home-manager-real-package-config-check"
            {
              src = self;
              execStart = homeRealConfigEval.config.systemd.user.services.ptt-audio-menu.Service.ExecStart;
              packages = lib.concatMapStringsSep "\n" toString homeRealConfigEval.config.home.packages;
            }
            ''
              grep -Fx '${self.packages.${system}.default}' <<< "$packages"
              [[ "$execStart" == *"--config ${self}/examples/config.validation.toml --check-config"* ]]
              (cd "$src" && $execStart) > log
              grep -F "config validation passed" log
              touch "$out"
            '';
          nixos-real-package-help = pkgs.runCommand "ptt-audio-menu-nixos-real-package-help-check"
            {
              execStart = nixosRealHelpEval.config.systemd.services.ptt-audio-menu.serviceConfig.ExecStart;
              packages = lib.concatMapStringsSep "\n" toString nixosRealHelpEval.config.environment.systemPackages;
            }
            ''
              grep -Fx '${self.packages.${system}.default}' <<< "$packages"
              $execStart > help
              grep -F "Usage:" help
              grep -F -- "--config" help
              grep -F -- "--check-config" help
              touch "$out"
            '';
          nixos-real-package-config = pkgs.runCommand "ptt-audio-menu-nixos-real-package-config-check"
            {
              src = self;
              execStart = nixosRealConfigEval.config.systemd.services.ptt-audio-menu.serviceConfig.ExecStart;
              packages = lib.concatMapStringsSep "\n" toString nixosRealConfigEval.config.environment.systemPackages;
            }
            ''
              grep -Fx '${self.packages.${system}.default}' <<< "$packages"
              [[ "$execStart" == *"--config ${self}/examples/config.validation.toml --check-config"* ]]
              (cd "$src" && $execStart) > log
              grep -F "config validation passed" log
              touch "$out"
            '';
          real-package-config-fixture = pkgs.runCommand "ptt-audio-menu-real-package-config-fixture-check"
            {
              src = self;
              nativeBuildInputs = [ self.packages.${system}.default ];
            }
            ''
              (cd "$src" && ptt-audio-menu --config examples/config.validation.toml --check-config) > log
              grep -F "config validation passed" log
              touch "$out"
            '';
          nixos-service-vm = nixosServiceTest;
        });

      nixosModules.default = ./nix/nixos-module.nix;
      homeManagerModules.default = ./nix/home-manager-module.nix;

      devShells = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          isDarwin = pkgs.stdenv.hostPlatform.isDarwin;
        in
        {
          default = if isDarwin then
            pkgs.mkShell {
              packages = with pkgs; [
                cargo
                cmake
                rustc
                rustfmt
                clippy
                pkg-config
                espeak-ng
                llvmPackages.libclang.lib
                onnxruntime
                sonic
                openssl.dev
              ];

              LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
              PIPER_ESPEAKNG_DATA_DIRECTORY = "${pkgs.espeak-ng}/share";
              PKG_CONFIG_PATH = "${pkgs.onnxruntime}/lib/pkgconfig:${pkgs.openssl.dev}/lib/pkgconfig";
              LIBRARY_PATH = "${pkgs.sonic}/lib:${pkgs.onnxruntime}/lib";
              RUSTFLAGS = "-C link-arg=-lsonic";
            }
          else
            pkgs.mkShell {
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
              PIPER_ESPEAKNG_DATA_DIRECTORY = "${pkgs.espeak-ng}/share";
            };
        });
    };
}
