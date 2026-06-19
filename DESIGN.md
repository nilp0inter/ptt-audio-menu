# Project Hand-off Document: Screenless Audio-First UI Engine

## 1. Project Overview
The objective is to design a software engine that turns a hardware **Bluetooth Remote Speaker Microphone (RSM)** into an interactive, screenless, voice-based interface. The user navigates features, executes commands, and switches between various tool states using physical button inputs as controls and audio/Text-to-Speech (TTS) as feedback.

This document synthesizes the hardware constraints of the device and establishes the core software design patterns for future development.

---

## 2. Hardware Profile & Constraints

*   **Device:** B01 Bluetooth Remote Speaker Microphone.
*   **Host Connection:** Standard Bluetooth RFCOMM serial emulation `/dev/rfcomm0`.
*   **Serial Output Reference:**
    *   **PTT:** `+PTT=P` (Press), `+PTT=R` (Release)
    *   **Group Switch:** `C:GP*` (Press), `C:GR*` (Release)
    *   **Volume Up/Down:** `C:VP*` (Volume Up), `C:VM*` (Volume Down)
    *   **SOS Button:** `C:SP*` (Pressed), `C:SR*` (Released), `C:SOS*` (Hardware-controlled Long Press)

### The Hardware-Enforced State Machine
The RSM natively cycles between two distinct modes of operation. The software must adapt to this lifecycle, as key inputs physically change characters and roles on-chip:

```
               [State A: Blinking Green LED]
             (Volume keys managed by hardware)
                            |
                 (Press Group / C:GP*)
                            |
                            v
                [State B: Solid Green LED]
             (Volume keys sent over serial)
                            |
               (Press PTT / Sends +PTT=P)
                            |
                            v
               [State A: Blinking Green LED]
```

1.  **State A (Active Mode - Blinking Green LED):**
    *   **Volume Up/Down keys do not send serial data.** They physically manage the internal speaker amplifier of the RSM.
    *   Only PTT, Group, and SOS events are transmitted over Bluetooth.
2.  **State B (Selection Mode - Solid Green LED):**
    *   Entered by pressing the **Group Switch** (`C:GP*`).
    *   **Volume Up/Down keys transmit serial data** (`C:VP*`/`C:VM*`) and no longer affect speaker amplitude.
    *   Exited automatically when **PTT** (`+PTT=P`) is pressed, reverting the device physically back to State A.

---

## 3. Input Handling Guidelines

To ensure Snappy and robust physical interaction, the backend serial parser must apply the following processing rules:

### I. SOS Press Differentiation
The hardware controls its own long-press threshold. When the SOS button is held down, it immediately sends `C:SP*` (Press), followed by `C:SOS*` (Long Press) after a delay, and finally `C:SR*` (Release). 
*   **Software Rule:** The short-press action (triggered by release `C:SR*`) must be suppressed if a long-press signal (`C:SOS*`) was emitted during the current press cycle. 

### II. Context-Aware PTT Debouncing
The PTT button requires different evaluation parameters based on the current system mode:
*   **In Active Mode (State A):** Apply a configurable hold-down threshold (e.g., 350ms) to filter out brief accidental taps of the button when the device is worn on a belt or shoulder.
*   **In Selection Mode (State B):** Disable debouncing entirely. PTT inputs must register immediately to ensure navigation select commands feel fast and responsive.

---

## 4. Core Mental Model: The "Tabbed Menu" Interface

Rather than separating the configuration into completely different "Menus" and "Apps," the entire suite of software controls is structured around a unified **Tabbed Menu** abstraction.

Every feature or tool modeled in the system contains exactly two phases:

```
+--------------------------------------------------------------------------+
|                               ACTIVE PHASE                               |
|                         (LED Blinking/State A)                           |
|          * Volume controls local speaker hardware volume                 |
|          * PTT executes primary tool action                              |
+--------------------------------------------------------------------------+
                                     |
                          (User presses Group key)
                                     v
+--------------------------------------------------------------------------+
|                              CONTROL PHASE                               |
|                          (LED Solid/State B)                             |
|                                                                          |
|  +-------------------+  (Group Key)  +--------------------------------+  |
|  |       TAB 1       | ------------> |             TAB 2              |  |
|  |  (Local Context)  | <------------ |     (Global / System Context)    |  |
|  +-------------------+               +--------------------------------+  |
|           |                                          |                   |
|     (Volume keys)                               (Volume keys)            |
|           v                                          v                   |
|     Scroll items                               Scroll items              |
|           |                                          |                   |
|     (PTT Select)                                (PTT Select)             |
|           +----------- Executes & Returns to Active <----+                   |
+--------------------------------------------------------------------------+
```

### Phase 1: Active Phase (State A)
The tool is currently running its primary routine. 
*   **PTT:** Handles the primary action of the active tool.
*   **SOS (Short & Long):** Fully unburdened and available for user-defined, tool-specific shortcuts or favorite macro actions.
*   **Volume Up/Down:** Reserved for hardware-level local audio volume.

### Phase 2: Control Phase (State B)
Entered by pressing the **Group Switch**. This is treated as a multi-page tabbed interface:
1.  **Tab Navigation:** Pressing the **Group Switch** cycles through different "pages" or "tabs" (e.g., Tab 1 for local tool parameters, Tab 2 for system-level tool swapping).
2.  **Item Scrolling:** Pressing **Volume Up/Down** scrolls through the list of items within the active Tab.
3.  **Core Selection:** Pressing **PTT** executes the highlighted option and returns the user to the Active Phase.
4.  **Context-Preserving Interactions:** Since the hardware allows SOS presses to fire without exiting State B, **SOS Short** and **SOS Long** are unburdened to trigger actions on the focused item *without* dropping the user out of the navigation menu.
