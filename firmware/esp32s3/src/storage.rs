//! Settings storage on the ESP32-S3: the first sector of the `nvs` partition. We
//! don't use ESP-IDF's NVS, so that space is otherwise unused. Erasing the flash
//! (e.g. the web flasher's "erase" option) resets everything to defaults.

use esp_bootloader_esp_idf::partitions::{
    self, DataPartitionSubType, FlashStorage, PARTITION_TABLE_MAX_LEN, PartitionEntry,
    PartitionType,
};
use log::warn;
use nxcontroller_core::settings::{SettingsError, Storage};
use static_cell::StaticCell;

pub struct NvsStorage {
    flash: FlashStorage<'static>,
    nvs: PartitionEntry,
}

impl NvsStorage {
    /// None if the flash has no `nvs` partition (settings then can't be saved).
    #[allow(
        clippy::large_stack_frames,
        reason = "runs once at boot, on main's stack"
    )]
    pub fn new(flash: FlashStorage<'static>) -> Option<Self> {
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
        match nvs {
            Some(nvs) => Some(Self { flash, nvs }),
            None => {
                warn!("no nvs partition found: settings can't be saved");
                None
            }
        }
    }
}

impl Storage for NvsStorage {
    fn read(&mut self, record: &mut [u8]) -> Result<(), SettingsError> {
        self.nvs
            .as_flash_region(&mut self.flash)
            .read(0, record)
            .map_err(|_| SettingsError::Flash)
    }

    fn write(&mut self, record: &[u8]) -> Result<(), SettingsError> {
        let mut region = self.nvs.as_flash_region(&mut self.flash);
        region
            .erase(0, FlashStorage::SECTOR_SIZE)
            .map_err(|_| SettingsError::Flash)?;
        region.write(0, record).map_err(|_| SettingsError::Flash)
    }
}
