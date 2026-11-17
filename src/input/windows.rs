use crate::input::InputBackend;
use crate::protocol::{MouseButton, PressAction};
use std::collections::HashSet;
use std::mem::size_of;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT, VK_CONTROL,
    VK_DELETE, VK_DOWN, VK_END, VK_HOME, VK_INSERT, VK_LEFT, VK_LWIN, VK_NEXT, VK_PRIOR,
    VK_RCONTROL, VK_RETURN, VK_RIGHT, VK_RMENU, VK_RWIN, VK_UP,
};

pub struct WindowsInputBackend {
    held_buttons: [bool; 3],
    held_keys: HashSet<u16>,
}

impl WindowsInputBackend {
    pub fn new() -> Self {
        Self {
            held_buttons: [false; 3],
            held_keys: HashSet::new(),
        }
    }

    fn send_inputs(inputs: &[INPUT]) {
        if inputs.is_empty() {
            return;
        }
        unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                size_of::<INPUT>() as i32,
            );
        }
    }

    fn mouse_input(dx: i32, dy: i32, mouse_data: u32, flags: u32) -> INPUT {
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: mouse_data,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn key_input(vk: u16, scan: u16, flags: u32) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn is_extended_vk(vk: u16) -> bool {
        matches!(
            vk,
            VK_LEFT
                | VK_UP
                | VK_RIGHT
                | VK_DOWN
                | VK_INSERT
                | VK_DELETE
                | VK_HOME
                | VK_END
                | VK_PRIOR
                | VK_NEXT
                | VK_RCONTROL
                | VK_RMENU
                | VK_LWIN
                | VK_RWIN
        )
    }

    fn btn_index(button: MouseButton) -> usize {
        match button {
            MouseButton::Left => 0,
            MouseButton::Right => 1,
            MouseButton::Middle => 2,
        }
    }

    fn btn_flags(button: MouseButton) -> (u32, u32) {
        match button {
            MouseButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
            MouseButton::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
            MouseButton::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
        }
    }
}

impl InputBackend for WindowsInputBackend {
    fn move_mouse_rel(&mut self, dx: i32, dy: i32) {
        let inp = Self::mouse_input(dx, dy, 0, MOUSEEVENTF_MOVE);
        Self::send_inputs(&[inp]);
    }

    fn mouse_button(&mut self, button: MouseButton, action: PressAction) {
        let idx = Self::btn_index(button);
        let (down_flag, up_flag) = Self::btn_flags(button);

        match action {
            PressAction::Down => {
                self.held_buttons[idx] = true;
                Self::send_inputs(&[Self::mouse_input(0, 0, 0, down_flag)]);
            }
            PressAction::Up => {
                self.held_buttons[idx] = false;
                Self::send_inputs(&[Self::mouse_input(0, 0, 0, up_flag)]);
            }
            PressAction::Click => {
                self.held_buttons[idx] = false;
                Self::send_inputs(&[
                    Self::mouse_input(0, 0, 0, down_flag),
                    Self::mouse_input(0, 0, 0, up_flag),
                ]);
            }
        }
    }

    fn scroll(&mut self, dx: i32, dy: i32) {
        let mut batch = Vec::with_capacity(2);
        if dy != 0 {
            batch.push(Self::mouse_input(0, 0, dy as u32, MOUSEEVENTF_WHEEL));
        }
        if dx != 0 {
            batch.push(Self::mouse_input(0, 0, dx as u32, MOUSEEVENTF_HWHEEL));
        }
        Self::send_inputs(&batch);
    }

    fn pinch_zoom(&mut self, wheel_delta: i32) {
        let batch = [
            Self::key_input(VK_CONTROL, 0, 0),
            Self::mouse_input(0, 0, wheel_delta as u32, MOUSEEVENTF_WHEEL),
            Self::key_input(VK_CONTROL, 0, KEYEVENTF_KEYUP),
        ];
        Self::send_inputs(&batch);
    }

    fn key_event(&mut self, vk: u16, action: PressAction) {
        let ext = if Self::is_extended_vk(vk) {
            KEYEVENTF_EXTENDEDKEY
        } else {
            0
        };
        match action {
            PressAction::Down => {
                self.held_keys.insert(vk);
                Self::send_inputs(&[Self::key_input(vk, 0, ext)]);
            }
            PressAction::Up => {
                self.held_keys.remove(&vk);
                Self::send_inputs(&[Self::key_input(vk, 0, ext | KEYEVENTF_KEYUP)]);
            }
            PressAction::Click => {
                self.held_keys.remove(&vk);
                Self::send_inputs(&[
                    Self::key_input(vk, 0, ext),
                    Self::key_input(vk, 0, ext | KEYEVENTF_KEYUP),
                ]);
            }
        }
    }

    fn unicode_text(&mut self, text: &str) {
        let mut batch = Vec::with_capacity(text.len() * 2);
        for ch in text.chars() {
            if ch == '\n' {
                batch.push(Self::key_input(VK_RETURN, 0, 0));
                batch.push(Self::key_input(VK_RETURN, 0, KEYEVENTF_KEYUP));
                continue;
            }
            if ch == '\r' {
                continue;
            }
            let mut buf = [0u16; 2];
            for &unit in ch.encode_utf16(&mut buf) {
                batch.push(Self::key_input(0, unit, KEYEVENTF_UNICODE));
                batch.push(Self::key_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
            }
        }
        Self::send_inputs(&batch);
    }

    fn release_all(&mut self) {
        let mut batch = Vec::new();
        for (idx, held) in self.held_buttons.iter_mut().enumerate() {
            if *held {
                *held = false;
                let btn = match idx {
                    0 => MouseButton::Left,
                    1 => MouseButton::Right,
                    _ => MouseButton::Middle,
                };
                let (_, up_flag) = Self::btn_flags(btn);
                batch.push(Self::mouse_input(0, 0, 0, up_flag));
            }
        }
        for vk in self.held_keys.drain() {
            let ext = if Self::is_extended_vk(vk) {
                KEYEVENTF_EXTENDEDKEY
            } else {
                0
            };
            batch.push(Self::key_input(vk, 0, ext | KEYEVENTF_KEYUP));
        }
        Self::send_inputs(&batch);
    }
}
