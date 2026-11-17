pub mod mock;
#[cfg(windows)]
pub mod windows;

use crate::protocol::{ClientPacket, MouseButton, PressAction};

pub trait InputBackend: Send + Sync {
    fn move_mouse_rel(&mut self, dx: i32, dy: i32);
    fn mouse_button(&mut self, button: MouseButton, action: PressAction);
    fn scroll(&mut self, dx: i32, dy: i32);
    fn pinch_zoom(&mut self, wheel_delta: i32);
    fn key_event(&mut self, vk: u16, action: PressAction);
    fn unicode_text(&mut self, text: &str);
    fn release_all(&mut self);
}

pub fn create_default_backend() -> Box<dyn InputBackend> {
    #[cfg(windows)]
    {
        Box::new(windows::WindowsInputBackend::new())
    }
    #[cfg(not(windows))]
    {
        Box::new(mock::MockInputBackend::new())
    }
}

pub struct InputProcessor {
    backend: Box<dyn InputBackend>,
    rem_x: f32,
    rem_y: f32,
    rem_scroll_x: f32,
    rem_scroll_y: f32,
    rem_pinch: f32,
    pub sensitivity: f32,
    pub scroll_speed: f32,
}

impl InputProcessor {
    pub fn new(backend: Box<dyn InputBackend>, sensitivity: f32, scroll_speed: f32) -> Self {
        Self {
            backend,
            rem_x: 0.0,
            rem_y: 0.0,
            rem_scroll_x: 0.0,
            rem_scroll_y: 0.0,
            rem_pinch: 0.0,
            sensitivity,
            scroll_speed,
        }
    }

    pub fn process(&mut self, packet: ClientPacket) {
        match packet {
            ClientPacket::Move { dx, dy } => {
                self.rem_x += dx * self.sensitivity;
                self.rem_y += dy * self.sensitivity;

                let ix = self.rem_x.trunc() as i32;
                let iy = self.rem_y.trunc() as i32;

                if ix != 0 || iy != 0 {
                    self.rem_x -= ix as f32;
                    self.rem_y -= iy as f32;
                    self.backend.move_mouse_rel(ix, iy);
                }
            }
            ClientPacket::MouseButton { button, action } => {
                self.backend.mouse_button(button, action);
            }
            ClientPacket::Scroll { dx, dy } => {
                const WHEEL_DELTA: f32 = 120.0;
                self.rem_scroll_x += dx * self.scroll_speed * WHEEL_DELTA;
                self.rem_scroll_y += dy * self.scroll_speed * WHEEL_DELTA;

                let ix = self.rem_scroll_x.trunc() as i32;
                let iy = self.rem_scroll_y.trunc() as i32;
                if ix != 0 || iy != 0 {
                    self.rem_scroll_x -= ix as f32;
                    self.rem_scroll_y -= iy as f32;
                    self.backend.scroll(ix, iy);
                }
            }
            ClientPacket::Pinch { scale_delta } => {
                const WHEEL_DELTA: f32 = 120.0;
                self.rem_pinch += scale_delta * WHEEL_DELTA;
                let iz = self.rem_pinch.trunc() as i32;
                if iz != 0 {
                    self.rem_pinch -= iz as f32;
                    self.backend.pinch_zoom(iz);
                }
            }
            ClientPacket::Key { vk, action } => {
                self.backend.key_event(vk, action);
            }
            ClientPacket::Text(text) => {
                self.backend.unicode_text(&text);
            }
            ClientPacket::ReleaseAll => {
                self.release_all();
            }
            ClientPacket::Ping { .. } => {}
        }
    }

    pub fn release_all(&mut self) {
        self.rem_x = 0.0;
        self.rem_y = 0.0;
        self.rem_scroll_x = 0.0;
        self.rem_scroll_y = 0.0;
        self.rem_pinch = 0.0;
        self.backend.release_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::mock::{MockEvent, RecordingInputBackend};
    use std::sync::{Arc, Mutex};

    #[test]
    fn accumulates_subpixel_mouse_deltas() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let backend = Box::new(RecordingInputBackend::new(log.clone()));
        let mut proc = InputProcessor::new(backend, 1.0, 1.0);

        proc.process(ClientPacket::Move { dx: 0.4, dy: 0.3 });
        proc.process(ClientPacket::Move { dx: 0.4, dy: 0.3 });
        assert!(log.lock().unwrap().is_empty());

        proc.process(ClientPacket::Move { dx: 0.4, dy: 0.5 });
        let events = log.lock().unwrap().clone();
        assert_eq!(events, vec![MockEvent::MoveRel(1, 1)]);
    }

    #[test]
    fn scales_scroll_to_wheel_delta() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let backend = Box::new(RecordingInputBackend::new(log.clone()));
        let mut proc = InputProcessor::new(backend, 1.0, 1.0);

        proc.process(ClientPacket::Scroll { dx: 0.0, dy: 0.5 });
        let events = log.lock().unwrap().clone();
        assert_eq!(events, vec![MockEvent::Scroll(0, 60)]);
    }
}
