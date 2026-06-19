# Design Document: Screenless Audio-First UI Engine

## 1. Project Overview
The objective is to design a Rust software engine that turns a hardware **Bluetooth Remote Speaker Microphone (RSM)** into an interactive, screenless, voice-based interface. The user navigates features, executes commands, and switches between various tool states using physical button inputs as controls, while receiving audio/Text-to-Speech (TTS) feedback. 

The system is configured via TOML and operates on a "Tabbed Menu" abstraction, translating raw Bluetooth RFCOMM serial events into structured, context-aware actions.

---

## 2. Hardware Profile & Connection

### Device Specifications
- **Device Name:** `B02PTT-FF01`
- **MAC Address:** `00:02:5B:55:FF:01`
- **Service:** Bluetooth Serial Port Profile, UUID `00001101-0000-1000-8000-00805f9b34fb`
- **SDP Service Name:** `GAIA`

### Connection Approach
The program uses BlueR and BlueZ profile APIs instead of opening a hardcoded RFCOMM channel. The connection flow is as follows:
1. Create a BlueR `Session` and get the default Bluetooth adapter.
2. Power the adapter on and get the target device by MAC.
3. Register a local RFCOMM client profile for the Serial Port UUID.
4. Start `Device::connect_profile()` and concurrently wait for `ProfileHandle::next()`. (Waiting for `connect_profile()` to finish before polling the profile handle causes a BlueZ profile handshake timeout).
5. Accept the `ConnectRequest` into a `bluer::rfcomm::Stream` and read bytes continuously.

### Serial Button Codes
The parser uses token scanning rather than line parsing. Tokens may be concatenated with no delimiter, optional trailing NUL bytes are ignored, incomplete token suffixes are retained across reads, and unknown/noise bytes before a valid token are skipped.

| Serial Code | Button | Action / State | Physical Button No. |
| --- | --- | --- | ---: |
| `+PTT=P` | PTT | Pressed | 1 |
| `+PTT=R` | PTT | Released | 1 |
| `C:SP*` | SOS | Pressed | 4 |
| `C:SR*` | SOS | Released | 4 |
| `C:SOS*` | SOS | Long-Press | 4 |
| `C:GP*` | Group Switch | Pressed | 6 |
| `C:GR*` | Group Switch | Released | 6 |
| `C:VP*` | Volume Up | Clicked | 2 |
| `C:VM*` | Volume Down | Clicked | 3 |

### Hardware-Enforced State Machine
The RSM natively cycles between two distinct modes of operation. The software must adapt to this lifecycle, as key inputs physically change characters and roles on-chip:

1. **State A (Active Mode - Blinking Green LED):** 
   - Volume Up/Down keys do not send serial data; they physically manage the internal speaker amplifier.
   - Only PTT, Group, and SOS events are transmitted over Bluetooth.
2. **State B (Selection Mode - Solid Green LED):**
   - Entered by pressing the **Group Switch** (`C:GP*`).
   - Volume Up/Down keys transmit serial data (`C:VP*`/`C:VM*`) and no longer affect speaker amplitude.
   - Exited automatically when **PTT** (`+PTT=P`) is pressed, reverting the device physically back to State A.

---

## 3. Core Mental Model: The "Tabbed Menu" Interface

The entire suite of software controls is structured around a unified **Tabbed Menu** abstraction. Every feature or tool modeled in the system contains exactly two phases:

### Phase 1: Active Phase (State A)
The tool is currently running its primary routine.
- **PTT:** Handles the primary action of the active tool (after a hold threshold).
- **SOS (Short & Long):** Fully unburdened and available for user-defined, tool-specific shortcuts or favorite macro actions.
- **Volume Up/Down:** Reserved for hardware-level local audio volume.

### Phase 2: Control Phase (State B)
Entered by pressing the **Group Switch**. This is treated as a multi-page tabbed interface:
1. **Tab Navigation:** Pressing the **Group Switch** cycles through different "pages" or "tabs" (e.g., Tab 1 for local tool parameters, Tab 2 for system-level tool swapping).
2. **Item Scrolling:** Pressing **Volume Up/Down** scrolls through the list of items within the active Tab.
3. **Core Selection:** Pressing **PTT** executes the highlighted option and returns the user to the Active Phase.
4. **Context-Preserving Interactions:** Since the hardware allows SOS presses to fire without exiting State B, **SOS Short** and **SOS Long** are unburdened to trigger alternate actions on the focused item *without* dropping the user out of the navigation menu.

---

## 4. Input Handling Guidelines

To ensure snappy and robust physical interaction, the backend applies the following processing rules:

### I. SOS Press Differentiation
The hardware controls its own long-press threshold. When the SOS button is held down, it immediately sends `C:SP*` (Press), followed by `C:SOS*` (Long Press) after a delay, and finally `C:SR*` (Release).
- **Software Rule:** The short-press action (triggered by release `C:SR*`) must be suppressed if a long-press signal (`C:SOS*`) was emitted during the current press cycle.

### II. Context-Aware PTT Debouncing
The PTT button requires different evaluation parameters based on the current system mode:
- **In Active Mode (State A):** Apply a configurable hold-down threshold (e.g., 350ms) to filter out brief accidental taps of the button when the device is worn on a belt or shoulder.
- **In Selection Mode (State B):** Disable debouncing entirely. PTT inputs must register immediately to ensure navigation select commands feel fast and responsive.

---

## 5. Configuration Interface

Use `--config <path>`, otherwise load `$XDG_CONFIG_HOME/ptt-audio-menu/config.toml` or `~/.config/ptt-audio-menu/config.toml`.

**TOML Concepts:**
- `default_tool` is required.
- IDs are strict lowercase slugs and unique within their namespace.
- Voice config uses explicit Piper model/config paths.
- TTS cache defaults to `$XDG_CACHE_HOME/ptt-audio-menu/tts`, overrideable in config.
- `[audio] device` optionally specifies a bluetooth device MAC address for audio output routing. When set, audio is directed to the matching PipeWire sink via the `PIPEWIRE_NODE` environment variable. If omitted, the system default sink is used.
- Global defaults include active PTT hold threshold; tools may override it.
- Tools define active hooks and local control tabs. Global tabs are available from every tool.
- Items define label text plus primary/alternate actions.

**Action Types:**
- `internal`: `switch_tool`, `speak`, `noop`, `exit_control`, `reload_config`, `stop_audio`, `cancel_running_action`.
- `command`: argv-list only, no shell string. Inherits process cwd/env, with optional per-action `cwd` and extra env.
- Commands have no timeout by default; per-action timeout is optional.
- Per-action feedback is optional: start/success/failure spoken labels.

---

## 6. Runtime Behavior

### Audio & TTS
- At startup, validate config and prerender all configured TTS labels before connecting hardware.
- Cache WAV PCM prompts using a full input hash: text, voice/model paths, Piper settings, output format, and app version.
- On successful startup, speak the active tool label.
- Navigation speech uses interrupt-latest semantics.
- Playback is internal via `kira`.
- Audio output routing derives a PipeWire sink node name from the configured Bluetooth MAC (`bluez_output.<underscored_mac>.1`) and sets the `PIPEWIRE_NODE` environment variable before initializing the audio backend. When no audio device is configured, the system default sink applies.

### Action Execution
- Async actions run one at a time in a serial queue.
- Shell commands spawn in their own process group.
- `cancel_running_action` terminates the running command process group where supported.
- `reload_config` validates and prerenders the new config; on failure, speak/log failure and exit process.

### Logging
- Replace ad hoc `println!` diagnostics with `tracing` stdout logging.
- Keep raw RFCOMM/event logs available at debug level.

---

## 7. Implementation & Architecture

### Module Split
Split the current single-file binary into distinct modules:
- Transport & Parser
- Hardware Event Normalization
- Config Loading & Validation
- Menu State
- TTS Cache
- Audio Playback
- Action Execution

### Dependencies
Add support for config/CLI/logging/TTS/audio/cache: `serde`, `toml`, `clap`, `directories` (or equivalent XDG helper), `tracing`, `tracing-subscriber`, `sha2`, `piper-rs`, `kira`, and `bluer`.

### Current Constraints
- Device MAC is hardcoded.
- RFCOMM channel is not hardcoded.
- Audio output routing to the Bluetooth sink uses the `PIPEWIRE_NODE` environment variable with a node name derived from the hardcoded MAC; the `[audio] device` config field is present but not yet wired as the primary routing source.
- No runtime `sdptool` dependency.
- No shell command action backend (argv-list only).
- No keyboard/media/uinput backend.
- No systemd/NixOS service.
- No automatic device discovery by name.
- No pairing workflow.
- No state persistence.

---

## 8. Test Plan

- **Parser tests:** Remain unchanged and passing.
- **Config validation tests:**
  - Missing `default_tool`
  - Duplicate/invalid slug IDs
  - Unknown tool/action references
  - Invalid Piper paths
  - Command action rejects shell-string-only config
- **Menu state tests:**
  - Active to Control via Group
  - Tab cycling via Group
  - Item scrolling via Volume
  - PTT selection exits Control Phase
  - Control SOS alternate action stays in Control Phase
  - No idle timeout transition
- **Input semantics tests:**
  - Active PTT threshold suppresses short accidental taps
  - Control PTT bypasses threshold
  - SOS short suppressed after long signal
- **TTS cache tests:**
  - Same full input hash reuses WAV
  - Text/model/settings changes produce different cache entries
- **Action executor tests:**
  - Command queue is serial
  - Optional timeout works when configured
  - Cancel terminates process group
  - Reload failure exits instead of partially applying config

---

## 9. Development Commands

The project includes a Nix flake dev shell containing the tools needed to compile the project.

```sh
nix develop
cargo check
cargo test
cargo run
```
