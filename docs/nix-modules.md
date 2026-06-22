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

## Platform Support

The package and dev shell build on Linux (`x86_64-linux`, `aarch64-linux`)
and macOS (`aarch64-darwin`, `x86_64-darwin`). The NixOS and Home Manager
modules and their checks are Linux-only because they wire systemd services;
on macOS, use the package directly (e.g. via `launchd` or a foreground
terminal) and supply `bluetooth.serial_port` in the config as described in
the README.

The macOS dev shell does not pull in ALSA, dbus, PipeWire, or glibc bindgen
headers (cpal uses CoreAudio, and the transport is `tokio-serial` rather than
`bluer`). It sets `LIBRARY_PATH` to expose `libsonic` and `onnxruntime` to the
linker, and `RUSTFLAGS=-C link-arg=-lsonic` for the final binary link.

## Checks

`nix flake check` builds the package, evaluates NixOS/Home Manager module
examples, invokes the real package through the NixOS module-generated
`ExecStart` with `--help`, validates `examples/config.validation.toml` with
the real package's `--check-config` mode, invokes the NixOS module-generated
`ExecStart` with `services.ptt-audio-menu.configPath` plus `--check-config`,
invokes the Home Manager user-service-generated `ExecStart` with both `--help`
and `programs.ptt-audio-menu.configPath` plus `--check-config`, and runs a NixOS
VM smoke check for the system service with a dummy executable. These
real-package checks exit before TTS rendering and Bluetooth setup. The VM check
verifies systemd wiring, configured arguments, and service environment without
requiring Bluetooth hardware.

Hardware connection and audio playback remain host/runtime checks because they
depend on the paired RSM, BlueZ, and the host audio stack.

On small CI or sandbox stores, the full check can fail before reaching runtime
coverage because the real package closure includes Rust, audio/TTS, eSpeak, and
ONNX Runtime dependencies plus the VM closure. In that case,
`nix flake check --no-build` and the lightweight module checks still cover
evaluation, but a full `nix flake check` should be rerun on a larger Nix store.
