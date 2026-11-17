use crate::network::{choose_default_ip, detect_lan_interfaces, qr_matrix, NetworkInterfaceInfo};
use crate::server::{format_duration, AppState};
use crate::windows_setup;
use eframe::egui;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[cfg(windows)]
use crate::gui::tray::{AirPadTray, TrayCommand};

#[derive(PartialEq, Eq, Clone, Copy)]
enum QrTab {
    ConnectHttps,
    SetupCertHttp,
}

pub fn run_gui(state: Arc<AppState>) -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([430.0, 700.0])
            .with_min_inner_size([380.0, 600.0])
            .with_title("AirPad — Wireless iPhone Trackpad"),
        ..Default::default()
    };

    eframe::run_native(
        "AirPad",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(AirPadWindow::new(state)))
        }),
    )
    .map_err(|e| format!("GUI error: {e}"))
}

struct AirPadWindow {
    state: Arc<AppState>,
    interfaces: Vec<NetworkInterfaceInfo>,
    selected_ip: Option<Ipv4Addr>,
    qr_tab: QrTab,
    cached_qr_url: String,
    cached_qr: Option<(usize, Vec<bool>)>,
    copied_flash: f64,
    firewall_ok: Arc<AtomicBool>,
    firewall_busy: Arc<AtomicBool>,
    autostart: bool,
    #[cfg(windows)]
    should_quit: Arc<AtomicBool>,
    #[cfg(windows)]
    tray: Option<AirPadTray>,
}

impl AirPadWindow {
    fn new(state: Arc<AppState>) -> Self {
        let interfaces = detect_lan_interfaces();
        let (pref, https_port, http_port) = {
            let cfg = state.config.read().unwrap();
            (cfg.preferred_ip.clone(), cfg.https_port, cfg.http_onboarding_port)
        };
        let selected_ip = choose_default_ip(&interfaces, pref.as_deref());
        let firewall_ok = Arc::new(AtomicBool::new(false));
        let firewall_busy = Arc::new(AtomicBool::new(false));

        let fw_check = firewall_ok.clone();
        std::thread::spawn(move || {
            let ok = windows_setup::is_firewall_configured(https_port, http_port);
            fw_check.store(ok, Ordering::SeqCst);
        });

        let autostart = windows_setup::is_autostart_enabled();

        #[cfg(windows)]
        let should_quit = Arc::new(AtomicBool::new(false));

        #[cfg(windows)]
        let tray = AirPadTray::new(state.clone(), should_quit.clone()).ok();

        Self {
            state,
            interfaces,
            selected_ip,
            qr_tab: QrTab::ConnectHttps,
            cached_qr_url: String::new(),
            cached_qr: None,
            copied_flash: 0.0,
            firewall_ok,
            firewall_busy,
            autostart,
            #[cfg(windows)]
            should_quit,
            #[cfg(windows)]
            tray,
        }
    }

    fn refresh_interfaces(&mut self) {
        self.interfaces = detect_lan_interfaces();
        let pref = self.state.config.read().unwrap().preferred_ip.clone();
        self.selected_ip = choose_default_ip(&self.interfaces, pref.as_deref());
    }

    fn current_url(&self) -> String {
        let ip = self
            .selected_ip
            .map(|v| v.to_string())
            .unwrap_or_else(|| "127.0.0.1".to_string());
        match self.qr_tab {
            QrTab::ConnectHttps => self.state.connect_url(&ip),
            QrTab::SetupCertHttp => self.state.setup_url(&ip),
        }
    }

    fn ensure_qr(&mut self, url: &str) {
        if self.cached_qr_url != url {
            self.cached_qr_url = url.to_string();
            self.cached_qr = qr_matrix(url);
        }
    }
}

impl eframe::App for AirPadWindow {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(250));

        #[cfg(windows)]
        if let Some(tray) = &self.tray {
            while let Some(cmd) = tray.poll() {
                match cmd {
                    TrayCommand::ShowWindow => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    TrayCommand::Quit => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }

            if ctx.input(|i| i.viewport().close_requested()) && !self.should_quit.load(Ordering::SeqCst)
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }

        let url = self.current_url();
        self.ensure_qr(&url);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 10.0;

            // Header & Live Connection Status
            ui.horizontal(|ui| {
                ui.heading("AirPad");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let paused = self.state.is_paused();
                    let client = self.state.connected_client.read().unwrap().clone();
                    if paused {
                        ui.colored_label(egui::Color32::from_rgb(245, 158, 11), "PAUSED");
                    } else if let Some(info) = client {
                        let dur = format_duration(info.connected_at.elapsed());
                        ui.colored_label(
                            egui::Color32::from_rgb(34, 197, 94),
                            format!("Connected: {} ({})", info.addr, dur),
                        );
                    } else {
                        ui.colored_label(egui::Color32::from_rgb(148, 163, 184), "Waiting for iPhone…");
                    }
                });
            });

            // Quick Actions
            ui.horizontal(|ui| {
                let paused = self.state.is_paused();
                if ui
                    .button(if paused { "▶ Resume Input" } else { "⏸ Pause Input" })
                    .clicked()
                {
                    self.state.set_paused(!paused);
                }
                if ui.button("🔌 Disconnect").clicked() {
                    self.state.disconnect_active();
                }
                if ui.button("🔄 New Token").clicked() {
                    self.state.rotate_token();
                }
            });

            ui.separator();

            // Network Interface Selector
            ui.horizontal(|ui| {
                ui.label("LAN IP:");
                let current_label = self
                    .selected_ip
                    .map(|ip| ip.to_string())
                    .unwrap_or_else(|| "No LAN IP found".to_string());

                egui::ComboBox::from_id_salt("lan_ip_combo")
                    .selected_text(current_label)
                    .width(240.0)
                    .show_ui(ui, |ui| {
                        for iface in &self.interfaces {
                            let tag = if iface.is_likely_virtual { " [virtual]" } else { "" };
                            let label = format!("{} — {}{}", iface.ip, iface.name, tag);
                            let is_sel = self.selected_ip == Some(iface.ip);
                            if ui.selectable_label(is_sel, label).clicked() {
                                self.selected_ip = Some(iface.ip);
                                self.state.set_preferred_ip(iface.ip.to_string());
                            }
                        }
                    });

                if ui.button("↻").on_hover_text("Refresh network interfaces").clicked() {
                    self.refresh_interfaces();
                }
            });

            // QR Mode Tabs
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.qr_tab, QrTab::ConnectHttps, "1. Connect QR (HTTPS)");
                ui.selectable_value(
                    &mut self.qr_tab,
                    QrTab::SetupCertHttp,
                    "First-Time iPhone Cert Setup",
                );
            });

            // QR Code Canvas
            ui.vertical_centered(|ui| {
                if let Some((dim, modules)) = &self.cached_qr {
                    let qr_px = 220.0_f32;
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(qr_px, qr_px), egui::Sense::hover());
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, 8.0, egui::Color32::WHITE);

                    let quiet = 2.0_f32;
                    let cell = qr_px / (*dim as f32 + quiet * 2.0);
                    let origin = rect.min + egui::vec2(quiet * cell, quiet * cell);

                    for y in 0..*dim {
                        for x in 0..*dim {
                            if modules[y * *dim + x] {
                                let p0 = origin + egui::vec2(x as f32 * cell, y as f32 * cell);
                                let mod_rect = egui::Rect::from_min_size(p0, egui::vec2(cell + 0.35, cell + 0.35));
                                painter.rect_filled(mod_rect, 0.0, egui::Color32::BLACK);
                            }
                        }
                    }
                }

                ui.add_space(4.0);
                let hint = match self.qr_tab {
                    QrTab::ConnectHttps => "Scan with iPhone Camera to open Trackpad & Air-Mouse",
                    QrTab::SetupCertHttp => "First time? Scan this to install & trust the local HTTPS profile on iPhone",
                };
                ui.label(egui::RichText::new(hint).small().color(egui::Color32::GRAY));

                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(egui::RichText::new(&url).monospace().small()).wrap());
                    if ui.button("Copy").clicked() {
                        ctx.copy_text(url.clone());
                        self.copied_flash = ui.input(|i| i.time);
                    }
                });
                if ui.input(|i| i.time) - self.copied_flash < 1.5 && self.copied_flash > 0.0 {
                    ui.colored_label(egui::Color32::from_rgb(34, 197, 94), "Copied URL!");
                }
            });

            ui.separator();

            // Sensitivity Sliders
            let mut sens = self.state.config.read().unwrap().sensitivity;
            if ui
                .add(egui::Slider::new(&mut sens, 0.3..=3.0).text("PC Cursor Sensitivity"))
                .changed()
            {
                self.state.set_sensitivity(sens);
            }

            let mut scroll = self.state.config.read().unwrap().scroll_speed;
            if ui
                .add(egui::Slider::new(&mut scroll, 0.3..=3.0).text("PC Scroll Speed"))
                .changed()
            {
                self.state.set_scroll_speed(scroll);
            }

            ui.separator();

            // Environment & Startup
            ui.horizontal(|ui| {
                let fw_ok = self.firewall_ok.load(Ordering::SeqCst);
                let fw_busy = self.firewall_busy.load(Ordering::SeqCst);
                if fw_ok {
                    ui.colored_label(egui::Color32::from_rgb(34, 197, 94), "Firewall: Ready");
                } else if fw_busy {
                    ui.colored_label(egui::Color32::from_rgb(245, 158, 11), "Configuring Firewall…");
                } else if ui.button("Allow in Windows Firewall").clicked() {
                    let (https_port, http_port) = {
                        let cfg = self.state.config.read().unwrap();
                        (cfg.https_port, cfg.http_onboarding_port)
                    };
                    let ok_flag = self.firewall_ok.clone();
                    let busy_flag = self.firewall_busy.clone();
                    busy_flag.store(true, Ordering::SeqCst);
                    std::thread::spawn(move || {
                        let _ = windows_setup::ensure_firewall_rules(https_port, http_port);
                        ok_flag.store(
                            windows_setup::is_firewall_configured(https_port, http_port),
                            Ordering::SeqCst,
                        );
                        busy_flag.store(false, Ordering::SeqCst);
                    });
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.checkbox(&mut self.autostart, "Start with Windows").changed() {
                        if windows_setup::set_autostart(self.autostart).is_err() {
                            self.autostart = windows_setup::is_autostart_enabled();
                        }
                    }
                });
            });

            #[cfg(windows)]
            ui.label(
                egui::RichText::new("Closing this window minimizes AirPad to the Windows System Tray.")
                    .small()
                    .color(egui::Color32::GRAY),
            );
        });
    }
}
