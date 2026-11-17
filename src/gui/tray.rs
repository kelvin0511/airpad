use crate::server::AppState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub enum TrayCommand {
    ShowWindow,
    Quit,
}

pub struct AirPadTray {
    _tray: TrayIcon,
    show_id: MenuId,
    pause_id: MenuId,
    rotate_id: MenuId,
    quit_id: MenuId,
    pause_item: MenuItem,
    state: Arc<AppState>,
    should_quit: Arc<AtomicBool>,
}

impl AirPadTray {
    pub fn new(state: Arc<AppState>, should_quit: Arc<AtomicBool>) -> Result<Self, String> {
        let show_item = MenuItem::new("Open AirPad", true, None);
        let pause_item = MenuItem::new("Pause Input", true, None);
        let rotate_item = MenuItem::new("Rotate Pairing Token", true, None);
        let quit_item = MenuItem::new("Quit AirPad", true, None);

        let show_id = show_item.id().clone();
        let pause_id = pause_item.id().clone();
        let rotate_id = rotate_item.id().clone();
        let quit_id = quit_item.id().clone();

        let menu = Menu::new();
        menu.append(&show_item)
            .and_then(|_| menu.append(&pause_item))
            .and_then(|_| menu.append(&rotate_item))
            .and_then(|_| menu.append(&quit_item))
            .map_err(|e| format!("failed to build tray menu: {e}"))?;

        let icon = build_rgba_icon();
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("AirPad Wireless Trackpad")
            .with_icon(icon)
            .build()
            .map_err(|e| format!("failed to create tray icon: {e}"))?;

        Ok(Self {
            _tray: tray,
            show_id,
            pause_id,
            rotate_id,
            quit_id,
            pause_item,
            state,
            should_quit,
        })
    }

    pub fn poll(&self) -> Option<TrayCommand> {
        let is_paused = self.state.is_paused();
        self.pause_item
            .set_text(if is_paused { "Resume Input" } else { "Pause Input" });

        while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = ev
            {
                return Some(TrayCommand::ShowWindow);
            }
        }

        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if ev.id == self.show_id {
                return Some(TrayCommand::ShowWindow);
            } else if ev.id == self.pause_id {
                self.state.set_paused(!self.state.is_paused());
            } else if ev.id == self.rotate_id {
                self.state.rotate_token();
            } else if ev.id == self.quit_id {
                self.should_quit.store(true, Ordering::SeqCst);
                return Some(TrayCommand::Quit);
            }
        }

        None
    }
}

fn build_rgba_icon() -> Icon {
    const SIZE: usize = 32;
    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let idx = (y * SIZE + x) * 4;
            let in_pad = (3..29).contains(&x) && (3..29).contains(&y);
            let is_border = in_pad && (x <= 4 || x >= 27 || y <= 4 || y >= 27);
            let is_cursor = (10..22).contains(&x) && (9..23).contains(&y) && (x - 10) <= (y - 9);

            if is_cursor {
                rgba[idx] = 240;
                rgba[idx + 1] = 246;
                rgba[idx + 2] = 255;
                rgba[idx + 3] = 255;
            } else if is_border {
                rgba[idx] = 59;
                rgba[idx + 1] = 130;
                rgba[idx + 2] = 246;
                rgba[idx + 3] = 255;
            } else if in_pad {
                rgba[idx] = 19;
                rgba[idx + 1] = 28;
                rgba[idx + 2] = 44;
                rgba[idx + 3] = 255;
            }
        }
    }
    Icon::from_rgba(rgba, SIZE as u32, SIZE as u32).expect("valid 32x32 RGBA icon")
}
