# MIDI Sequencer — Design Document

**Version:** 0.25
**Stage:** Build, opened by the owner on 2026-11-18. Scope: Milestone 1.
Any return to discussion pauses building until the owner reopens the build stage.

## 1. Big picture

A home-built MIDI step sequencer running on a Raspberry Pi.

### 1.1 Target setup (later)
- Pi acts as USB host for any number of class-compliant USB-MIDI synths and controllers.
- Possible later outputs: serial (DIN) MIDI, and CV/Gate through a DAC (assumed "DAC"; to confirm).

### 1.2 Build-stage setup (now)
- No hardware synths yet. All instruments are VST plugins hosted on a laptop.
- MIDI over the network is therefore required from day one.
- Control surface: Behringer BCR32 (SKU 0715-AAW). It is not available yet.
  - 32 rotary encoders (one per step), 4 track-select buttons, controller/sequencer mode toggle, parameter display.
  - USB-C (data + power), separate USB-C power, MIDI In, MIDI Out A, MIDI Out B/Thru (DIN).
  - Analogue sync in/out, 4x CV out (1 V/oct, 0 to +10 V), 4x gate out (5 V or 10 V), 2 footswitch inputs.
  - Controller mode sends CC/NRPN; Learn mode available. Built-in 4-track step sequencer in the other mode.
  - Controls (Behringer open-source repo + community docs, manual V2.0 2026-04-06, firmware 1.0.3):
    - 32 push encoders E25–E56, 4 rows x 8, each with a 15-LED ring (12 ring styles). Plus a SEL encoder for menus.
    - B1 mode; B5–B20: 16 parameter buttons in 2 rows of 8 (ZAQ labels: PITCH, VELOCITY, MUTE, DELAY, LENGTH,
      CTRL α, CTRL β, WAVE GEN, CHORD, STEP SKIP, SEQ START, SEQ LENGTH, SEQ OFFSET, CLK DIV, SHUFFLE, DIRECTION).
    - B21–B24: track select T1–T4. B59–B68: global, exit, store, load, page -/+, rec, mixer, reset, start/stop.
    - Keys are partly RGB, partly single colour. OLED 128 x 32 mono.
  - Controller (CTRL) mode: assignable are B5–B24, all 32 encoders (turn and push), B65–B68, both footswitches.
    Encoder modes: absolute, 14-bit, relative (3 variants), inc/dec. Button modes: toggle on/off, increment.
    Messages: CC, NRPN, PC, note (buttons), pitch bend (encoders), aftertouch, MMC. 64 presets.
  - USB-MIDI ports: "BCR-32 CTRL" and "BCR-32 ZAQ". Class compliant.
  - Unknown: whether LED rings and button LEDs follow incoming MIDI (feedback from the Pi).
  - Firmware: Behringer published schematic, MCU docs and demo firmware (AGPL-3.0, GD32F405, flashable over USB).
    Custom firmware is a later fallback if CTRL mode falls short.
  - Sources: github.com/MusicTribeOpenSource/0715-AAW, github.com/UniversalSyntropy/bcr32.
- Stand-in: **PCR32**, a native Android app that roughly emulates the BCR32 (see Decisions).

## 2. Open questions
- Confirm "DCA" means DAC for CV output.

## 3. Intent beyond the BCR32 sequencer
The feature set is open-ended on purpose. Finding the workflow is a long creative process.
Known directions:
- Organizing pre-sequenced material.
- Live manipulation during playback.
- Refining sequences: scales and modes, rhythmic additions, generative manipulation.

Consequence: the core must make new ideas cheap to try. Extensibility is a first-class requirement.

## 4. Decisions
- **Order of work:** Rig first: solid, working, ready for the future. Artistic and workflow decisions come later.
  Deferred until then: whether live manipulation is non-destructive or can commit into the pattern.
- **Hardware path:** Prototype on the existing Pi Zero W (ARMv6, 32-bit, Wi-Fi only, 1 USB data port).
  Move to a stronger board (Pi 4/5) later. Constraints that follow:
  - Toolchain and dependencies must support ARMv6 / 32-bit, and also build for 64-bit ARM.
  - Timing figures measured on the Zero over Wi-Fi do not count as final; timing is re-validated on the target board.
  - A spare Zero W is used; no other project is affected.
  - Board prices are high; the upgrade waits until the prototype proves what the target needs.
- **Role split:** The BCR32 runs in controller mode only. Its built-in sequencer is not used.
  The Pi holds all sequencing logic: everything the BCR32 sequencer does, built new, plus much more.

- **Roles:** Owner directs; Claude writes the code.
- **Engine language:** Rust (confirmed 2026-11-18).
  - Targets: `arm-unknown-linux-gnueabihf` (Zero W, ARMv6) now, `aarch64-unknown-linux-gnu` (Pi 4/5) later. Same source.
  - Built on the workstation by cross-compiling (`cargo zigbuild`, target glibc 2.41, against the Pi's
    libasound with `_TIME_BITS=64`); the Zero is too slow to compile on. Engine runs on the Zero W
    since 2026-11-18; worst lateness 1.7 ms idle.
  - Threaded from day one: clock on its own thread. On the Zero all threads share one core.
    On the Pi 5 the clock thread gets pinned to a reserved core. That is configuration, not new code.

- **Network MIDI:** RTP-MIDI (confirmed). Laptop: rtpMIDI driver. Pi: `rtpmidid` daemon.
  The engine only talks to local ALSA MIDI ports; USB, network and later DIN look the same to it.
  `rtpmidid` v26.01 verified on the Zero W (2026-11-18): built from source on the Pi (67 min), needs
  `pi/rtpmidid-v26.01-32bit-formatter.patch`. Config `pi/rtpmidid.ini`: listener "seqpi" for PCR32,
  laptop connected by IP as ALSA port "Laptop".
- **Laptop VST host:** Cantabile Lite (decided). Fallback: Carla. Free-edition limits still to check.

- **PCR32 (phone controller):** Native Android app, full screen, speaks RTP-MIDI to the Pi over the LAN.
  - Sends what the BCR32 sends in controller mode; the engine cannot tell them apart.
  - Bidirectional: the Pi sends values back for encoder rings and display.
  - Own small RTP-MIDI client (Android has no built-in network MIDI). Kotlin by default.
  - No web server, DNS, certificates or Tailscale. Updates by installing a new APK.
  - Built on the workstation; only the owner can test it on the phone.
- **Laptop:** any laptop on the same LAN as the Pi.
- **OSC:** Not for PCR32; PCR32 speaks MIDI only, like the BCR32. Engine control input is built so an
  OSC input can be added later (richer views: grid, names, text) without touching the core.

- **Clock:** The Pi is the only clock master for now and sends MIDI clock to all outputs.
  The engine reads time from a swappable clock source, so following an external clock
  (MIDI clock or analogue sync) can be added later without touching the sequencing code.

- **Power-cut safety:** Power can be lost at any moment (live on stage). The OS runs from a
  read-only root filesystem. Patterns and settings live on a separate data partition; every save
  writes a new file and atomically renames it over the old one. A cut leaves old or new, never half.
- **Autosave and resume:** A few seconds after the last edit, the engine saves its full state.
  After a power cut it boots back to exactly that state, transport stopped, ready to play.
  At most the last few seconds of tweaks are lost.
- **Deploy:** One command on the workstation cross-compiles, copies the engine to the Pi over SSH
  (same LAN), keeps the previous version beside it, and restarts the engine. One command rolls back.
  The engine runs as a systemd service: starts at boot, restarts on crash.
- **Development on Windows:** The engine also runs on the Windows workstation for development and tests.
  - Sequencing core is plain Rust with no OS calls; automated tests run here, timing against a simulated clock.
  - MIDI through `midir` (ALSA on Linux, WinMM on Windows). Windows needs loopMIDI for virtual ports.
  - Linux-only parts (real-time priority, core pinning, systemd) sit behind a build switch.
  - The Pi confirms real timing, USB and network MIDI.
  - If Windows support starts to cost real effort, it is dropped.

- **Version control:** One git repository for everything (engine, PCR32, profiles, deploy scripts, this doc).
  Local now; a private GitHub remote is added later when the owner provides it.

## 5. Milestone 1: rig proven end to end (agreed 2026-11-18)
- 4 tracks x 32 steps (the BCR32 layout), looping at a set tempo.
- Turning a PCR32 encoder changes that step's note while it plays.
- Notes reach Cantabile on the laptop via RTP-MIDI and sound from a VST.
- Pull the Pi's plug, power up: the pattern is back.
- Out of scope: scales, rhythm additions, generative features, workflow.

## 6. Device mapping layer (agreed 2026-11-18)
Hardware details (BCR32 included) are unknown, so the engine never sees raw device MIDI.
- **Input:** a per-device profile file translates raw MIDI into engine actions, e.g.
  "encoder 5 turned +2", "track button 3 pressed". It covers absolute vs relative encoder modes.
- **Feedback:** the same profile maps engine values back to the device (rings, display).
- **Output:** the engine plays to named instruments; a routing file maps each to a port and channel.
- Profiles are data files, not code. PCR32's profile is written now; the BCR32's when it arrives.
- **PCR32 MIDI map** (`profiles/pcr32.json`; a BCR32 CTRL preset will be programmed to match):
  all CC on channel 16. Encoder turn CC 1–32 (relative, two's complement), encoder push CC 33–64,
  parameter buttons B5–B20 CC 65–80, track buttons CC 81–84, rec/mixer/reset/start-stop CC 85–88,
  footswitches CC 89–90. Press = 127, release = 0. Feedback returns on the same CC:
  ring = absolute value, lights = 127/0.
  App only: tempo CC 91 (relative, whole BPM); the engine reports tempo as one pitch-bend message
  on channel 16 (14-bit whole BPM). In the app, drag on the display = tempo, long press = host.
- **Milestone 1 surface behaviour:** track buttons select the track; PITCH (B5) is the only active
  parameter; encoder turn sets step note; encoder push toggles step on/off; start/stop and
  footswitch 1 toggle transport. Other controls are shown but inactive.

## 7. Notes: small jobs for the next build
Small fixes the owner has approved. They are collected here and done together in the next build,
then removed from this list.
- rtpmidid: drop an accepted session that has been silent for 30 s (PCR32 pings every 10 s).
  A killed app never sends BY, so dead "PCR32" ports pile up. Second patch beside
  `pi/rtpmidid-v26.01-32bit-formatter.patch`.

## Changelog
- 0.25 (2026-11-18): Notes list for small approved jobs; first entry: rtpmidid silent-session timeout.
- 0.24 (2026-11-18): Engine and rtpmidid running on the Zero W; cross-build recorded. PCR32 tempo
  control (app only) added to the MIDI map.
- 0.23 (2026-11-18): PCR32 MIDI map and Milestone 1 surface behaviour recorded.
- 0.22 (2026-11-18): BCR32 control set researched and recorded (corrects earlier "4 buttons").
- 0.21 (2026-11-18): Build stage opened by the owner.
- 0.20 (2026-11-18): Version control: single local repo, GitHub remote later.
- 0.19 (2026-11-18): Milestone 1 defined.
- 0.18 (2026-11-18): Engine also runs on Windows for development and tests.
- 0.17 (2026-11-18): Deploy and rollback over SSH; engine as systemd service.
- 0.16 (2026-11-18): Autosave and resume-after-power-cut decided.
- 0.15 (2026-11-18): Power-cut safety: read-only root, data partition, atomic saves.
- 0.14 (2026-11-18): Pi is clock master; clock source kept swappable for later external sync.
- 0.13 (2026-11-18): Device mapping layer agreed.
- 0.12 (2026-11-18): OSC deferred to a later second control channel; device mapping layer proposed.
- 0.11 (2026-11-18): PCR32 decided as native Android RTP-MIDI app; RTP-MIDI confirmed; laptop is any LAN laptop.
- 0.10 (2026-11-18): Network MIDI proposal (RTP-MIDI) and VST host (Cantabile Lite) recorded.
- 0.9 (2026-11-18): Rust confirmed.
- 0.8 (2026-11-18): Roles set; Rust proposed with dual-target build and thread layout.
- 0.7 (2026-11-18): Spare Zero W confirmed; upgrade deferred on cost.
- 0.6 (2026-11-17): Prototype on Pi Zero W, migrate later; portability constraints recorded.
- 0.5 (2026-11-17): Rig before workflow; workflow questions deferred.
- 0.4 (2026-11-17): Recorded open-ended intent and its directions; extensibility made a core requirement.
- 0.3 (2026-11-17): Role split decided: BCR32 is controller surface only; Pi does all sequencing.
- 0.2 (2026-11-17): Surface confirmed as BCR32; specs from the product page added.
- 0.1 (2026-11-17): Initial big picture: Pi host, USB-MIDI, network MIDI from day one, BCR surface, phone emulator.
