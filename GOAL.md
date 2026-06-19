# Configurable Audio UI Engine

## Summary
Build a TOML-configured engine on top of the existing RFCOMM parser. The user-facing model is a tabbed audio UI: one active tool, per-tool local tabs, global tabs, focused items, and spoken feedback rendered with `piper-rs` and played internally with `kira`.

Non-goals for this phase:
- No systemd/NixOS service.
- No automatic pairing/discovery.
- No config schema version field.
- No state persistence.
- No event-binding-only raw hardware config model.
- No external audio playback command backend.

## Configuration Interface
Use `--config <path>`, otherwise load `$XDG_CONFIG_HOME/ptt-audio-menu/config.toml` or `~/.config/ptt-audio-menu/config.toml`.

TOML concepts:
- `default_tool` is required.
- IDs are strict lowercase slugs and unique within their namespace.
- Voice config uses explicit Piper model/config paths.
- TTS cache defaults to `$XDG_CACHE_HOME/ptt-audio-menu/tts`, overrideable in config.
- Global defaults include active PTT hold threshold; tools may override it.
- Tools define active hooks and local control tabs.
- Global tabs are available from every tool.
- Items define label text plus primary/alternate actions.

Action types:
- `internal`: `switch_tool`, `speak`, `noop`, `exit_control`, `reload_config`, `stop_audio`, `cancel_running_action`.
- `command`: argv-list only, no shell string. Inherits process cwd/env, with optional per-action `cwd` and extra env.
- Commands have no timeout by default; per-action timeout is optional.
- Per-action feedback is optional: start/success/failure spoken labels.

## Runtime Behavior
Input state:
- Active Phase maps PTT to press/release hooks after the active-mode hold threshold.
- Control Phase disables PTT debounce; PTT selects focused item and returns to Active Phase.
- Group enters Control Phase from Active Phase, then cycles tabs while in Control Phase.
- Volume Up/Down scroll focused item in Control Phase.
- No software idle timeout; hardware state is authoritative.
- Active SOS exposes `sos_short`/`sos_long`; engine suppresses short after hardware long.
- Control SOS runs focused-item alternate short/long actions without leaving Control Phase.

Audio:
- At startup, validate config and prerender all configured TTS labels before connecting hardware.
- Cache WAV PCM prompts using a full input hash: text, voice/model paths, Piper settings, output format, and app version.
- On successful startup, speak the active tool label.
- Navigation speech uses interrupt-latest semantics.
- Playback is internal via `kira`.

Actions:
- Async actions run one at a time in a serial queue.
- Shell commands spawn in their own process group.
- `cancel_running_action` terminates the running command process group where supported.
- `reload_config` validates and prerenders the new config; on failure, speak/log failure and exit process.

Logging:
- Replace ad hoc `println!` diagnostics with `tracing` stdout logging.
- Keep raw RFCOMM/event logs available at debug level.

## Implementation Changes
- Split current single-file binary into modules for transport/parser, hardware event normalization, config loading/validation, menu state, TTS cache, audio playback, and action execution.
- Preserve existing parser behavior and tests.
- Add dependencies for config/CLI/logging/TTS/audio/cache support: `serde`, `toml`, `clap`, `directories` or equivalent XDG helper, `tracing`, `tracing-subscriber`, `sha2`, `piper-rs`, `kira`.
- Add a sample config file documenting the minimal working tabbed UI.

## Test Plan
- Parser tests remain unchanged and passing.
- Config validation tests:
  - missing `default_tool`
  - duplicate/invalid slug IDs
  - unknown tool/action references
  - invalid Piper paths
  - command action rejects shell-string-only config
- Menu state tests:
  - Active to Control via Group
  - tab cycling via Group
  - item scrolling via Volume
  - PTT selection exits Control Phase
  - Control SOS alternate action stays in Control Phase
  - no idle timeout transition
- Input semantics tests:
  - Active PTT threshold suppresses short accidental taps
  - Control PTT bypasses threshold
  - SOS short suppressed after long signal
- TTS cache tests:
  - same full input hash reuses WAV
  - text/model/settings changes produce different cache entries
- Action executor tests:
  - command queue is serial
  - optional timeout works when configured
  - cancel terminates process group
  - reload failure exits instead of partially applying config
