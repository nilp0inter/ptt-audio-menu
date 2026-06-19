# Implementation Plan

## Leg 1: Module Split Foundation

Status: complete

The current implementation is a single-file binary that already covers the hardcoded BlueZ RFCOMM profile connection flow and token-scanning parser described in `DESIGN.md`. The next step is to split this into the first architecture modules without changing runtime behavior:

- Move serial token scanning and parser tests into `src/parser.rs`.
- Move BlueZ/RFCOMM connection setup into `src/transport.rs`.
- Keep `src/main.rs` as the runtime loop that reads bytes, logs raw input, and prints normalized parser events.

This keeps the next leg small while preparing for later hardware event normalization, config loading, menu state, TTS/audio, and action execution modules.

## Leg 2: Hardware Event Normalization

Status: complete

Add the layer between raw parser events and menu state:

- Define typed raw button events instead of stringly typed event fields.
- Track SOS press cycles so `C:SR*` emits a short SOS action only when no `C:SOS*` happened during the same press.
- Track active/control hardware mode transitions from Group/PTT semantics.
- Apply active-mode PTT hold threshold while keeping control-mode PTT immediate.
- Add focused unit tests for the input semantics listed in `DESIGN.md`.

## Leg 3: Configuration Loading and Validation

Status: complete

The implementation now has transport, parsing, and hardware input normalization, but runtime behavior is still hardcoded and diagnostic. The next step is to introduce the TOML configuration boundary before building menu state:

- Add CLI/config path resolution for `--config`, `$XDG_CONFIG_HOME/ptt-audio-menu/config.toml`, and `~/.config/ptt-audio-menu/config.toml`.
- Define serde-backed config structs for `default_tool`, voice paths, global defaults, tools, tabs, items, and actions.
- Validate strict lowercase slug IDs, uniqueness within namespaces, required `default_tool`, known tool/action references, Piper model/config paths, and argv-list command actions.
- Keep the runtime loop diagnostic for now, but load and validate config at startup.
- Add focused config validation tests from `DESIGN.md`.

## Leg 4: Menu State Foundation

Status: complete

The program now validates configuration before connecting Bluetooth and has normalized hardware input events, but it still only prints diagnostics. The next step is to add the menu state layer without executing actions or producing audio yet:

- Build a `menu` module that initializes from validated config and tracks the active tool, active/control phase, selected tab, and selected item.
- Map `InputEvent` values to menu outcomes: enter control, cycle tabs, scroll items, select primary action, and SOS alternate actions that stay in control mode.
- Resolve global tabs plus the active tool's local tabs into the control tab list.
- Return action IDs as outcomes instead of executing them.
- Add unit tests for active-to-control, tab cycling, item scrolling, PTT selection exiting control, control SOS alternate action staying in control, and no idle timeout transition.

## Leg 5: Action Dispatch Foundation

Status: complete

Menu state now emits action IDs, but the runtime still only prints diagnostics. The next step is to introduce the action dispatch boundary while keeping audio/TTS and full command lifecycle small enough for a later leg:

- Add an `actions` module that indexes validated config actions by ID and resolves `MenuOutcome::Action` values.
- Implement internal action effects that are purely stateful or diagnostic for now: `noop`, `switch_tool`, and `exit_control`.
- Keep external command actions as queued/recognized but not fully executed until the command runner leg.
- Update `MenuState` as needed so internal `switch_tool` and `exit_control` can mutate menu state through explicit methods instead of bypassing menu invariants.
- Add focused tests for action lookup, unknown action defense, switching tools, and exiting control.

## Leg 6: Command Runner Foundation

Status: complete

Action dispatch now recognizes command actions but does not execute them. The next step is to add the first real action backend while keeping audio feedback and reload semantics for later:

- Add an async command runner that receives queued command effects and runs argv-list commands without a shell.
- Ensure command actions run serially, preserving the one-action-at-a-time runtime model.
- Support optional per-action timeout.
- Start the process in its own process group on Unix so later cancellation can target the group.
- Wire `cancel_running_action` to terminate the running command process group where supported.
- Add focused tests for serial command execution, timeout behavior, and cancellation.

## Leg 7: TTS Cache Foundation

Status: complete

Commands now execute through a serial backend, but feedback is still diagnostic stdout. The next step is to add the cache boundary needed before real audio playback:

- Add a `tts` module that computes stable cache keys from prompt text, voice model/config paths, Piper settings placeholder data, output format, and app version.
- Resolve the TTS cache directory from config or `$XDG_CACHE_HOME/ptt-audio-menu/tts`.
- Add a cache lookup/write interface that can store WAV bytes without invoking Piper yet.
- Update startup validation/runtime wiring only enough to construct the cache.
- Add focused tests that identical full inputs reuse the same cache path and text/model/settings changes produce different cache paths.

## Leg 8: Prompt Catalog Foundation

Status: complete

The runtime creates a TTS cache, but audio rendering/playback is a large enough step to split. The first boundary is now complete:

- Collect prompt text from tool labels, tab labels, item labels, internal speak actions, and command feedback labels.
- Deduplicate prompt text in stable order and ignore empty/whitespace-only prompts.
- Wire startup to build the prompt catalog before connecting Bluetooth.
- Preserve existing TTS cache key tests and add focused prompt collection coverage.

## Leg 9: TTS Rendering and Audio Playback Foundation

Status: complete

The runtime now knows which prompt texts need speech, but it still does not render prompts or play audio. The next step is to add the first real feedback backend:

- Add Piper rendering behind the existing TTS cache so configured prompt texts can be prerendered at startup.
- Add a small audio playback module using `kira` with interrupt-latest semantics for navigation speech.
- Wire startup to prerender prompts before connecting Bluetooth and speak the active tool label after successful startup.
- Add navigation speech for control entry/focus changes using cached prompt audio.
- Keep command feedback and reload behavior minimal if needed, but preserve tests around cache key reuse and prompt collection.

## Leg 10: Command Feedback and Runtime Control Actions

Status: complete

The program now prerenders configured prompts and uses cached audio for startup, navigation, `speak`, tool switching, and `stop_audio`, but command feedback and reload semantics are still minimal. The next step is to finish the remaining runtime action effects:

- Speak command feedback labels for start, success, and failure using cached prompt audio.
- Preserve serial command execution while allowing feedback playback to occur around background command completion.
- Implement `reload_config` so it validates and prerenders the replacement config, applies it only on success, and exits/logs/speaks failure on validation or render errors.
- Keep `cancel_running_action` and `stop_audio` behavior intact.
- Add focused tests around feedback effect data and any reload helper that can be tested without Bluetooth hardware.

## Leg 11: Tracing Logging Foundation

Status: complete

Most core runtime behavior is now wired, but diagnostics still use ad hoc `println!` output. The next step is to align runtime observability with `DESIGN.md` while keeping raw Bluetooth/event logs available:

- Add `tracing` and `tracing-subscriber` dependencies.
- Initialize stdout logging at startup, with an environment filter suitable for normal info-level operation and debug-level raw diagnostics.
- Replace runtime `println!` diagnostics in `main.rs` with structured `tracing` calls.
- Keep raw RFCOMM chunks, parser events, input events, menu outcomes, action effects, command completions, and reload failures visible at debug/info levels as appropriate.
- Preserve current behavior and tests; add minimal coverage only if helper code is introduced.

## Leg 12: NixOS and Home Manager Module Foundation

Status: pending

The core runtime now matches the design closely enough that new feature work should pause. The next step is to improve system integration through Nix modules:

- Inspect the current flake outputs and package definition.
- Add a NixOS module that can install the package and define a systemd service with configurable package, config path, user/group, Bluetooth/audio-related service settings, and environment.
- Add a Home Manager module for user-level installation and optional user service wiring where practical.
- Add flake checks that evaluate the package and module examples without requiring Bluetooth hardware.
- Document module usage and any intentionally deferred hardware/e2e checks.
