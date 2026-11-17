use crate::input::InputBackend;
use crate::protocol::{MouseButton, PressAction};
use std::sync::{Arc, Mutex};

pub struct MockInputBackend;

impl MockInputBackend {
    pub fn new() -> Self {
        Self
    }
}

impl InputBackend for MockInputBackend {
    fn move_mouse_rel(&mut self, _dx: i32, _dy: i32) {}
    fn mouse_button(&mut self, _button: MouseButton, _action: PressAction) {}
    fn scroll(&mut self, _dx: i32, _dy: i32) {}
    fn pinch_zoom(&mut self, _wheel_delta: i32) {}
    fn key_event(&mut self, _vk: u16, _action: PressAction) {}
    fn unicode_text(&mut self, _text: &str) {}
    fn release_all(&mut self) {}
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockEvent {
    MoveRel(i32, i32),
    MouseButton(MouseButton, PressAction),
    Scroll(i32, i32),
    PinchZoom(i32),
    Key(u16, PressAction),
    Text(String),
    ReleaseAll,
}

#[allow(dead_code)]
pub struct RecordingInputBackend {
    pub events: Arc<Mutex<Vec<MockEvent>>>,
}

#[allow(dead_code)]
impl RecordingInputBackend {
    pub fn new(events: Arc<Mutex<Vec<MockEvent>>>) -> Self {
        Self { events }
    }
}

impl InputBackend for RecordingInputBackend {
    fn move_mouse_rel(&mut self, dx: i32, dy: i32) {
        self.events.lock().unwrap().push(MockEvent::MoveRel(dx, dy));
    }

    fn mouse_button(&mut self, button: MouseButton, action: PressAction) {
        self.events
            .lock()
            .unwrap()
            .push(MockEvent::MouseButton(button, action));
    }

    fn scroll(&mut self, dx: i32, dy: i32) {
        self.events.lock().unwrap().push(MockEvent::Scroll(dx, dy));
    }

    fn pinch_zoom(&mut self, wheel_delta: i32) {
        self.events
            .lock()
            .unwrap()
            .push(MockEvent::PinchZoom(wheel_delta));
    }

    fn key_event(&mut self, vk: u16, action: PressAction) {
        self.events.lock().unwrap().push(MockEvent::Key(vk, action));
    }

    fn unicode_text(&mut self, text: &str) {
        self.events
            .lock()
            .unwrap()
            .push(MockEvent::Text(text.to_string()));
    }

    fn release_all(&mut self) {
        self.events.lock().unwrap().push(MockEvent::ReleaseAll);
    }
}
