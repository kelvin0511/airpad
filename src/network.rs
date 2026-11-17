use local_ip_address::list_afinet_netifas;
use qrcode::{Color, QrCode};
use std::net::{IpAddr, Ipv4Addr};

#[derive(Debug, Clone)]
pub struct NetworkInterfaceInfo {
    pub name: String,
    pub ip: Ipv4Addr,
    pub is_likely_virtual: bool,
}

pub fn detect_lan_interfaces() -> Vec<NetworkInterfaceInfo> {
    let mut out = Vec::new();
    let Ok(ifaces) = list_afinet_netifas() else {
        return out;
    };

    for (name, ip) in ifaces {
        let IpAddr::V4(v4) = ip else {
            continue;
        };
        if v4.is_loopback() || v4.is_link_local() || v4.is_unspecified() {
            continue;
        }
        let lower = name.to_lowercase();
        let is_likely_virtual = lower.contains("vethernet")
            || lower.contains("wsl")
            || lower.contains("hyper-v")
            || lower.contains("vmware")
            || lower.contains("virtualbox")
            || lower.contains("vbox")
            || lower.contains("docker")
            || lower.contains("tailscale")
            || lower.contains("zerotier")
            || lower.contains("loopback")
            || lower.starts_with("br-")
            || lower.starts_with("veth");

        out.push(NetworkInterfaceInfo {
            name,
            ip: v4,
            is_likely_virtual,
        });
    }

    // Sort physical adapters first, then private RFC1918 (192.168 / 10 / 172.16-31), then name.
    out.sort_by_key(|info| {
        let octets = info.ip.octets();
        let is_192 = octets[0] == 192 && octets[1] == 168;
        let is_10 = octets[0] == 10;
        let prio = if is_192 {
            0
        } else if is_10 {
            1
        } else if info.ip.is_private() {
            2
        } else {
            3
        };
        (info.is_likely_virtual, prio, info.name.clone())
    });

    out
}

pub fn choose_default_ip(ifaces: &[NetworkInterfaceInfo], preferred: Option<&str>) -> Option<Ipv4Addr> {
    if let Some(pref) = preferred {
        if let Ok(parsed) = pref.parse::<Ipv4Addr>() {
            if ifaces.iter().any(|i| i.ip == parsed) {
                return Some(parsed);
            }
        }
    }
    ifaces.first().map(|i| i.ip)
}

pub fn qr_matrix(data: &str) -> Option<(usize, Vec<bool>)> {
    let code = QrCode::new(data.as_bytes()).ok()?;
    let width = code.width();
    let modules = code
        .to_colors()
        .into_iter()
        .map(|c| matches!(c, Color::Dark))
        .collect();
    Some((width, modules))
}

pub fn render_ascii_qr(data: &str) -> String {
    let Some((w, modules)) = qr_matrix(data) else {
        return String::new();
    };
    let mut out = String::new();
    let quiet = 2;
    let total = w + quiet * 2;
    let get = |x: isize, y: isize| -> bool {
        let xi = x - quiet as isize;
        let yi = y - quiet as isize;
        if xi < 0 || yi < 0 || xi >= w as isize || yi >= w as isize {
            false
        } else {
            modules[yi as usize * w + xi as usize]
        }
    };

    let mut y = 0isize;
    while y < total as isize {
        for x in 0..total as isize {
            let top = get(x, y);
            let bot = if y + 1 < total as isize {
                get(x, y + 1)
            } else {
                false
            };
            let ch = match (top, bot) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            };
            out.push(ch);
        }
        out.push('\n');
        y += 2;
    }
    out
}
