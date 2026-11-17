#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod certs;
mod config;
mod gui;
mod input;
mod network;
mod protocol;
mod server;

use clap::Parser;
use config::Config;
use network::{choose_default_ip, detect_lan_interfaces, render_ascii_qr};
use server::{run_servers, AppState};
use std::net::Ipv4Addr;
use tracing::{error, info};

#[derive(Parser, Debug)]
#[command(name = "airpad", version, about = "iPhone Wi-Fi Wireless Mouse & Trackpad Server for Windows")]
struct Cli {
    /// Run without a desktop GUI window and print QR codes in the terminal
    #[arg(long)]
    headless: bool,

    /// Override HTTPS port (default 8443)
    #[arg(long)]
    https_port: Option<u16>,

    /// Override HTTP onboarding/cert setup port (default 8442)
    #[arg(long)]
    http_port: Option<u16>,

    /// Generate a fresh pairing token on startup
    #[arg(long)]
    rotate_token: bool,
}

#[cfg(windows)]
fn enable_dpi_awareness() {
    use windows_sys::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

fn main() {
    #[cfg(windows)]
    enable_dpi_awareness();

    let _ = rustls::crypto::ring::default_provider().install_default();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let mut cfg = Config::load_or_create();
    if let Some(p) = cli.https_port {
        cfg.https_port = p;
    }
    if let Some(p) = cli.http_port {
        cfg.http_onboarding_port = p;
    }
    if cli.rotate_token {
        cfg.pairing_token = config::generate_token();
        let _ = cfg.save();
    }

    let ifaces = detect_lan_interfaces();
    let lan_ips: Vec<Ipv4Addr> = ifaces.iter().map(|i| i.ip).collect();
    let default_ip = choose_default_ip(&ifaces, cfg.preferred_ip.as_deref())
        .unwrap_or(Ipv4Addr::LOCALHOST);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to create Tokio runtime");

    let tls_bundle = rt
        .block_on(certs::build_tls_bundle(&lan_ips))
        .expect("failed to initialize local TLS certificates");

    let https_port = cfg.https_port;
    let http_port = cfg.http_onboarding_port;
    let state = AppState::new(cfg, tls_bundle.ca_der.clone(), tls_bundle.mobileconfig.clone());

    let headless = cli.headless || !cfg!(feature = "gui");

    if headless {
        let ip_str = default_ip.to_string();
        let setup_url = state.setup_url(&ip_str);
        let connect_url = state.connect_url(&ip_str);

        println!("\n=== AirPad (Headless Mode) ===");
        println!("First-time iPhone Cert Setup: {setup_url}");
        println!("{}", render_ascii_qr(&setup_url));
        println!("Direct Trackpad Connect (HTTPS): {connect_url}");
        println!("{}", render_ascii_qr(&connect_url));

        rt.block_on(async move {
            tokio::select! {
                res = run_servers(state, tls_bundle, https_port, http_port) => {
                    if let Err(e) = res {
                        error!("Server exited with error: {e}");
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("Shutting down AirPad.");
                }
            }
        });
    } else {
        let server_state = state.clone();
        std::thread::spawn(move || {
            rt.block_on(async move {
                if let Err(e) = run_servers(server_state, tls_bundle, https_port, http_port).await {
                    error!("Server exited with error: {e}");
                }
            });
        });

        #[cfg(feature = "gui")]
        if let Err(e) = gui::window::run_gui(state) {
            error!("Failed to launch AirPad GUI: {e}");
        }
    }
}
