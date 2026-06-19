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

Status: pending

Menu state now emits action IDs, but the runtime still only prints diagnostics. The next step is to introduce the action dispatch boundary while keeping audio/TTS and full command lifecycle small enough for a later leg:

- Add an `actions` module that indexes validated config actions by ID and resolves `MenuOutcome::Action` values.
- Implement internal action effects that are purely stateful or diagnostic for now: `noop`, `switch_tool`, and `exit_control`.
- Keep external command actions as queued/recognized but not fully executed until the command runner leg.
- Update `MenuState` as needed so internal `switch_tool` and `exit_control` can mutate menu state through explicit methods instead of bypassing menu invariants.
- Add focused tests for action lookup, unknown action defense, switching tools, and exiting control.
