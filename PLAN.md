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

Status: complete

The core runtime now matches the design closely enough that new feature work should pause. The next step is to improve system integration through Nix modules:

- Inspect the current flake outputs and package definition.
- Add a NixOS module that can install the package and define a systemd service with configurable package, config path, user/group, Bluetooth/audio-related service settings, and environment.
- Add a Home Manager module for user-level installation and optional user service wiring where practical.
- Add flake checks that evaluate the package and module examples without requiring Bluetooth hardware.
- Document module usage and any intentionally deferred hardware/e2e checks.

## Leg 13: NixOS Module Runtime Smoke Checks

Status: complete

The flake now builds the package and evaluates module examples, but it still does not exercise a NixOS VM runtime path. The next integration step should stay focused on module/service behavior rather than new application features:

- Add a minimal NixOS VM check that enables the service with a dummy executable package and verifies the generated systemd unit can start.
- Add a Home Manager-compatible evaluation or activation check if a lightweight pattern is available without adding a hard Home Manager flake input.
- Keep Bluetooth/RSM/audio hardware tests documented as manual or host-specific until there is a reliable test device setup.

## Leg 14: Real Package NixOS CLI Smoke Checks

Status: complete

The module can now be boot-tested with a dummy executable, but the VM path still avoids the real Rust binary because the normal runtime connects Bluetooth hardware. The next integration step should exercise the packaged binary in a hardware-free mode:

- Add a NixOS VM or derivation check that uses the real package with a non-Bluetooth CLI path such as `--help`.
- Verify the NixOS module can install and invoke the real package without needing RSM hardware.
- Keep the existing dummy service VM check for long-running systemd service semantics.

## Leg 15: Configuration Fixture Integration Checks

Status: complete

The flake now checks the real packaged binary through the NixOS module with `--help`, but the package check still does not exercise config-file loading outside unit tests. The next integration step should remain hardware-free and focus on the TOML boundary:

- Add a representative example config fixture with dummy Piper model/config paths suitable for validation tests.
- Add a derivation or NixOS check that runs the real binary far enough to validate CLI/config behavior without attempting Bluetooth, if a narrow CLI path is introduced.
- If that requires a new CLI mode, keep it explicitly diagnostic and avoid changing the runtime menu behavior.

## Leg 16: Config Check Through Module Wiring

Status: complete

The real package now has a hardware-free config validation path, but the NixOS module still only invokes the real package through `--help`. The next integration step should keep focusing on module behavior:

- Add a NixOS module check that wires the example config path through `services.ptt-audio-menu.configPath` and appends `--check-config` through `extraArgs`.
- Run the module-generated `ExecStart` with the real package to prove module argument ordering works for a real config load.
- Keep this as a derivation check, not a VM, because no long-running service behavior or hardware access is needed.

## Leg 17: Home Manager Service CLI Smoke Checks

Status: complete

The NixOS module now has real-package hardware-free checks for both `--help` and config validation through module-generated `ExecStart`. The next module integration step should give the Home Manager service path the same kind of lightweight coverage without adding a Home Manager flake input:

- Extend the existing Home Manager-compatible eval check or add a sibling derivation that uses the generated user service `ExecStart`.
- Use the real package with a hardware-free CLI path such as `--help`, and, if practical, a config fixture with `--check-config`.
- Keep the check lightweight and avoid starting a user systemd service, because Bluetooth/audio permissions remain host-specific.

## Leg 18: Full Flake Integration Recheck

Status: complete

The Home Manager service path now has the same hardware-free real-package coverage as the NixOS module path. The next step should avoid new runtime features and focus on proving the complete integration surface still holds together:

- Run the full flake check matrix, including package, module, real-package CLI/config checks, and the NixOS service VM when `/nix/store` capacity permits.
- If store pressure prevents the full run, record the exact check subset that passed and any cleanup needed before retrying.
- Keep any edits limited to check reliability, documentation accuracy, or small Nix-module integration fixes discovered by the full run.

Result: attempted `nix flake check` after freeing `/nix/store` to roughly 3.2 GiB available, but the full run failed while realizing the real package dependency chain with `No space left on device` during eSpeak/mbrola/Rust toolchain substitution. Verified the flake evaluation surface with `nix flake check --no-build`, whitespace with `git diff --check`, and the lightweight module checks with `nix build .#checks.x86_64-linux.nixos-module .#checks.x86_64-linux.home-manager-module`.

## Leg 19: Store-Headroom Full Integration Retry

Status: complete

The full integration surface is structurally evaluated, but this machine still needs enough `/nix/store` capacity to realize the real Rust package plus VM closure in one run:

- Start by checking `df -h /nix/store`; aim for substantially more than 3.2 GiB free before retrying `nix flake check`.
- Re-run `nix flake check` and the explicit real-package checks when store capacity permits.
- If the full run still fails for reasons other than store pressure, limit edits to check reliability or Nix integration fixes discovered by the failing check.

Result: `/nix/store` is currently a 3.9 GiB overlay with only 2.6 GiB available, so this environment cannot provide substantially more than the 3.2 GiB that already failed in Leg 18. Verified the flake evaluation surface with `nix flake check --no-build`, whitespace with `git diff --check`, and the lightweight module checks with `nix build .#checks.x86_64-linux.nixos-module .#checks.x86_64-linux.home-manager-module`.

## Leg 20: Expanded-Store Full Integration Run

Status: pending (blocked locally)

The implementation and module integration are structurally covered, but the full package plus VM check needs a larger Nix store than this current 3.9 GiB overlay:

- Run the full `nix flake check` on a machine or mount with enough `/nix/store` capacity for the real Rust/TTS/ONNX package closure plus the NixOS VM closure.
- Include the explicit real-package checks and `nix build .#checks.x86_64-linux.nixos-service-vm` if they are not already covered by the full check result.
- If capacity is available and a check fails for a real build, module, or runtime-smoke reason, keep edits limited to check reliability or Nix integration fixes.

Latest local audit: `/nix/store` is still a 3.9 GiB overlay with 2.6 GiB available, which is less headroom than the roughly 3.2 GiB that already failed during Leg 18. Re-running the full package plus VM closure here would only reproduce the capacity failure. Re-verified the flake evaluation surface with `nix flake check --no-build`, whitespace with `git diff --check`, and the lightweight module checks with `nix build .#checks.x86_64-linux.nixos-module .#checks.x86_64-linux.home-manager-module`.

Follow-up audit: `/nix/store` remains a 3.9 GiB overlay with 2.6 GiB available. The local blocker is unchanged, so the full package plus VM closure is still deferred to a larger store. Re-verified `nix flake check --no-build`, `git diff --check`, and `nix build .#checks.x86_64-linux.nixos-module .#checks.x86_64-linux.home-manager-module`.
