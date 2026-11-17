use std::fmt;

pub const OP_MOVE: u8 = 0x01;
pub const OP_MOUSE_BTN: u8 = 0x02;
pub const OP_SCROLL: u8 = 0x03;
pub const OP_PINCH: u8 = 0x04;
pub const OP_KEY: u8 = 0x05;
pub const OP_TEXT: u8 = 0x06;
pub const OP_PING: u8 = 0x07;
pub const OP_RELEASE_ALL: u8 = 0x08;
pub const OP_PONG: u8 = 0x87;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

impl TryFrom<u8> for MouseButton {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(MouseButton::Left),
            1 => Ok(MouseButton::Right),
            2 => Ok(MouseButton::Middle),
            _ => Err(ProtocolError::InvalidValue("unknown mouse button")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressAction {
    Up,
    Down,
    Click,
}

impl TryFrom<u8> for PressAction {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(PressAction::Up),
            1 => Ok(PressAction::Down),
            2 => Ok(PressAction::Click),
            _ => Err(ProtocolError::InvalidValue("unknown press action")),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClientPacket {
    Move { dx: f32, dy: f32 },
    MouseButton { button: MouseButton, action: PressAction },
    Scroll { dx: f32, dy: f32 },
    Pinch { scale_delta: f32 },
    Key { vk: u16, action: PressAction },
    Text(String),
    Ping { client_ts_ms: u64 },
    ReleaseAll,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    EmptyPacket,
    Truncated,
    UnknownOpcode(u8),
    InvalidValue(&'static str),
    InvalidUtf8,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolError::EmptyPacket => write!(f, "empty packet"),
            ProtocolError::Truncated => write!(f, "truncated packet"),
            ProtocolError::UnknownOpcode(op) => write!(f, "unknown opcode 0x{op:02x}"),
            ProtocolError::InvalidValue(msg) => write!(f, "invalid field value: {msg}"),
            ProtocolError::InvalidUtf8 => write!(f, "invalid utf-8 text payload"),
        }
    }
}

impl std::error::Error for ProtocolError {}

fn read_f32_le(bytes: &[u8]) -> Result<f32, ProtocolError> {
    let arr: [u8; 4] = bytes.try_into().map_err(|_| ProtocolError::Truncated)?;
    let val = f32::from_le_bytes(arr);
    if !val.is_finite() {
        return Err(ProtocolError::InvalidValue("non-finite float"));
    }
    Ok(val)
}

fn read_u16_le(bytes: &[u8]) -> Result<u16, ProtocolError> {
    let arr: [u8; 2] = bytes.try_into().map_err(|_| ProtocolError::Truncated)?;
    Ok(u16::from_le_bytes(arr))
}

fn read_u64_le(bytes: &[u8]) -> Result<u64, ProtocolError> {
    let arr: [u8; 8] = bytes.try_into().map_err(|_| ProtocolError::Truncated)?;
    Ok(u64::from_le_bytes(arr))
}

pub fn parse_client_packet(data: &[u8]) -> Result<ClientPacket, ProtocolError> {
    let (&op, rest) = data.split_first().ok_or(ProtocolError::EmptyPacket)?;
    match op {
        OP_MOVE => {
            if rest.len() < 8 {
                return Err(ProtocolError::Truncated);
            }
            let dx = read_f32_le(&rest[0..4])?;
            let dy = read_f32_le(&rest[4..8])?;
            Ok(ClientPacket::Move { dx, dy })
        }
        OP_MOUSE_BTN => {
            if rest.len() < 2 {
                return Err(ProtocolError::Truncated);
            }
            let button = MouseButton::try_from(rest[0])?;
            let action = PressAction::try_from(rest[1])?;
            Ok(ClientPacket::MouseButton { button, action })
        }
        OP_SCROLL => {
            if rest.len() < 8 {
                return Err(ProtocolError::Truncated);
            }
            let dx = read_f32_le(&rest[0..4])?;
            let dy = read_f32_le(&rest[4..8])?;
            Ok(ClientPacket::Scroll { dx, dy })
        }
        OP_PINCH => {
            if rest.len() < 4 {
                return Err(ProtocolError::Truncated);
            }
            let scale_delta = read_f32_le(&rest[0..4])?;
            Ok(ClientPacket::Pinch { scale_delta })
        }
        OP_KEY => {
            if rest.len() < 3 {
                return Err(ProtocolError::Truncated);
            }
            let vk = read_u16_le(&rest[0..2])?;
            let action = PressAction::try_from(rest[2])?;
            Ok(ClientPacket::Key { vk, action })
        }
        OP_TEXT => {
            if rest.len() < 2 {
                return Err(ProtocolError::Truncated);
            }
            let len = read_u16_le(&rest[0..2])? as usize;
            let payload = rest.get(2..2 + len).ok_or(ProtocolError::Truncated)?;
            let text = std::str::from_utf8(payload)
                .map_err(|_| ProtocolError::InvalidUtf8)?
                .to_owned();
            Ok(ClientPacket::Text(text))
        }
        OP_PING => {
            if rest.len() < 8 {
                return Err(ProtocolError::Truncated);
            }
            let client_ts_ms = read_u64_le(&rest[0..8])?;
            Ok(ClientPacket::Ping { client_ts_ms })
        }
        OP_RELEASE_ALL => Ok(ClientPacket::ReleaseAll),
        other => Err(ProtocolError::UnknownOpcode(other)),
    }
}

pub fn encode_pong(client_ts_ms: u64) -> [u8; 9] {
    let mut out = [0u8; 9];
    out[0] = OP_PONG;
    out[1..9].copy_from_slice(&client_ts_ms.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_move_packet() {
        let mut buf = vec![OP_MOVE];
        buf.extend_from_slice(&12.5_f32.to_le_bytes());
        buf.extend_from_slice(&(-3.25_f32).to_le_bytes());
        assert_eq!(
            parse_client_packet(&buf).unwrap(),
            ClientPacket::Move { dx: 12.5, dy: -3.25 }
        );
    }

    #[test]
    fn rejects_nan_move() {
        let mut buf = vec![OP_MOVE];
        buf.extend_from_slice(&f32::NAN.to_le_bytes());
        buf.extend_from_slice(&0.0_f32.to_le_bytes());
        assert!(matches!(
            parse_client_packet(&buf),
            Err(ProtocolError::InvalidValue(_))
        ));
    }

    #[test]
    fn parses_mouse_button_and_key() {
        assert_eq!(
            parse_client_packet(&[OP_MOUSE_BTN, 1, 2]).unwrap(),
            ClientPacket::MouseButton {
                button: MouseButton::Right,
                action: PressAction::Click
            }
        );

        let mut key_buf = vec![OP_KEY];
        key_buf.extend_from_slice(&0x001B_u16.to_le_bytes());
        key_buf.push(2);
        assert_eq!(
            parse_client_packet(&key_buf).unwrap(),
            ClientPacket::Key {
                vk: 0x1B,
                action: PressAction::Click
            }
        );
    }

    #[test]
    fn parses_utf8_text_including_emoji() {
        let msg = "Hello 👋 世界";
        let bytes = msg.as_bytes();
        let mut buf = vec![OP_TEXT];
        buf.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
        buf.extend_from_slice(bytes);
        assert_eq!(
            parse_client_packet(&buf).unwrap(),
            ClientPacket::Text(msg.to_string())
        );
    }

    #[test]
    fn encodes_pong_roundtrip() {
        let pong = encode_pong(1731799999123);
        assert_eq!(pong[0], OP_PONG);
        assert_eq!(u64::from_le_bytes(pong[1..9].try_into().unwrap()), 1731799999123);
    }
}
