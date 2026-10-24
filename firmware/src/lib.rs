#![no_std]

#[cfg(not(any(feature = "ble", feature = "wifi")))]
compile_error!("enable `ble`, `wifi` or both: otherwise nothing can control the board");
#[cfg(all(feature = "ble", feature = "wifi", not(feature = "coex")))]
compile_error!("`ble` and `wifi` together need `coex`: they share the radio");
#[cfg(all(feature = "coex", not(all(feature = "ble", feature = "wifi"))))]
compile_error!("`coex` is only for builds with both `ble` and `wifi`");

#[cfg(feature = "ble")]
pub mod ble;
pub mod config;
pub mod control;
pub mod logbuf;
#[cfg(feature = "wifi")]
pub mod net;
pub mod protocol;
pub mod settings;
#[cfg(feature = "wifi")]
pub mod setup;
pub mod usb;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Timer};

/// Settings shared between the control transports (and the WiFi setup page).
pub type SharedSettings = &'static Mutex<CriticalSectionRawMutex, settings::Settings>;

/// Give in-flight replies a moment to leave, then reboot.
pub async fn restart() -> ! {
    Timer::after(Duration::from_millis(500)).await;
    esp_hal::system::software_reset()
}
