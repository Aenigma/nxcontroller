//! Persisted settings: USB mode and WiFi credentials.
//!
//! Stored as one small record in the first sector of the `nvs` partition. We
//! don't use ESP-IDF's NVS, so that space is otherwise unused. Erasing the flash
//! (e.g. the web flasher's "erase" option) resets everything to defaults.

use esp_bootloader_esp_idf::partitions::{
    self, DataPartitionSubType, FlashStorage, PARTITION_TABLE_MAX_LEN, PartitionEntry,
    PartitionType,
};
use log::warn;
use static_cell::StaticCell;

const MAGIC: [u8; 4] = *b"NXCM";

pub const MAX_SSID_LEN: usize = 32;
pub const MAX_PASSWORD_LEN: usize = 64;

// Record layout: magic[4] | mode | ssid_len | password_len | reserved | ssid[32] | password[64]
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
    /// Flash has no `nvs` partition, so nothing can be saved.
    NoStorage,
    TooLong,
    Flash(partitions::Error),
}

impl From<partitions::Error> for SettingsError {
    fn from(e: partitions::Error) -> Self {
        SettingsError::Flash(e)
    }
}

pub struct Settings {
    flash: FlashStorage<'static>,
    nvs: Option<PartitionEntry>,
    record: [u8; RECORD_LEN],
}

impl Settings {
    pub fn new(flash: FlashStorage<'static>) -> Self {
        // Park the second core during writes so it can't fetch from flash meanwhile.
        let mut flash = flash.multicore_auto_park();
        static TABLE: StaticCell<[u8; PARTITION_TABLE_MAX_LEN]> = StaticCell::new();
        let nvs =
            partitions::read_partition_table(&mut flash, TABLE.init([0; PARTITION_TABLE_MAX_LEN]))
                .ok()
                .and_then(|table| {
                    table
                        .find_partition(PartitionType::Data(DataPartitionSubType::Nvs))
                        .ok()
                        .flatten()
                });

        let mut record = [0xFFu8; RECORD_LEN];
        match nvs {
            Some(nvs) => {
                if nvs
                    .as_flash_region(&mut flash)
                    .read(0, &mut record)
                    .is_err()
                    || record[..4] != MAGIC
                {
                    record = [0xFF; RECORD_LEN]; // blank or foreign: use defaults
                }
            }
            None => warn!("no nvs partition found: settings can't be saved"),
        }
        Self { flash, nvs, record }
    }

    pub fn mode(&self) -> Mode {
        Mode::from_u8(self.record[4]).unwrap_or(Mode::DEFAULT)
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
        let nvs = self.nvs.ok_or(SettingsError::NoStorage)?;
        self.record[..4].copy_from_slice(&MAGIC);
        let mut region = nvs.as_flash_region(&mut self.flash);
        region.erase(0, FlashStorage::SECTOR_SIZE)?;
        region.write(0, &self.record)?;
        Ok(())
    }
}
