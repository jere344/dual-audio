# Contributing

Thanks for taking the time to contribute to Dual Audio.

## Getting started

1. Install **Python 3.10** (PyInstaller 6.x supports up to 3.12).
2. Install dependencies:

   ```bat
   py -3.10 -m pip install -r requirements.txt
   ```

3. Run the app from source:

   ```bat
   run.bat
   ```

## Project layout

```
dual_audio.py     Audio engine (WASAPI loopback capture + multi-output fan-out), UI and tray
DualAudio.spec    PyInstaller build configuration
version_info.txt  Windows executable metadata
make_icon.py      Generates assets/icon.ico and assets/icon.png
```

## Building the executable

```bat
build.bat
```

The output is `dist\DualAudio.exe` (single file). The build is driven by `DualAudio.spec`, which trims
unused modules and bundles only the PortAudio build the app loads.

## Verifying a build

```bat
dist\DualAudio.exe --selftest
```

This loads every bundled dependency and writes `%AppData%\DualAudio\selftest.ok` (or `selftest.err` on
failure).

## Guidelines

- Keep the audio engine on the main thread free; capture and playback run in worker threads.
- Match the existing code style (no unnecessary comments, small focused functions).
- Test with at least two output devices before opening a pull request.

## Reporting bugs

Please use the [issue templates](.github/ISSUE_TEMPLATE) and include your Windows version, the
devices involved, and any relevant lines from `%AppData%\DualAudio\error.log`.
