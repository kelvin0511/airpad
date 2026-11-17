@echo off
setlocal EnableExtensions
cd /d "%~dp0"

set "EXE_LOCAL=%~dp0airpad.exe"
set "EXE_RELEASE=%~dp0target\release\airpad.exe"

if exist "%EXE_LOCAL%" (
    echo Launching AirPad...
    start "" "%EXE_LOCAL%" %*
    exit /b 0
)

if exist "%EXE_RELEASE%" (
    echo Launching AirPad...
    start "" "%EXE_RELEASE%" %*
    exit /b 0
)

where cargo >nul 2>&1
if %ERRORLEVEL% EQU 0 (
    echo airpad.exe not found yet. Building release binary with Cargo...
    cargo build --release
    if %ERRORLEVEL% NEQ 0 (
        echo.
        echo Build failed. Make sure Visual Studio C++ Build Tools / Windows SDK are installed.
        pause
        exit /b 1
    )
    if exist "%EXE_RELEASE%" (
        echo Launching AirPad...
        start "" "%EXE_RELEASE%" %*
        exit /b 0
    )
)

echo.
echo Could not find airpad.exe or Cargo.
echo - Download airpad-windows-x86_64.zip from GitHub Releases and place airpad.exe next to this .bat file, OR
echo - Install Rust from https://rustup.rs/ and run this launcher again to build from source.
echo.
pause
exit /b 1
