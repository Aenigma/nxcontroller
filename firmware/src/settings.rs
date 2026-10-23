//! Persisted settings (currently just the USB mode).
//!
//! Stored as a tiny record in the first sector of the `nvs` partition. We don't
//! use ESP-IDF's NVS, so that space is otherwise unused.

use esp_bootloader_esp_idf::partitions::{
    self, DataPartitionSubType, FlashStorage, PARTITION_TABLE_MAX_LEN, PartitionEntry,
    PartitionType,
};
use log::warn;
use static_cell::StaticCell;

const MAGIC: [u8; 4] = *b"NXCM";

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

pub struct Settings {
    flash: FlashStorage<'static>,
    nvs: Option<PartitionEntry>,
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
        if nvs.is_none() {
            warn!("no nvs partition found: mode changes can't be saved");
        }
        Self { flash, nvs }
    }

    pub fn mode(&mut self) -> Mode {
        let Some(nvs) = self.nvs else {
            return Mode::DEFAULT;
        };
        let mut record = [0u8; 8];
        if nvs
            .as_flash_region(&mut self.flash)
            .read(0, &mut record)
            .is_err()
            || record[..4] != MAGIC
        {
            return Mode::DEFAULT;
        }
        Mode::from_u8(record[4]).unwrap_or(Mode::DEFAULT)
    }

    pub fn set_mode(&mut self, mode: Mode) -> Result<(), partitions::Error> {
        let nvs = self.nvs.ok_or(partitions::Error::NotSupported)?;
        let mut region = nvs.as_flash_region(&mut self.flash);
        region.erase(0, FlashStorage::SECTOR_SIZE)?;
        let mut record = [0xFFu8; 8];
        record[..4].copy_from_slice(&MAGIC);
        record[4] = mode as u8;
        region.write(0, &record)
    }
}
