#![no_std]
//! Everything that doesn't depend on the chip: the control protocol, USB HID
//! (generic over the USB driver), the BLE GATT service (generic over the HCI
//! transport), the TCP server (any embassy-net stack), settings, logging.
//! Each board crate brings up its peripherals and calls into this.

#[cfg(not(any(feature = "ble", feature = "wifi")))]
compile_error!("enable `ble`, `wifi` or both: otherwise nothing can control the board");

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

use core::cell::Cell;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Timer};

/// Settings shared between the control transports (and the WiFi setup page).
pub type SharedSettings = &'static Mutex<CriticalSectionRawMutex, settings::Settings>;

/// What the core needs from the board it runs on.
#[derive(Clone, Copy)]
pub struct Board {
    /// Reboots the chip.
    pub reset: fn() -> !,
    /// Prints one log line on a serial console, if the board has one.
    pub console: Option<fn(core::fmt::Arguments<'_>)>,
}

static BOARD: critical_section::Mutex<Cell<Option<Board>>> =
    critical_section::Mutex::new(Cell::new(None));

/// Call once, first thing at boot: installs the logger.
pub fn init(board: Board) {
    critical_section::with(|cs| BOARD.borrow(cs).set(Some(board)));
    logbuf::init();
}

fn board() -> Option<Board> {
    critical_section::with(|cs| BOARD.borrow(cs).get())
}

/// Give in-flight replies a moment to leave, then reboot.
pub async fn restart() -> ! {
    Timer::after(Duration::from_millis(500)).await;
    match board() {
        Some(board) => (board.reset)(),
        None => panic!("nxcontroller_core::init was never called"),
    }
}
