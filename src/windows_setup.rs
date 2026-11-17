#[cfg(windows)]
mod imp {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x08000000;

    fn rule_names(https_port: u16, http_port: u16) -> (String, String) {
        (
            format!("AirPad HTTPS {https_port}"),
            format!("AirPad Setup {http_port}"),
        )
    }

    fn rule_exists(name: &str) -> bool {
        Command::new("netsh")
            .args(["advfirewall", "firewall", "show", "rule", &format!("name={name}")])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn is_firewall_configured(https_port: u16, http_port: u16) -> bool {
        let (r1, r2) = rule_names(https_port, http_port);
        rule_exists(&r1) && rule_exists(&r2)
    }

    fn encode_ps_utf16_b64(script: &str) -> String {
        let bytes: Vec<u8> = script
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
        for chunk in bytes.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
            let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
            let n = (b0 << 16) | (b1 << 8) | b2;
            out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
            out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
            out.push(if chunk.len() > 1 {
                TABLE[((n >> 6) & 0x3F) as usize] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                TABLE[(n & 0x3F) as usize] as char
            } else {
                '='
            });
        }
        out
    }

    /// Returns Ok(true) if newly configured, Ok(false) if already present and skipped.
    pub fn ensure_firewall_rules(https_port: u16, http_port: u16) -> Result<bool, String> {
        if is_firewall_configured(https_port, http_port) {
            return Ok(false);
        }

        let (r1, r2) = rule_names(https_port, http_port);
        let inner_script = format!(
            "netsh advfirewall firewall delete rule name='{r1}' | Out-Null; \
             netsh advfirewall firewall delete rule name='{r2}' | Out-Null; \
             netsh advfirewall firewall add rule name='{r1}' dir=in action=allow protocol=TCP localport={https_port} profile=any | Out-Null; \
             netsh advfirewall firewall add rule name='{r2}' dir=in action=allow protocol=TCP localport={http_port} profile=any | Out-Null"
        );
        let inner_b64 = encode_ps_utf16_b64(&inner_script);
        let outer_cmd = format!(
            "Start-Process powershell -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList '-NoProfile','-NonInteractive','-WindowStyle','Hidden','-EncodedCommand','{inner_b64}'"
        );

        let status = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                &outer_cmd,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| format!("failed to launch PowerShell firewall setup: {e}"))?;

        if !status.success() {
            return Err("Firewall elevation was canceled or failed.".to_string());
        }

        if is_firewall_configured(https_port, http_port) {
            Ok(true)
        } else {
            Err("Firewall rules were not created.".to_string())
        }
    }

    pub fn is_autostart_enabled() -> bool {
        Command::new("reg")
            .args([
                "query",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "AirPad",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn set_autostart(enabled: bool) -> Result<(), String> {
        if enabled {
            let exe = std::env::current_exe()
                .map_err(|e| format!("cannot locate executable path: {e}"))?;
            let quoted = format!("\"{}\"", exe.display());
            let out = Command::new("reg")
                .args([
                    "add",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                    "/v",
                    "AirPad",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &quoted,
                    "/f",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map_err(|e| format!("failed to update startup registry: {e}"))?;
            if !out.status.success() {
                return Err("Failed to enable Start with Windows.".to_string());
            }
        } else {
            let _ = Command::new("reg")
                .args([
                    "delete",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                    "/v",
                    "AirPad",
                    "/f",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn is_firewall_configured(_https_port: u16, _http_port: u16) -> bool {
        true
    }

    pub fn ensure_firewall_rules(_https_port: u16, _http_port: u16) -> Result<bool, String> {
        Ok(false)
    }

    pub fn is_autostart_enabled() -> bool {
        false
    }

    pub fn set_autostart(_enabled: bool) -> Result<(), String> {
        Ok(())
    }
}

pub use imp::{
    ensure_firewall_rules, is_autostart_enabled, is_firewall_configured, set_autostart,
};
