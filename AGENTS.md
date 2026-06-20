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
nix build .#checks.x86_64-linux.home-manager-real-package-help
nix build .#checks.x86_64-linux.home-manager-real-package-config
nix build .#checks.x86_64-linux.nixos-service-vm
```

- `nix flake check` may warn that `homeManagerModules` is an unknown non-core flake output; this is expected for Home Manager consumers.
- `checks.${system}.nixos-real-package-help` evaluates the NixOS module with the real package, verifies the package is installed, and invokes the module-generated `ExecStart` with `--help` so no Bluetooth hardware is required.
- `checks.${system}.real-package-config-fixture` runs the real packaged binary with `--config examples/config.validation.toml --check-config`, validating TOML loading and references without TTS rendering or Bluetooth hardware.
- `checks.${system}.nixos-real-package-config` evaluates the NixOS module with the real package, wires `services.ptt-audio-menu.configPath` plus `extraArgs = [ "--check-config" ]`, and invokes the module-generated `ExecStart` from the source tree so the fixture's relative voice paths resolve.
- `checks.${system}.home-manager-real-package-help` evaluates the Home Manager module with the real package and invokes the generated user service `ExecStart` with `--help` without starting user systemd.
- `checks.${system}.home-manager-real-package-config` evaluates the Home Manager module with the real package, wires `programs.ptt-audio-menu.configPath` plus `extraArgs = [ "--check-config" ]`, and invokes the generated user service `ExecStart` from the source tree so the fixture's relative voice paths resolve.
- `checks.${system}.nixos-service-vm` boots a NixOS VM with a dummy long-running `ptt-audio-menu` executable and verifies systemd arguments/environment. The minimal VM must define the module's default `audio` and `bluetooth` supplementary groups.
- The Nix store overlay on this machine can fill while building VM checks. Check `df -h /nix/store`; a targeted or interrupted `nix-store --gc` may be needed before rerunning.
- During the 2026-06-19 Leg 18 recheck, `nix flake check` still filled `/nix/store` with about 3.2 GiB free while realizing the real package dependency chain around Rust/eSpeak/mbrola. For a full package plus VM run, free substantially more space first; `nix flake check --no-build` and the lightweight `nixos-module`/`home-manager-module` checks fit in less space.
- During the Leg 19 retry, `/nix/store` was a 3.9 GiB overlay with 2.6 GiB available. This environment cannot provide enough headroom for the full package plus VM closure; run full `nix flake check` on a larger store instead of repeatedly retrying here.
- During the Leg 20 audit, `/nix/store` was unchanged at a 3.9 GiB overlay with 2.6 GiB available. Treat the full `nix flake check` as locally blocked in this environment unless the store is expanded; use `nix flake check --no-build` plus lightweight module checks for local structural verification.
- During the Leg 20 follow-up audit, `/nix/store` was still a 3.9 GiB overlay with 2.6 GiB available. The local blocker is unchanged; do not retry the full package plus VM closure here unless store capacity changes.
- During the next Leg 20 follow-up audit, `/nix/store` remained a 3.9 GiB overlay with 2.6 GiB available. Continue treating the full package plus VM closure as locally blocked on this machine.
- During the current Leg 20 audit, `/nix/store` remained a 3.9 GiB overlay with 2.6 GiB available. Continue treating the full package plus VM closure as locally blocked on this machine.
- During the latest Leg 20 follow-up audit, `/nix/store` remained a 3.9 GiB overlay with 2.6 GiB available. Continue treating the full package plus VM closure as locally blocked on this machine; use `nix flake check --no-build` and lightweight module checks for local verification.
- Run Rust verification inside the shell:

```sh
nix develop --command cargo fmt --check
nix develop --command cargo test
nix develop --command cargo check
```

- Running `cargo test` directly on a host without dbus development files fails while building `libdbus-sys`.
- The dev shell now also carries native audio/TTS/recording build inputs for Kira/Piper/CPAL: ALSA, OpenSSL, eSpeak-ng, libclang, glibc bindgen headers, and CMake. It sets `PIPER_ESPEAKNG_DATA_DIRECTORY` to the Nix-provided `share` directory, because `espeak-rs` appends `espeak-ng-data` internally.
- The Nix package enables `ort`'s pkg-config path through a direct dependency and uses Nixpkgs `onnxruntime`. It also links `sonic` explicitly because `espeak-rs-sys` can find libsonic during CMake configuration without emitting `-lsonic` to Cargo.
- The app now also uses `parakeet-rs` for built-in Parakeet TDT processing. Model files are not downloaded or packaged; configs point at a local `model_dir`.

## Current Code Layout

- `src/main.rs`: application entry point, stdout tracing initialization, reloadable config/cache/prompt-catalog runtime state, TTS prerendering, audio playback wiring, hardcoded target device address, RFCOMM read loop, command-completion feedback handling, parser/menu/action event logging.
- `examples/config.validation.toml`: representative validation-only config fixture used by the real-package config flake check.
- `examples/config.personal-workflow.toml`: real-use workflow example. It uses the real local Piper voice paths from `/tmp`, models Handy plain and polished dictation as separate tools, uses `globals.active_ptt_trigger = "hold_toggle"`, intentionally omits command feedback for Handy PTT/cancel commands, and adds a separate Daily Log tool backed by native recording packets and Parakeet TDT.
- `examples/daily_log_render.py`: simple renderer invoked by the Daily Log packet processor. It reads a per-day JSON file and writes static HTML.
- `src/audio.rs`: Kira-backed interrupt-latest WAV prompt playback and stop-current handling. On startup, derives a PipeWire sink node name from the hardcoded Bluetooth MAC (`bluez_output.<underscored_mac>.1`), sets the `PIPEWIRE_NODE` environment variable before Kira initializes its cpal stream, and falls back to the system default sink when no device is configured. cpal device enumeration cannot see Bluetooth A2DP sinks because they are PipeWire-native devices invisible to cpal's ALSA backend.
- `src/transport.rs`: BlueZ session/adapter setup, RFCOMM Serial Port profile registration, concurrent `connect_profile` and profile request acceptance, and connection lifecycle tracing.
- `src/parser.rs`: token-scanning serial parser, typed raw button/action events, and parser unit tests.
- `src/input.rs`: hardware event normalization, active/control mode tracking, SOS long-press suppression, active PTT trigger handling (`release_after_hold`, `press`, `hold_toggle`), active PTT press/release edge events for recording packets, and input semantics unit tests.
- `src/config.rs`: CLI config path resolution helpers, serde-backed TOML schema (including `AudioConfig` with optional `device` field for future audio sink configurability and `globals.active_ptt_trigger`), validation, and config unit tests.
- `src/menu.rs`: menu phase/focus state, active/global control tab resolution, remembered valid control focus across exit/tool switches, tab-vs-item focus outcomes, input-to-action outcome mapping, and menu state unit tests.
- `src/actions.rs`: action ID dispatch, immediate internal effects for no-op/tool switching/control exit, deferred command/internal effects, recording packet edge dispatch, and action dispatcher unit tests.
- `src/commands.rs`: async argv-list command runner, serial execution guard, optional timeout handling, Unix process-group cancellation, and command runner unit tests.
- `src/recorder.rs`: CPAL default-input recording, in-memory capture, mono mixing, linear resampling, and 16 kHz mono PCM WAV writing.
- `src/packets.rs`: durable recording packet queues, operational metadata, state-directory movement, retry/backoff, dead-letter handling, stale processing recovery, and built-in Daily Log Parakeet processing.
- `src/tts.rs`: TTS cache directory resolution, stable prompt hash keys, placeholder Piper settings, prompt text collection, Piper rendering to PCM WAV, WAV cache read/write helpers, and TTS cache unit tests.
- `nix/package.nix`: Nix package derivation for the Rust binary and native audio/TTS/ONNX dependencies.
- `nix/nixos-module.nix`: NixOS service module for system-level installation and systemd wiring.
- `nix/home-manager-module.nix`: Home Manager module for package installation and optional user-level service wiring.
- `docs/nix-modules.md`: NixOS/Home Manager usage examples and check limitations.
