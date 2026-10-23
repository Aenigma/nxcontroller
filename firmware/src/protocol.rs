//! Wire protocol. Mirrors `protocol/PROTOCOL.md`; keep in sync with
//! `python/src/nxcontroller/protocol.py`.

use crate::usb::{GamepadReport, KeyboardReport};

pub const PROTOCOL_VERSION: u8 = 1;
pub const TCP_PORT: u16 = 7700;
pub const DISCOVERY_PORT: u16 = 7701;

pub const DISCOVER_REQUEST: &[u8] = b"NXC?";
pub const DISCOVER_REPLY_MAGIC: &[u8] = b"NXC!";

pub mod msg {
    // Client -> device
    pub const HELLO: u8 = 0x01;
    pub const PING: u8 = 0x02;
    pub const GAMEPAD: u8 = 0x10;
    pub const KEYBOARD: u8 = 0x11;
    pub const RELEASE_ALL: u8 = 0x1F;
    // Device -> client
    pub const HELLO_ACK: u8 = 0x81;
    pub const PONG: u8 = 0x82;
    pub const ERROR: u8 = 0x8F;
}

pub mod feature {
    pub const GAMEPAD: u8 = 0x01;
    pub const KEYBOARD: u8 = 0x02;
}

pub mod error {
    pub const UNKNOWN_TYPE: u8 = 1;
    pub const BAD_LENGTH: u8 = 2;
    pub const UNSUPPORTED: u8 = 3;
    pub const VERSION_MISMATCH: u8 = 4;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Hello { version: u8 },
    Ping { seq: u32 },
    Gamepad(GamepadReport),
    Keyboard(KeyboardReport),
    ReleaseAll,
}

/// Decode a client message. On failure returns the protocol error code.
pub fn parse(msg_type: u8, payload: &[u8]) -> Result<Command, u8> {
    let expect = |len: usize| {
        if payload.len() == len {
            Ok(())
        } else {
            Err(error::BAD_LENGTH)
        }
    };
    match msg_type {
        msg::HELLO => {
            expect(1)?;
            Ok(Command::Hello {
                version: payload[0],
            })
        }
        msg::PING => {
            expect(4)?;
            let seq = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
            Ok(Command::Ping { seq })
        }
        msg::GAMEPAD => {
            expect(7)?;
            Ok(Command::Gamepad(GamepadReport {
                buttons: u16::from_le_bytes([payload[0], payload[1]]),
                hat: payload[2],
                lx: payload[3],
                ly: payload[4],
                rx: payload[5],
                ry: payload[6],
            }))
        }
        msg::KEYBOARD => {
            expect(7)?;
            let mut keys = [0u8; 6];
            keys.copy_from_slice(&payload[1..7]);
            Ok(Command::Keyboard(KeyboardReport {
                modifiers: payload[0],
                keys,
            }))
        }
        msg::RELEASE_ALL => {
            expect(0)?;
            Ok(Command::ReleaseAll)
        }
        _ => Err(error::UNKNOWN_TYPE),
    }
}

/// Write `[type, len, payload..]` into `buf` and return the used slice.
pub fn frame<'a>(buf: &'a mut [u8], msg_type: u8, payload: &[u8]) -> &'a [u8] {
    let len = payload.len().min(255).min(buf.len() - 2);
    buf[0] = msg_type;
    buf[1] = len as u8;
    buf[2..2 + len].copy_from_slice(&payload[..len]);
    &buf[..2 + len]
}
