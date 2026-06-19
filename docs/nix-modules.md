# Nix Modules

This flake exposes a package plus NixOS and Home Manager modules for installing
`ptt-audio-menu` and wiring it into systemd.

## NixOS

```nix
{
  inputs.ptt-audio-menu.url = "github:nilp0inter/ptt-audio-menu";

  outputs = { nixpkgs, ptt-audio-menu, ... }: {
    nixosConfigurations.host = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ptt-audio-menu.nixosModules.default
        {
          services.ptt-audio-menu = {
            enable = true;
            configPath = /etc/ptt-audio-menu/config.toml;
            environment.RUST_LOG = "ptt_audio_menu=debug,info";
          };
        }
      ];
    };
  };
}
```

The system service defaults to `root` with `audio` and `bluetooth`
supplementary groups. Set `user`, `group`, and `supplementaryGroups` if the
host has a narrower Bluetooth/audio permission model.

## Home Manager

```nix
{
  imports = [ inputs.ptt-audio-menu.homeManagerModules.default ];

  programs.ptt-audio-menu = {
    enable = true;
    configPath = "${config.xdg.configHome}/ptt-audio-menu/config.toml";
    service.enable = true;
  };
}
```

The Home Manager module always installs the package. The user service is
optional because Bluetooth profile registration and audio device access may be
host policy dependent.

## Checks

`nix flake check` builds the package and evaluates NixOS/Home Manager module
examples without requiring Bluetooth hardware. Hardware connection and audio
playback remain runtime checks because they depend on the paired RSM, BlueZ,
and the host audio stack.
