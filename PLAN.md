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

Status: pending

The implementation now has transport, parsing, and hardware input normalization, but runtime behavior is still hardcoded and diagnostic. The next step is to introduce the TOML configuration boundary before building menu state:

- Add CLI/config path resolution for `--config`, `$XDG_CONFIG_HOME/ptt-audio-menu/config.toml`, and `~/.config/ptt-audio-menu/config.toml`.
- Define serde-backed config structs for `default_tool`, voice paths, global defaults, tools, tabs, items, and actions.
- Validate strict lowercase slug IDs, uniqueness within namespaces, required `default_tool`, known tool/action references, Piper model/config paths, and argv-list command actions.
- Keep the runtime loop diagnostic for now, but load and validate config at startup.
- Add focused config validation tests from `DESIGN.md`.
