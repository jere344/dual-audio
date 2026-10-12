# -*- mode: python ; coding: utf-8 -*-
import os

from PyInstaller.utils.hooks import collect_all

datas = []
binaries = []
hiddenimports = []
for _pkg in ("customtkinter", "soundcard", "sounddevice"):
    _d, _b, _h = collect_all(_pkg)
    datas += _d
    binaries += _b
    hiddenimports += _h
hiddenimports += [
    "soundcard.mediafoundation",
    "numpy.core._multiarray_tests",
    "secrets",
    "random",
    "hmac",
    "hashlib",
    "base64",
    "bisect",
    "numbers",
    "logging",
    "logging.handlers",
    "weakref",
    "decimal",
    "contextlib",
]

excludes = [
    "matplotlib",
    "pandas",
    "scipy",
    "IPython",
    "PyQt5",
    "PyQt6",
    "PySide2",
    "PySide6",
    "wx",
    "notebook",
    "setuptools",
    "pkg_resources",
    "altgraph",
    "pip",
    "wheel",
    "pytest",
    "tkinter.test",
    "turtledemo",
    "lib2to3",
    "distutils",
    "test",
    "win32",
    "win32api",
    "win32con",
    "win32gui",
    "win32ui",
    "win32com",
    "win32comext",
    "pythoncom",
    "pywintypes",
    "Pythonwin",
    "pywin",
    "adodbapi",
    "isapi",
    "numpy.testing",
    "numpy.f2py",
    "numpy.distutils",
    "PIL._webp",
    "PIL._imagingcms",
    "PIL.ImageQt",
]

a = Analysis(
    ["dual_audio.py"],
    pathex=[],
    binaries=binaries,
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=excludes,
    noarchive=False,
)

# Remove duplicate binaries that share the same source file (e.g. numpy's
# libopenblas is picked up both at the archive root and under numpy/.libs).
_seen = {}
_order = []
for _entry in a.binaries:
    _dest, _src, _typ = _entry
    _key = os.path.normcase(os.path.abspath(_src)) if _src else _dest
    if _key in _seen:
        if os.sep in _dest and os.sep not in _seen[_key][0]:
            _seen[_key] = _entry
        continue
    _seen[_key] = _entry
    _order.append(_key)
a.binaries = [_seen[_k] for _k in _order]


# Keep only the PortAudio build this app actually loads.
def _keep_binary(dest):
    if "_sounddevice_data" not in dest.replace("\\", "/"):
        return True
    return os.path.basename(dest).lower() == "libportaudio64bit.dll"


a.binaries = [b for b in a.binaries if _keep_binary(b[0])]
a.datas = [d for d in a.datas if _keep_binary(d[0])]

pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    [],
    name="DualAudio",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=False,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
    icon="assets/icon.ico",
    version="version_info.txt",
)
