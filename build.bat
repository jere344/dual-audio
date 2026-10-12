@echo off
setlocal
cd /d "%~dp0"
echo Installing dependencies...
py -3.10 -m pip install -r requirements.txt
echo Generating icon...
py -3.10 make_icon.py
echo Building executable...
py -3.10 -m PyInstaller --noconfirm --clean DualAudio.spec
echo.
echo Done. Your executable is: dist\DualAudio.exe
