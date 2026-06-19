# Agent Notes

## Repository Workflow

- Read `PROMPT.md`, `DESIGN.md`, `PLAN.md`, `EXECUTION.md`, and `AGENTS.md` at the start of each session when present.
- Keep sessions short and focused.
- Use `rg` / `rg --files` for repository inspection.
- Use `apply_patch` for manual source and documentation edits.
- Leave unrelated untracked files alone; `nixos.qcow2` was present before this session.
- This repository may not have Git author identity configured on a fresh machine. The local config was set from the latest commit author during the 2026-06-19 session.

## Build and Test

- The project uses a Nix flake dev shell.
- The flake now also exposes `packages.${system}.default`, `checks`, `nixosModules.default`, and `homeManagerModules.default`.
- Build/evaluate the integration surface with:

```sh
nix build .#packages.x86_64-linux.default
nix flake check
nix build .#checks.x86_64-linux.nixos-real-package-help
nix build .#checks.x86_64-linux.nixos-real-package-config
nix build .#checks.x86_64-linux.nixos-service-vm
```

- `nix flake check` may warn that `homeManagerModules` is an unknown non-core flake output; this is expected for Home Manager consumers.
- `checks.${system}.nixos-real-package-help` evaluates the NixOS module with the real package, verifies the package is installed, and invokes the module-generated `ExecStart` with `--help` so no Bluetooth hardware is required.
- `checks.${system}.real-package-config-fixture` runs the real packaged binary with `--config examples/config.validation.toml --check-config`, validating TOML loading and references without TTS rendering or Bluetooth hardware.
- `checks.${system}.nixos-real-package-config` evaluates the NixOS module with the real package, wires `services.ptt-audio-menu.configPath` plus `extraArgs = [ "--check-config" ]`, and invokes the module-generated `ExecStart` from the source tree so the fixture's relative voice paths resolve.
- `checks.${system}.nixos-service-vm` boots a NixOS VM with a dummy long-running `ptt-audio-menu` executable and verifies systemd arguments/environment. The minimal VM must define the module's default `audio` and `bluetooth` supplementary groups.
- The Nix store overlay on this machine can fill while building VM checks. Check `df -h /nix/store`; a targeted or interrupted `nix-store --gc` may be needed before rerunning.
- Run Rust verification inside the shell:

```sh
nix develop --command cargo fmt --check
nix develop --command cargo test
nix develop --command cargo check
```

- Running `cargo test` directly on a host without dbus development files fails while building `libdbus-sys`.
- The dev shell now also carries native audio/TTS build inputs for Kira/Piper: ALSA, OpenSSL, eSpeak-ng, libclang, glibc bindgen headers, and CMake. It sets `PIPER_ESPEAKNG_DATA_DIRECTORY` to the Nix-provided eSpeak data directory for local runs.
- The Nix package enables `ort`'s pkg-config path through a direct dependency and uses Nixpkgs `onnxruntime`. It also links `sonic` explicitly because `espeak-rs-sys` can find libsonic during CMake configuration without emitting `-lsonic` to Cargo.

## Current Code Layout

- `src/main.rs`: application entry point, stdout tracing initialization, reloadable config/cache/prompt-catalog runtime state, TTS prerendering, audio playback wiring, hardcoded target device address, RFCOMM read loop, command-completion feedback handling, parser/menu/action event logging.
- `examples/config.validation.toml`: representative validation-only config fixture used by the real-package config flake check.
- `src/audio.rs`: Kira-backed interrupt-latest WAV prompt playback and stop-current handling.
- `src/transport.rs`: BlueZ session/adapter setup, RFCOMM Serial Port profile registration, concurrent `connect_profile` and profile request acceptance, and connection lifecycle tracing.
- `src/parser.rs`: token-scanning serial parser, typed raw button/action events, and parser unit tests.
- `src/input.rs`: hardware event normalization, active/control mode tracking, SOS long-press suppression, PTT threshold handling, and input semantics unit tests.
- `src/config.rs`: CLI config path resolution helpers, serde-backed TOML schema, validation, and config unit tests.
- `src/menu.rs`: menu phase/focus state, active/global control tab resolution, input-to-action outcome mapping, and menu state unit tests.
- `src/actions.rs`: action ID dispatch, immediate internal effects for no-op/tool switching/control exit, deferred command/internal effects, and action dispatcher unit tests.
- `src/commands.rs`: async argv-list command runner, serial execution guard, optional timeout handling, Unix process-group cancellation, and command runner unit tests.
- `src/tts.rs`: TTS cache directory resolution, stable prompt hash keys, placeholder Piper settings, prompt text collection, Piper rendering to PCM WAV, WAV cache read/write helpers, and TTS cache unit tests.
- `nix/package.nix`: Nix package derivation for the Rust binary and native audio/TTS/ONNX dependencies.
- `nix/nixos-module.nix`: NixOS service module for system-level installation and systemd wiring.
- `nix/home-manager-module.nix`: Home Manager module for package installation and optional user-level service wiring.
- `docs/nix-modules.md`: NixOS/Home Manager usage examples and check limitations.
