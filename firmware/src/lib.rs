#![no_std]

#[cfg(feature = "ble")]
pub mod ble;
pub mod config;
pub mod net;
pub mod protocol;
pub mod settings;
pub mod setup;
pub mod usb;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Timer};

/// Settings shared between the control server and the setup page.
pub type SharedSettings = &'static Mutex<CriticalSectionRawMutex, settings::Settings>;

/// Give in-flight replies a moment to leave, then reboot.
pub async fn restart() -> ! {
    Timer::after(Duration::from_millis(500)).await;
    esp_hal::system::software_reset()
}
