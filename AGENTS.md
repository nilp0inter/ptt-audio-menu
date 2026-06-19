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
- Run Rust verification inside the shell:

```sh
nix develop --command cargo fmt --check
nix develop --command cargo test
nix develop --command cargo check
```

- Running `cargo test` directly on a host without dbus development files fails while building `libdbus-sys`.
- The dev shell now also carries native audio/TTS build inputs for Kira/Piper: ALSA, OpenSSL, eSpeak-ng, libclang, glibc bindgen headers, and CMake. It sets `PIPER_ESPEAKNG_DATA_DIRECTORY` to the Nix-provided eSpeak data directory for local runs.

## Current Code Layout

- `src/main.rs`: application entry point, config/cache/prompt-catalog startup wiring, TTS prerendering, audio playback wiring, hardcoded target device address, RFCOMM read loop, raw diagnostic output, parser/menu/action event output.
- `src/audio.rs`: Kira-backed interrupt-latest WAV prompt playback and stop-current handling.
- `src/transport.rs`: BlueZ session/adapter setup, RFCOMM Serial Port profile registration, concurrent `connect_profile` and profile request acceptance.
- `src/parser.rs`: token-scanning serial parser, typed raw button/action events, and parser unit tests.
- `src/input.rs`: hardware event normalization, active/control mode tracking, SOS long-press suppression, PTT threshold handling, and input semantics unit tests.
- `src/config.rs`: CLI config path resolution helpers, serde-backed TOML schema, validation, and config unit tests.
- `src/menu.rs`: menu phase/focus state, active/global control tab resolution, input-to-action outcome mapping, and menu state unit tests.
- `src/actions.rs`: action ID dispatch, immediate internal effects for no-op/tool switching/control exit, deferred command/internal effects, and action dispatcher unit tests.
- `src/commands.rs`: async argv-list command runner, serial execution guard, optional timeout handling, Unix process-group cancellation, and command runner unit tests.
- `src/tts.rs`: TTS cache directory resolution, stable prompt hash keys, placeholder Piper settings, prompt text collection, Piper rendering to PCM WAV, WAV cache read/write helpers, and TTS cache unit tests.
