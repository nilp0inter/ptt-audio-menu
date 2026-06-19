# Execution Log

## 2026-06-19

- Read `PROMPT.md` and `DESIGN.md`.
- Found `PLAN.md`, `EXECUTION.md`, and `AGENTS.md` were missing and created them.
- Found the implementation is currently a single Rust binary containing BlueZ RFCOMM connection logic, serial token parsing, and parser tests.
- Selected the first implementation leg: split transport and parser code into modules without changing behavior.
- Initial `cargo test` outside the Nix dev shell failed because `dbus-1.pc` was not available in `PKG_CONFIG_PATH`; verification should run through `nix develop` or an equivalent environment with dbus development files.
- Moved token scanning and parser tests to `src/parser.rs`.
- Moved BlueZ RFCOMM profile registration and connection setup to `src/transport.rs`.
- Reduced `src/main.rs` to device connection, byte read loop, raw diagnostics, and parser event output.
- Verified with `nix develop --command cargo fmt --check`.
- Verified with `nix develop --command cargo test` (6 parser tests passed).
- Verified with `nix develop --command cargo check`.
- Commit initially failed because Git author identity was not configured; set the local repo identity from the latest commit author.
- Created commit `551d0af` (`Split parser and transport modules`).
- `git push` failed because GitHub credentials were unavailable in the non-interactive environment: `could not read Username for 'https://github.com'`.
- Read `PROMPT.md`, `DESIGN.md`, `PLAN.md`, `EXECUTION.md`, and `AGENTS.md` at the start of the next session.
- Selected pending Leg 2: hardware event normalization.
- Changed parser events from string button/action fields to typed `Button` and `RawAction` enums while preserving token and physical button number diagnostics.
- Added `src/input.rs` with hardware mode tracking, active/control transitions, active-mode PTT hold threshold handling, immediate control-mode PTT selection, SOS short suppression after long press, and control-mode volume scrolling.
- Wired `InputNormalizer` into `src/main.rs` so the diagnostic loop prints both raw parser events and normalized input events.
- Verified with `nix develop --command cargo fmt --check`.
- Verified with `nix develop --command cargo test` (13 unit tests passed).
- Verified with `nix develop --command cargo check`.
- Marked Leg 2 complete and added Leg 3 for TOML config loading and validation.
- Created local commit `Add hardware input normalization`.
- `git push` failed because the configured SSH identity was missing: `/home/nil/.ssh/assistant-cage.pub`.
