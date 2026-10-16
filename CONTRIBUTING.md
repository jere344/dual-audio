# Contributing

Thanks for taking the time to contribute to Dual Audio.

## Getting started

1. Install [Rust](https://rustup.rs) (1.85+).
2. Run the app from source:

   ```bash
   cargo run
   ```

No system development packages are required on either platform.

## Project layout

```
src/main.rs           Entry point, window/tray lifecycle, CLI flags
src/app.rs            Application state shared by the window and the tray
src/engine.rs         Capture → per-device jitter buffer → output fan-out
src/audio/mod.rs      Backend-neutral types
src/audio/pulse.rs    Linux backend (native PulseAudio protocol; works with PipeWire)
src/audio/wasapi.rs   Windows backend (WASAPI loopback via cpal)
src/tray/             Tray icon: StatusNotifierItem (Linux), notification area (Windows)
src/ui.rs             egui main window
src/settings.rs       JSON settings
src/icon.rs           Icon rasterised at runtime
build.rs              Embeds assets/icon.ico in the Windows executable
```

## Building a release

```bash
cargo build --release
```

Release binaries are built by [`.github/workflows/release.yml`](.github/workflows/release.yml) for
Windows and Linux when a `v*` tag is pushed.

## Testing

```bash
cargo test
```

`engine::tests::mirrors_audio_between_devices` is an end-to-end test against a live sound server and
is ignored by default. On Linux with PipeWire you can run it with two scratch sinks:

```bash
pw-cli -m create-node adapter '{ factory.name=support.null-audio-sink node.name=dualtest_src media.class=Audio/Sink object.linger=false audio.position=[FL FR] }' &
pw-cli -m create-node adapter '{ factory.name=support.null-audio-sink node.name=dualtest_dst media.class=Audio/Sink object.linger=false audio.position=[FL FR] }' &
DUAL_TEST_SRC=dualtest_src DUAL_TEST_DST=dualtest_dst cargo test -- --ignored
```

Stop the two `pw-cli` processes afterwards to remove the sinks.

## Guidelines

- Keep audio callbacks short: no allocation-heavy work and no blocking beyond the per-output lock.
- Run `cargo fmt` and `cargo clippy` before opening a pull request.
- Test with at least two output devices.

## Reporting bugs

Please use the [issue templates](.github/ISSUE_TEMPLATE) and include your OS (and on Linux, whether
you use PipeWire or PulseAudio), the devices involved, and any relevant lines from `error.log` in the
settings folder (`%AppData%\DualAudio` or `~/.config/DualAudio`).
