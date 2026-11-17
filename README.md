# AirPad — iPhone Wireless Trackpad, Keyboard & Air-Mouse for Windows 10/11

AirPad turns your iPhone into a low-latency Wi-Fi wireless trackpad, remote keyboard, and mid-air gyroscope pointer for your Windows 10/11 PC—with **zero App Store download or Xcode required**.

## Features

- **Native Windows Server (`airpad.exe`)**: Lightweight Rust (`axum` + `tokio` + Win32 `SendInput` + `egui` + `tray-icon`) single executable.
- **Precision Glass Trackpad** in iPhone Safari (PWA):
  - 1-finger move with gentle pointer acceleration and sub-pixel remainder accumulation on the PC
  - 1-finger tap-to-click, double-tap-and-hold drag-and-drop, and dedicated Left / Middle / Right buttons
  - 2-finger vertical & horizontal scrolling (Natural or Classic scroll toggle)
  - 2-finger right-click, 3-finger middle-click, and 2-finger pinch-to-zoom (`Ctrl + Wheel`)
- **Mid-Air Gyroscope ("Air-Mouse")**:
  - Uses iOS Safari `DeviceMotionEvent.rotationRate` to steer the cursor by tilting/rotating your phone in mid-air
  - Ergonomic **Hold to Pause / Re-center** thumb clutch so you can reposition your wrist cleanly
- **Remote Keyboard**:
  - Full iPhone keyboard Unicode text input (`KEYEVENTF_UNICODE`, supports emojis & international text)
  - Sticky modifier keys (`Ctrl`, `Alt`, `Win`, `Shift`) and quick keys (`Esc`, `Tab`, `Del`, `Backspace`, `Enter`, arrow keys)
- **Local HTTPS + 1-Tap iPhone Certificate Setup**:
  - Auto-generates a persistent local Root CA + LAN IP TLS certificate (`rcgen`)
  - Includes an HTTP onboarding page (`:8442`) with a 1-tap `.mobileconfig` profile so iOS Safari trusts `wss://` and enables Gyroscope permissions cleanly
  - QR-embedded pairing token + single-active-controller takeover + Pause/Disconnect from the Windows tray

## Running on Windows 10/11

### Option 1: Prebuilt Release
Download `airmouse-windows-x86_64.zip` from **GitHub Releases**, extract it, and double-click `launch-airmouse.bat` (or `airpad.exe`).

### Option 2: Build from Source
1. Install [Rust](https://rustup.rs/) (MSVC toolchain).
2. Double-click `launch-airmouse.bat` (it will auto-compile if the binary isn't built yet), or build manually:
   ```powershell
   cargo build --release
   .\target\release\airpad.exe
   ```
   Or run headless in a terminal (prints ASCII QR codes):
   ```powershell
   .\launch-airmouse.bat --headless
   ```

## First-Time iPhone Setup

1. Connect your iPhone and Windows PC to the **same Wi-Fi network**.
2. On Windows, allow `airpad.exe` through **Windows Defender Firewall** (Private networks) when prompted, for TCP ports `8442` and `8443`.
3. In the AirPad window on Windows, select **First-Time iPhone Cert Setup** and scan the QR code with your iPhone Camera:
   - Tap **Install AirPad Certificate Profile** -> **Allow**.
   - On iPhone: **Settings → General → VPN & Device Management → AirPad Local HTTPS Trust → Install**.
   - On iPhone: **Settings → General → About → Certificate Trust Settings** -> toggle ON **AirPad Local Root CA**.
4. Tap **Open AirPad Trackpad** (or scan the **Connect QR (HTTPS)** tab in AirPad).
   - Tip: In Safari, tap **Share → Add to Home Screen** for a full-screen app experience!

## Running Tests (Linux / Windows)

```bash
cargo test
```
