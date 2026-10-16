# Changelog

All notable changes to this project are documented here.

## [2.0.0] - 2026-10-16

Rewritten in Rust.

- **Linux support**: PulseAudio and PipeWire (via `pipewire-pulse`), using the native PulseAudio protocol
  with no system library dependencies. Tray icon via StatusNotifierItem.
- Sync delay and volume now apply instantly, with no stream restart.
- Clock drift between devices is corrected automatically (buffer trimming / re-priming).
- The capture source is shown in the device list and can't be selected as a target (avoids echo).
- The selected capture source is remembered.
- New command-line flags: `--tray`, `--list-devices`, `--version`.
- Much smaller, faster-starting native executable; no Python runtime.

## [1.0.0] - 2026-10-13

Initial release (Python, Windows only).

- Share system audio (WASAPI loopback) to multiple output devices simultaneously.
- Per-device volume (0–150%) and sync delay (0–2000 ms).
- Live input level meter and capture-source selector.
- Per-device test tone.
- System tray with Show / Start / Stop / Quit.
- Settings persisted to `%AppData%\DualAudio\settings.json`.
- Portable single-file Windows executable.
