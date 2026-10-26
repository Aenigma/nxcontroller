//! Settings storage on the ESP32-S3: the first sector of the `nvs` partition. We
//! don't use ESP-IDF's NVS, so that space is otherwise unused. Erasing the flash
//! (e.g. the web flasher's "erase" option) resets everything to defaults.

use esp_bootloader_esp_idf::partitions::{
    self, DataPartitionSubType, FlashStorage, PARTITION_TABLE_MAX_LEN, PartitionType,
};
use log::warn;
use nxcontroller_core::settings::SectorStorage;
use static_cell::StaticCell;

/// None if the flash has no `nvs` partition (settings then can't be saved).
#[allow(
    clippy::large_stack_frames,
    reason = "runs once at boot, on main's stack"
)]
pub fn nvs(flash: FlashStorage<'static>) -> Option<SectorStorage<FlashStorage<'static>>> {
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
        Some(nvs) => Some(SectorStorage::new(flash, nvs.offset())),
        None => {
            warn!("no nvs partition found: settings can't be saved");
            None
        }
    }
}
