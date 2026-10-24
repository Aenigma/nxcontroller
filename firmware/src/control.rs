//! Control message handling shared by every transport (TCP over WiFi, BLE).
//!
//! A transport reassembles `[type][len][payload]` frames, hands each one to
//! [`handle`], sends back the reply if there is one, and restarts the board when
//! asked to. While idle it also forwards typing progress (see [`progress`]).

use log::{info, warn};

use crate::config::DEVICE_NAME;
use crate::protocol::{self, Command, error, feature, msg, option};
use crate::settings::Mode;
use crate::{SharedSettings, usb};

/// Buffer size for a reply frame.
pub const REPLY_MAX: usize = 64;

/// Handles one client frame. Returns the reply to send, if any. Sets `restart`
/// when the board must reboot after the reply has gone out (settings changed).
pub async fn handle<'a>(
    msg_type: u8,
    payload: &[u8],
    settings: SharedSettings,
    out: &'a mut [u8],
    restart: &mut bool,
) -> Option<&'a [u8]> {
    match protocol::parse(msg_type, payload) {
        Ok(Command::Hello { version }) if version == protocol::PROTOCOL_VERSION => {
            let mut body = [0u8; 34];
            // Mode values are the gamepad/keyboard feature bits.
            let features = usb::mode() as u8 | feature::PACED_TYPING;
            let name = &DEVICE_NAME.as_bytes()[..DEVICE_NAME.len().min(32)];
            body[0] = protocol::PROTOCOL_VERSION;
            body[1] = features;
            body[2..2 + name.len()].copy_from_slice(name);
            Some(protocol::frame(
                out,
                msg::HELLO_ACK,
                &body[..2 + name.len()],
            ))
        }
        Ok(Command::Hello { .. }) => {
            Some(protocol::frame(out, msg::ERROR, &[error::VERSION_MISMATCH]))
        }
        Ok(Command::Ping { seq }) => {
            let s = seq.to_le_bytes();
            let body = [s[0], s[1], s[2], s[3], usb::is_configured() as u8];
            Some(protocol::frame(out, msg::PONG, &body))
        }
        Ok(Command::SetMode { mode }) => match Mode::from_u8(mode) {
            None => Some(protocol::frame(out, msg::ERROR, &[error::INVALID_VALUE])),
            Some(new) if new == usb::mode() => {
                Some(protocol::frame(out, msg::MODE_ACK, &[mode, 0]))
            }
            Some(new) => match settings.lock().await.set_mode(new) {
                Ok(()) => {
                    info!("mode set to {new:?}");
                    *restart = true;
                    Some(protocol::frame(out, msg::MODE_ACK, &[mode, 1]))
                }
                Err(e) => {
                    warn!("saving mode failed: {e:?}");
                    Some(protocol::frame(out, msg::ERROR, &[error::STORAGE]))
                }
            },
        },
        Ok(Command::SetWifi { ssid, password }) => {
            match settings.lock().await.set_wifi(ssid, password) {
                Ok(()) => {
                    info!("WiFi credentials updated (network '{ssid}')");
                    *restart = true;
                    Some(protocol::frame(out, msg::WIFI_ACK, &[]))
                }
                Err(crate::settings::SettingsError::TooLong) => {
                    Some(protocol::frame(out, msg::ERROR, &[error::INVALID_VALUE]))
                }
                Err(e) => {
                    warn!("saving WiFi credentials failed: {e:?}");
                    Some(protocol::frame(out, msg::ERROR, &[error::STORAGE]))
                }
            }
        }
        Ok(Command::SetOption {
            id: option::KEYBOARD_POLL_MS,
            value,
        }) => {
            let mut settings = settings.lock().await;
            let ms = if value == 0 {
                crate::settings::DEFAULT_KEYBOARD_POLL_MS
            } else {
                value
            };
            if ms == settings.keyboard_poll_ms() {
                Some(protocol::frame(
                    out,
                    msg::OPTION_ACK,
                    &[option::KEYBOARD_POLL_MS, ms, 0],
                ))
            } else {
                match settings.set_keyboard_poll_ms(ms) {
                    Ok(()) => {
                        info!("keyboard poll interval set to {ms} ms");
                        *restart = true;
                        Some(protocol::frame(
                            out,
                            msg::OPTION_ACK,
                            &[option::KEYBOARD_POLL_MS, ms, 1],
                        ))
                    }
                    Err(e) => {
                        warn!("saving option failed: {e:?}");
                        Some(protocol::frame(out, msg::ERROR, &[error::STORAGE]))
                    }
                }
            }
        }
        Ok(Command::SetOption { .. }) => {
            Some(protocol::frame(out, msg::ERROR, &[error::INVALID_VALUE]))
        }
        Ok(Command::Gamepad(report)) if usb::mode().has_gamepad() => {
            usb::submit_gamepad(report);
            None
        }
        Ok(Command::Keyboard(report)) if usb::mode().has_keyboard() => {
            usb::submit_keyboard(report);
            None
        }
        Ok(Command::Type {
            flags,
            press_ms,
            release_ms,
            keys,
        }) if usb::mode().has_keyboard() => {
            if usb::queue_taps(flags, press_ms, release_ms, keys) {
                None
            } else {
                warn!("keystroke queue full, rejecting {} keys", keys.len() / 2);
                Some(protocol::frame(out, msg::ERROR, &[error::BUSY]))
            }
        }
        Ok(Command::Gamepad(_)) | Ok(Command::Keyboard(_)) | Ok(Command::Type { .. }) => {
            Some(protocol::frame(out, msg::ERROR, &[error::UNSUPPORTED]))
        }
        Ok(Command::ReleaseAll) => {
            usb::release_all();
            None
        }
        Err(code) => Some(protocol::frame(out, msg::ERROR, &[code])),
    }
}

/// Waits until keystrokes have been typed, then returns the TYPE_PROGRESS frame
/// to send. Transports race this against reading the next client frame, since the
/// client holds back further TYPE messages until it hears about progress.
pub async fn progress(out: &mut [u8]) -> Option<&[u8]> {
    let count = usb::typed().await;
    if count == 0 {
        return None;
    }
    let count = count.min(u32::from(u16::MAX)) as u16;
    Some(protocol::frame(
        out,
        msg::TYPE_PROGRESS,
        &count.to_le_bytes(),
    ))
}
