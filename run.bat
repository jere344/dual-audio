@echo off
setlocal
cd /d "%~dp0"
py -3.10 -c "import sounddevice, soundcard, customtkinter, pystray, PIL, numpy" 2>nul
if errorlevel 1 (
    echo Installing dependencies...
    py -3.10 -m pip install -r requirements.txt
)
py -3.10 dual_audio.py
