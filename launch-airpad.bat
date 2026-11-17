@echo off
setlocal EnableExtensions EnableDelayedExpansion
cd /d "%~dp0"

set "EXE_LOCAL=%~dp0airpad.exe"
set "EXE_RELEASE=%~dp0target\release\airpad.exe"
set "EXE_PATH="
set "MARKER_DIR=%LOCALAPPDATA%\AirPad"

if exist "%EXE_LOCAL%" (
    echo [OK] Found airpad.exe - skipping build/download.
    set "EXE_PATH=%EXE_LOCAL%"
    goto :check_firewall
)

if exist "%EXE_RELEASE%" (
    echo [OK] Found compiled release binary - skipping build.
    set "EXE_PATH=%EXE_RELEASE%"
    goto :check_firewall
)

echo [Setup] airpad.exe not found yet. Checking environment...

:: Check Cargo on PATH or default rustup location
where cargo >nul 2>&1
if errorlevel 1 (
    if exist "%USERPROFILE%\.cargo\bin\cargo.exe" (
        set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
    )
)

where cargo >nul 2>&1
if not errorlevel 1 goto :build_with_cargo

:: Cargo not found: try downloading prebuilt release binary first (fastest, no compiler needed)
echo [Setup] Rust/Cargo not detected. Downloading prebuilt Windows release...
powershell -NoProfile -ExecutionPolicy Bypass -Command "try { $ProgressPreference='SilentlyContinue'; $zip=Join-Path $env:TEMP 'airpad-windows-x86_64.zip'; Invoke-WebRequest -Uri 'https://github.com/kelvin0511/airpad/releases/latest/download/airpad-windows-x86_64.zip' -OutFile $zip -UseBasicParsing; Expand-Archive -Path $zip -DestinationPath '%~dp0' -Force; Remove-Item $zip -Force } catch { exit 1 }"
if exist "%EXE_LOCAL%" (
    echo [OK] Downloaded airpad.exe.
    set "EXE_PATH=%EXE_LOCAL%"
    goto :check_firewall
)

if not exist "%~dp0Cargo.toml" (
    echo.
    echo [Error] Could not download airpad.exe and no Cargo.toml found here.
    echo Please check your internet connection or download airpad-windows-x86_64.zip from GitHub Releases.
    pause
    exit /b 1
)

:: Offer automatic Rust installation if building from source
echo [Setup] Prebuilt release unavailable. Installing Rust toolchain via rustup...
where winget >nul 2>&1
if not errorlevel 1 (
    winget install -e --id Rustlang.Rustup --accept-package-agreements --accept-source-agreements
) else (
    powershell -NoProfile -ExecutionPolicy Bypass -Command "try { $exe=Join-Path $env:TEMP 'rustup-init.exe'; Invoke-WebRequest -Uri 'https://win.rustup.rs/x86_64' -OutFile $exe -UseBasicParsing; Start-Process -FilePath $exe -ArgumentList '-y' -Wait; Remove-Item $exe -Force } catch { exit 1 }"
)

if exist "%USERPROFILE%\.cargo\bin\cargo.exe" (
    set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
)

where cargo >nul 2>&1
if errorlevel 1 (
    echo.
    echo [Error] Rust installation did not complete. Please install Rust from https://rustup.rs/ and re-run this script.
    pause
    exit /b 1
)

:build_with_cargo
if not exist "%~dp0Cargo.toml" (
    echo [Error] Cargo.toml not found in %~dp0.
    pause
    exit /b 1
)

echo [Setup] Building airpad.exe with Cargo (release mode)...
cargo build --release
if errorlevel 1 (
    echo.
    echo [Error] Cargo build failed. See compiler output above.
    echo If linker/MSVC tools are missing, install "Desktop development with C++" from Visual Studio Build Tools,
    echo or download the prebuilt airpad-windows-x86_64.zip from GitHub Releases.
    pause
    exit /b 1
)

if exist "%EXE_RELEASE%" (
    copy /Y "%EXE_RELEASE%" "%EXE_LOCAL%" >nul 2>&1
    if exist "%EXE_LOCAL%" (
        set "EXE_PATH=%EXE_LOCAL%"
    ) else (
        set "EXE_PATH=%EXE_RELEASE%"
    )
    goto :check_firewall
)

echo [Error] Build finished but airpad.exe was not found.
pause
exit /b 1

:check_firewall
netsh advfirewall firewall show rule name="AirPad HTTPS 8443" >nul 2>&1
if errorlevel 1 goto :setup_firewall
netsh advfirewall firewall show rule name="AirPad Setup 8442" >nul 2>&1
if errorlevel 1 goto :setup_firewall

echo [OK] Windows Firewall rules already configured - skipping.
goto :launch

:setup_firewall
if exist "%MARKER_DIR%\.firewall_skipped" (
    echo [Skip] Firewall setup was previously skipped ^(you can enable it inside AirPad anytime^).
    goto :launch
)

echo [Setup] Adding Windows Firewall rules for ports 8442 and 8443 (one-time)...
powershell -NoProfile -ExecutionPolicy Bypass -Command "try { $cmd = 'netsh advfirewall firewall delete rule name=\"AirPad HTTPS 8443\" >nul 2>&1 & netsh advfirewall firewall delete rule name=\"AirPad Setup 8442\" >nul 2>&1 & netsh advfirewall firewall add rule name=\"AirPad HTTPS 8443\" dir=in action=allow protocol=TCP localport=8443 profile=any & netsh advfirewall firewall add rule name=\"AirPad Setup 8442\" dir=in action=allow protocol=TCP localport=8442 profile=any'; $p = Start-Process cmd -ArgumentList '/c', $cmd -Verb RunAs -Wait -WindowStyle Hidden -PassThru; exit $p.ExitCode } catch { exit 1 }"
if errorlevel 1 (
    if not exist "%MARKER_DIR%" mkdir "%MARKER_DIR%" >nul 2>&1
    type nul > "%MARKER_DIR%\.firewall_skipped"
    echo [Warn] Firewall setup was skipped/canceled. You can enable it later with one click inside AirPad.
) else (
    if exist "%MARKER_DIR%\.firewall_skipped" del "%MARKER_DIR%\.firewall_skipped" >nul 2>&1
    echo [OK] Windows Firewall configured.
)

:launch
echo Launching AirPad...
start "" "%EXE_PATH%" %*
exit /b 0
