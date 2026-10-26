//! Persisted settings: USB mode and WiFi credentials.
//!
//! Stored as one small record in one flash sector that each board sets aside
//! (see [`SectorStorage`]). Erasing the flash resets everything to defaults.

use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};

const MAGIC: [u8; 4] = *b"NXCM";

pub const MAX_SSID_LEN: usize = 32;
pub const MAX_PASSWORD_LEN: usize = 64;

// Record layout: magic[4] | mode | ssid_len | password_len | keyboard_poll_ms | ssid[32] | password[64]
const KEYBOARD_POLL_AT: usize = 7;
/// USB polling interval for the keyboard when none is saved.
pub const DEFAULT_KEYBOARD_POLL_MS: u8 = 5;
const SSID_AT: usize = 8;
const PASSWORD_AT: usize = SSID_AT + MAX_SSID_LEN;
const RECORD_LEN: usize = PASSWORD_AT + MAX_PASSWORD_LEN;

/// What the device presents on USB. The value doubles as the protocol's
/// feature bitmask (bit 0 gamepad, bit 1 keyboard).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    Gamepad = 1,
    Keyboard = 2,
    Both = 3,
}

impl Mode {
    pub const DEFAULT: Self = Mode::Both;

    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Mode::Gamepad),
            2 => Some(Mode::Keyboard),
            3 => Some(Mode::Both),
            _ => None,
        }
    }

    pub fn has_gamepad(self) -> bool {
        self as u8 & 1 != 0
    }

    pub fn has_keyboard(self) -> bool {
        self as u8 & 2 != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsError {
    /// The board has nowhere to save settings.
    NoStorage,
    TooLong,
    /// Reading, erasing or writing the flash failed.
    Flash,
}

/// Where a board keeps the settings record: the start of a flash sector that
/// nothing else uses.
pub trait Storage: Send {
    /// Reads the start of the sector into `record`.
    fn read(&mut self, record: &mut [u8]) -> Result<(), SettingsError>;
    /// Erases the sector, then writes `record` at its start.
    fn write(&mut self, record: &[u8]) -> Result<(), SettingsError>;
}

/// [`Storage`] on one erase sector of a NOR flash, starting at `offset`.
pub struct SectorStorage<F> {
    flash: F,
    offset: u32,
}

impl<F: NorFlash> SectorStorage<F> {
    pub fn new(flash: F, offset: u32) -> Self {
        Self { flash, offset }
    }
}

impl<F: NorFlash + Send> Storage for SectorStorage<F> {
    fn read(&mut self, record: &mut [u8]) -> Result<(), SettingsError> {
        ReadNorFlash::read(&mut self.flash, self.offset, record).map_err(|_| SettingsError::Flash)
    }

    fn write(&mut self, record: &[u8]) -> Result<(), SettingsError> {
        let end = self.offset + F::ERASE_SIZE as u32;
        self.flash
            .erase(self.offset, end)
            .map_err(|_| SettingsError::Flash)?;
        NorFlash::write(&mut self.flash, self.offset, record).map_err(|_| SettingsError::Flash)
    }
}

pub struct Settings {
    storage: Option<&'static mut dyn Storage>,
    record: [u8; RECORD_LEN],
}

impl Settings {
    /// Loads the saved settings. Without `storage`, defaults apply and nothing can be saved.
    pub fn new(mut storage: Option<&'static mut dyn Storage>) -> Self {
        let mut record = [0xFFu8; RECORD_LEN];
        if let Some(storage) = storage.as_deref_mut()
            && (storage.read(&mut record).is_err() || record[..4] != MAGIC)
        {
            record = [0xFF; RECORD_LEN]; // blank or foreign: use defaults
        }
        Self { storage, record }
    }

    pub fn mode(&self) -> Mode {
        Mode::from_u8(self.record[4]).unwrap_or(Mode::DEFAULT)
    }

    /// How often (ms) the USB host should poll the keyboard. Lower is faster typing.
    pub fn keyboard_poll_ms(&self) -> u8 {
        match self.record[KEYBOARD_POLL_AT] {
            0 | 0xFF => DEFAULT_KEYBOARD_POLL_MS, // unset
            ms => ms,
        }
    }

    pub fn set_keyboard_poll_ms(&mut self, ms: u8) -> Result<(), SettingsError> {
        self.record[KEYBOARD_POLL_AT] = ms;
        self.save()
    }

    pub fn set_mode(&mut self, mode: Mode) -> Result<(), SettingsError> {
        self.record[4] = mode as u8;
        self.save()
    }

    /// Saved WiFi credentials, if any.
    pub fn wifi(&self) -> Option<(&str, &str)> {
        let ssid_len = self.record[5] as usize;
        let password_len = self.record[6] as usize;
        if ssid_len == 0 || ssid_len > MAX_SSID_LEN || password_len > MAX_PASSWORD_LEN {
            return None;
        }
        let ssid = core::str::from_utf8(&self.record[SSID_AT..SSID_AT + ssid_len]).ok()?;
        let password =
            core::str::from_utf8(&self.record[PASSWORD_AT..PASSWORD_AT + password_len]).ok()?;
        Some((ssid, password))
    }

    pub fn set_wifi(&mut self, ssid: &str, password: &str) -> Result<(), SettingsError> {
        if ssid.is_empty() || ssid.len() > MAX_SSID_LEN || password.len() > MAX_PASSWORD_LEN {
            return Err(SettingsError::TooLong);
        }
        self.record[5] = ssid.len() as u8;
        self.record[6] = password.len() as u8;
        self.record[SSID_AT..PASSWORD_AT].fill(0);
        self.record[SSID_AT..SSID_AT + ssid.len()].copy_from_slice(ssid.as_bytes());
        self.record[PASSWORD_AT..].fill(0);
        self.record[PASSWORD_AT..PASSWORD_AT + password.len()].copy_from_slice(password.as_bytes());
        self.save()
    }

    fn save(&mut self) -> Result<(), SettingsError> {
        let storage = self
            .storage
            .as_deref_mut()
            .ok_or(SettingsError::NoStorage)?;
        self.record[..4].copy_from_slice(&MAGIC);
        storage.write(&self.record)
    }
}
