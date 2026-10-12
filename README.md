<div align="center">

<img src="assets/icon.png" width="96" alt="Dual Audio">

# Dual Audio

**Play your PC's audio through several headphones, speakers or earbuds — at the same time.**

Windows only outputs to one device at a time. Dual Audio mirrors your system audio to as many
devices as you want, live — no virtual cables, no drivers, no reboots.

[![Download](https://img.shields.io/github/v/release/jere344/windows-dual-audio?label=download&style=for-the-badge)](https://github.com/jere344/windows-dual-audio/releases/latest)
[![License](https://img.shields.io/github/license/jere344/windows-dual-audio?style=for-the-badge)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows-0078D6?style=for-the-badge)](#)
[![Python](https://img.shields.io/badge/python-3.10-3776AB?style=for-the-badge)](https://www.python.org/)

<img src="assets/screenshot.png" width="720" alt="Dual Audio screenshot">

</div>

## Features

- **Multi-device output** — the same audio on 2, 3, 4+ devices at once
- **No drivers** — pure Windows Core Audio (WASAPI); nothing to install
- **Per-device volume** and **sync delay** (0–2000 ms) to line up Bluetooth latency
- **Live level meter** so you can see what is being captured
- **Choose the capture source** — your default output or any other device
- **Test tone** per device to verify it before sharing
- **System tray** — close the window and audio keeps playing
- **Portable** — a single `.exe` (~25 MB), no installer

## Download

Grab the latest `DualAudio.exe` from the [**Releases**](https://github.com/jere344/windows-dual-audio/releases/latest)
page — a single portable file, no installation required.

> Requires Windows 10/11 (64-bit).

## Quick start

1. Connect / pair all your headphones.
2. Set the one that should play "natively" as your **Windows default output**.
3. Run `DualAudio.exe` and tick the **other** devices you want to hear the audio on.
4. Press **Start sharing**.

## How it works

```
Windows default output ──(WASAPI loopback)──▶ fan-out ─┬─▶ Device A  (volume, delay)
                                                         ├─▶ Device B  (volume, delay)
                                                         └─▶ Device N
```

Your default device already plays the sound, so it is excluded from the target list (this avoids an
echo). Every enabled device gets its own independent render stream, so they all play simultaneously.

## Bluetooth out of sync?

Different headsets have different codec latency (often 150–250 ms). Raise the **Sync delay** on the
device that is *ahead* until both line up.

## Troubleshooting

| Problem | Fix |
|---|---|
| Level meter stays at `-inf dB` | Nothing is being captured — play some audio, or pick another **source** |
| No sound on a mirrored device | Press **Test**; if it stays silent, check that device's Windows volume/mute |
| Netflix / Disney+ is silent | DRM blocks capture — use a browser (Chrome/Firefox) instead |

## Build from source

Requires **Python 3.10** (PyInstaller 6.x supports up to 3.12).

```bat
build.bat
```

Produces `dist\DualAudio.exe`. See [CONTRIBUTING.md](CONTRIBUTING.md) for details.

## Star history

<a href="https://star-history.com/#jere344/windows-dual-audio&Date">
  <img src="https://api.star-history.com/svg?repos=jere344/windows-dual-audio&type=Date" width="600" alt="Star history">
</a>

## License

[Apache-2.0](LICENSE)
