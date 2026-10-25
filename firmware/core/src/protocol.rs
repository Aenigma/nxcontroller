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
    pub const SET_MODE: u8 = 0x03;
    pub const SET_WIFI: u8 = 0x04;
    pub const SET_OPTION: u8 = 0x05;
    pub const LOGS: u8 = 0x06;
    pub const GAMEPAD: u8 = 0x10;
    pub const KEYBOARD: u8 = 0x11;
    pub const TYPE: u8 = 0x12;
    pub const RELEASE_ALL: u8 = 0x1F;
    // Device -> client
    pub const HELLO_ACK: u8 = 0x81;
    pub const PONG: u8 = 0x82;
    pub const MODE_ACK: u8 = 0x83;
    pub const WIFI_ACK: u8 = 0x84;
    pub const TYPE_PROGRESS: u8 = 0x85;
    pub const OPTION_ACK: u8 = 0x86;
    pub const LOG: u8 = 0x87;
    pub const ERROR: u8 = 0x8F;
}

pub mod feature {
    pub const GAMEPAD: u8 = 0x01;
    pub const KEYBOARD: u8 = 0x02;
    /// The device paces TYPE keystrokes itself.
    pub const PACED_TYPING: u8 = 0x80;
    /// LOGS works: the device keeps its recent log output.
    pub const LOGS: u8 = 0x20;
    /// Built with WiFi: SET_WIFI works and the board can join a network.
    pub const WIFI: u8 = 0x40;
}

/// SET_OPTION ids.
pub mod option {
    /// USB keyboard polling interval in ms (1..=255; 0 = default). Applies after a restart.
    pub const KEYBOARD_POLL_MS: u8 = 1;
}

/// TYPE flags.
pub mod type_flags {
    /// Go straight from one key to the next without a release report in
    /// between, when the modifiers match and the key differs.
    pub const ROLLOVER: u8 = 0x01;
}

pub mod error {
    pub const UNKNOWN_TYPE: u8 = 1;
    pub const BAD_LENGTH: u8 = 2;
    pub const UNSUPPORTED: u8 = 3;
    pub const VERSION_MISMATCH: u8 = 4;
    pub const INVALID_VALUE: u8 = 5;
    pub const STORAGE: u8 = 6;
    pub const BUSY: u8 = 7;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command<'a> {
    Hello {
        version: u8,
    },
    Ping {
        seq: u32,
    },
    SetMode {
        mode: u8,
    },
    SetWifi {
        ssid: &'a str,
        password: &'a str,
    },
    /// Keystrokes for the device to type at its own pace: `keys` holds
    /// `(modifiers, key)` pairs.
    Type {
        flags: u8,
        press_ms: u8,
        release_ms: u8,
        keys: &'a [u8],
    },
    SetOption {
        id: u8,
        value: u8,
    },
    Logs {
        follow: bool,
    },
    Gamepad(GamepadReport),
    Keyboard(KeyboardReport),
    ReleaseAll,
}

/// Decode a client message. On failure returns the protocol error code.
pub fn parse(msg_type: u8, payload: &[u8]) -> Result<Command<'_>, u8> {
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
        msg::SET_MODE => {
            expect(1)?;
            Ok(Command::SetMode { mode: payload[0] })
        }
        msg::SET_WIFI => {
            // ssid_len | ssid | password (rest of the payload)
            let ssid_len = *payload.first().ok_or(error::BAD_LENGTH)? as usize;
            let ssid = payload.get(1..1 + ssid_len).ok_or(error::BAD_LENGTH)?;
            let password = &payload[1 + ssid_len..];
            let as_str = |b| core::str::from_utf8(b).map_err(|_| error::INVALID_VALUE);
            Ok(Command::SetWifi {
                ssid: as_str(ssid)?,
                password: as_str(password)?,
            })
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
        msg::TYPE => {
            // flags | press_ms | release_ms | (modifiers, key) pairs
            if payload.len() < 3 || payload.len().is_multiple_of(2) {
                return Err(error::BAD_LENGTH);
            }
            Ok(Command::Type {
                flags: payload[0],
                press_ms: payload[1],
                release_ms: payload[2],
                keys: &payload[3..],
            })
        }
        msg::LOGS => {
            expect(1)?;
            Ok(Command::Logs {
                follow: payload[0] != 0,
            })
        }
        msg::SET_OPTION => {
            expect(2)?;
            Ok(Command::SetOption {
                id: payload[0],
                value: payload[1],
            })
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
