# Project Status

## Goal

Create a Rust software controller for a physical Bluetooth PTT microphone.

The microphone sends serial button codes over Bluetooth RFCOMM. The program should read those button events on the computer so they can later be used to trigger actions.

## Device

- Device name: `B02PTT-FF01`
- Device MAC: `00:02:5B:55:FF:01`
- Service used: Bluetooth Serial Port Profile, UUID `00001101-0000-1000-8000-00805f9b34fb`
- Observed SDP service name: `GAIA`
- RFCOMM channel is not assumed stable and is not hardcoded.

## Current Approach

The program uses BlueR and BlueZ profile APIs instead of opening a hardcoded RFCOMM channel.

Current connection flow:

1. Create a BlueR `Session`.
2. Get the default Bluetooth adapter.
3. Power the adapter on.
4. Get the hardcoded target device by MAC.
5. Register a local RFCOMM client profile for the Serial Port UUID.
6. Start `Device::connect_profile()` and concurrently wait for `ProfileHandle::next()`.
7. Accept the received `ConnectRequest` into a `bluer::rfcomm::Stream`.
8. Read bytes from the stream continuously.
9. Log raw RFCOMM chunks as hex and lossy ASCII.
10. Parse known button tokens from the byte stream and log structured events.

The concurrent `connect_profile()` / `ProfileHandle::next()` handling is required. Waiting for `connect_profile()` to finish before polling the profile handle causes a BlueZ profile handshake timeout.

## Parsed Button Codes

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

Parser behavior:

- Token scanning is used, not line parsing.
- Tokens may be concatenated with no delimiter.
- Optional trailing NUL bytes after tokens are ignored.
- Incomplete token suffixes are retained across reads.
- Unknown/noise bytes before a valid token are skipped.

## Current Status

- Rust crate exists.
- Nix flake dev shell exists and contains the tools needed to compile the project.
- Parser unit tests exist and pass.
- `cargo check` passes inside the flake dev shell.
- Runtime connection works after disabling the Serial-port autoconnect option in Bluetooth settings.
- Program currently logs raw input and decoded button events.

## Current Constraints

- Device MAC is hardcoded.
- RFCOMM channel is not hardcoded.
- No runtime `sdptool` dependency.
- No shell command action backend.
- No keyboard/media/uinput backend.
- No systemd/NixOS service.
- No automatic device discovery by name.
- No pairing workflow.

## Development Commands

```sh
nix develop
cargo check
cargo test
cargo run
```
