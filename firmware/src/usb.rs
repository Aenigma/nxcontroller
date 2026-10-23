//! USB HID side: a Switch-compatible wired gamepad (HORIPAD-style descriptor,
//! which the Switch accepts without any handshake) plus an optional boot keyboard.
//!
//! Network tasks push reports into the channels below; the USB tasks forward
//! them to the host in order, so short taps are never coalesced away.

use core::sync::atomic::{AtomicBool, Ordering};

use embassy_futures::join::join;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_usb::class::hid::{self, HidReaderWriter, HidSubclass, RequestHandler};
use embassy_usb::{Builder, Handler, UsbDevice, UsbVersion};
use esp_hal::usb::otg::embassy_usb_device::Driver;
use log::{info, warn};
use static_cell::StaticCell;

pub type UsbDriver = Driver<'static>;

pub const GAMEPAD_ENABLED: bool = cfg!(feature = "gamepad");
pub const KEYBOARD_ENABLED: bool = cfg!(feature = "keyboard");

#[cfg(not(any(feature = "gamepad", feature = "keyboard")))]
compile_error!("enable at least one of the `gamepad` / `keyboard` features");

struct Identity {
    vid: u16,
    pid: u16,
    manufacturer: &'static str,
    product: &'static str,
    release: u16,
}

// With a gamepad we present as a HORI Pokken Tournament controller: the Switch
// recognises this VID/PID as a wired Pro-Controller-compatible pad.
const GAMEPAD_IDENTITY: Identity = Identity {
    vid: 0x0F0D,
    pid: 0x0092,
    manufacturer: "HORI CO.,LTD.",
    product: "POKKEN CONTROLLER",
    release: 0x0572,
};

// Keyboard-only builds are a plain HID keyboard, which needs no special identity.
// 1209:0001 is the pid.codes test PID, fine for personal use.
const KEYBOARD_IDENTITY: Identity = Identity {
    vid: 0x1209,
    pid: 0x0001,
    manufacturer: "nxcontroller",
    product: "nxcontroller keyboard",
    release: 0x0100,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GamepadReport {
    pub buttons: u16,
    pub hat: u8,
    pub lx: u8,
    pub ly: u8,
    pub rx: u8,
    pub ry: u8,
}

impl GamepadReport {
    pub const NEUTRAL: Self = Self {
        buttons: 0,
        hat: 8,
        lx: 128,
        ly: 128,
        rx: 128,
        ry: 128,
    };

    fn to_bytes(self) -> [u8; 8] {
        let [b0, b1] = self.buttons.to_le_bytes();
        [b0, b1, self.hat, self.lx, self.ly, self.rx, self.ry, 0]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyboardReport {
    pub modifiers: u8,
    pub keys: [u8; 6],
}

impl KeyboardReport {
    pub const EMPTY: Self = Self {
        modifiers: 0,
        keys: [0; 6],
    };

    fn to_bytes(self) -> [u8; 8] {
        let k = self.keys;
        [self.modifiers, 0, k[0], k[1], k[2], k[3], k[4], k[5]]
    }
}

static GAMEPAD_REPORTS: Channel<CriticalSectionRawMutex, GamepadReport, 32> = Channel::new();
static KEYBOARD_REPORTS: Channel<CriticalSectionRawMutex, KeyboardReport, 64> = Channel::new();
static CONFIGURED: AtomicBool = AtomicBool::new(false);

/// True once the USB host (Switch/PC) has enumerated and configured us.
pub fn is_configured() -> bool {
    CONFIGURED.load(Ordering::Relaxed)
}

pub fn submit_gamepad(report: GamepadReport) {
    if GAMEPAD_REPORTS.try_send(report).is_err() {
        warn!("gamepad queue full, dropping report");
    }
}

pub fn submit_keyboard(report: KeyboardReport) {
    if KEYBOARD_REPORTS.try_send(report).is_err() {
        warn!("keyboard queue full, dropping report");
    }
}

/// Drop anything queued and return every input to neutral.
pub fn release_all() {
    if GAMEPAD_ENABLED {
        GAMEPAD_REPORTS.clear();
        submit_gamepad(GamepadReport::NEUTRAL);
    }
    if KEYBOARD_ENABLED {
        KEYBOARD_REPORTS.clear();
        submit_keyboard(KeyboardReport::EMPTY);
    }
}

#[rustfmt::skip]
const GAMEPAD_DESCRIPTOR: &[u8] = &[
    0x05, 0x01,       // Usage Page (Generic Desktop)
    0x09, 0x05,       // Usage (Game Pad)
    0xA1, 0x01,       // Collection (Application)
    0x15, 0x00,       //   Logical Minimum (0)
    0x25, 0x01,       //   Logical Maximum (1)
    0x35, 0x00,       //   Physical Minimum (0)
    0x45, 0x01,       //   Physical Maximum (1)
    0x75, 0x01,       //   Report Size (1)
    0x95, 0x10,       //   Report Count (16)
    0x05, 0x09,       //   Usage Page (Button)
    0x19, 0x01,       //   Usage Minimum (1)
    0x29, 0x10,       //   Usage Maximum (16)
    0x81, 0x02,       //   Input (Data,Var,Abs)
    0x05, 0x01,       //   Usage Page (Generic Desktop)
    0x25, 0x07,       //   Logical Maximum (7)
    0x46, 0x3B, 0x01, //   Physical Maximum (315)
    0x75, 0x04,       //   Report Size (4)
    0x95, 0x01,       //   Report Count (1)
    0x65, 0x14,       //   Unit (Eng Rot: Degrees)
    0x09, 0x39,       //   Usage (Hat switch)
    0x81, 0x42,       //   Input (Data,Var,Abs,Null State)
    0x65, 0x00,       //   Unit (None)
    0x95, 0x01,       //   Report Count (1)
    0x81, 0x01,       //   Input (Const) - 4 bit padding
    0x26, 0xFF, 0x00, //   Logical Maximum (255)
    0x46, 0xFF, 0x00, //   Physical Maximum (255)
    0x09, 0x30,       //   Usage (X)
    0x09, 0x31,       //   Usage (Y)
    0x09, 0x32,       //   Usage (Z)
    0x09, 0x35,       //   Usage (Rz)
    0x75, 0x08,       //   Report Size (8)
    0x95, 0x04,       //   Report Count (4)
    0x81, 0x02,       //   Input (Data,Var,Abs)
    0x06, 0x00, 0xFF, //   Usage Page (Vendor Defined 0xFF00)
    0x09, 0x20,       //   Usage (0x20)
    0x95, 0x01,       //   Report Count (1)
    0x81, 0x02,       //   Input (Data,Var,Abs)
    0x0A, 0x21, 0x26, //   Usage (0x2621)
    0x95, 0x08,       //   Report Count (8)
    0x91, 0x02,       //   Output (Data,Var,Abs)
    0xC0,             // End Collection
];

#[rustfmt::skip]
const KEYBOARD_DESCRIPTOR: &[u8] = &[
    0x05, 0x01, // Usage Page (Generic Desktop)
    0x09, 0x06, // Usage (Keyboard)
    0xA1, 0x01, // Collection (Application)
    0x05, 0x07, //   Usage Page (Keyboard/Keypad)
    0x19, 0xE0, //   Usage Minimum (Left Control)
    0x29, 0xE7, //   Usage Maximum (Right GUI)
    0x15, 0x00, //   Logical Minimum (0)
    0x25, 0x01, //   Logical Maximum (1)
    0x75, 0x01, //   Report Size (1)
    0x95, 0x08, //   Report Count (8)
    0x81, 0x02, //   Input (Data,Var,Abs) - modifiers
    0x95, 0x01, //   Report Count (1)
    0x75, 0x08, //   Report Size (8)
    0x81, 0x01, //   Input (Const) - reserved
    0x95, 0x05, //   Report Count (5)
    0x75, 0x01, //   Report Size (1)
    0x05, 0x08, //   Usage Page (LEDs)
    0x19, 0x01, //   Usage Minimum (Num Lock)
    0x29, 0x05, //   Usage Maximum (Kana)
    0x91, 0x02, //   Output (Data,Var,Abs) - LEDs
    0x95, 0x01, //   Report Count (1)
    0x75, 0x03, //   Report Size (3)
    0x91, 0x01, //   Output (Const) - padding
    0x95, 0x06, //   Report Count (6)
    0x75, 0x08, //   Report Size (8)
    0x15, 0x00, //   Logical Minimum (0)
    0x25, 0xFF, //   Logical Maximum (255)
    0x05, 0x07, //   Usage Page (Keyboard/Keypad)
    0x19, 0x00, //   Usage Minimum (0)
    0x29, 0xFF, //   Usage Maximum (255)
    0x81, 0x00, //   Input (Data,Array) - keys
    0xC0,       // End Collection
];

struct DeviceState;

impl Handler for DeviceState {
    fn configured(&mut self, configured: bool) {
        CONFIGURED.store(configured, Ordering::Relaxed);
        info!(
            "USB {}",
            if configured {
                "configured by host"
            } else {
                "unconfigured"
            }
        );
    }

    fn reset(&mut self) {
        CONFIGURED.store(false, Ordering::Relaxed);
    }
}

struct IgnoreRequests;
impl RequestHandler for IgnoreRequests {}

pub struct UsbParts {
    pub device: UsbDevice<'static, UsbDriver>,
    pub gamepad: Option<HidReaderWriter<'static, UsbDriver, 8, 8>>,
    pub keyboard: Option<HidReaderWriter<'static, UsbDriver, 1, 8>>,
}

pub fn build(driver: UsbDriver) -> UsbParts {
    let id = if GAMEPAD_ENABLED {
        GAMEPAD_IDENTITY
    } else {
        KEYBOARD_IDENTITY
    };
    let mut config = embassy_usb::Config::new(id.vid, id.pid);
    config.manufacturer = Some(id.manufacturer);
    config.product = Some(id.product);
    config.serial_number = None;
    config.bcd_usb = UsbVersion::Two;
    config.device_release = id.release;
    config.max_power = 500;
    // Plain HID composite: no IADs, class defined per interface.
    config.device_class = 0;
    config.device_sub_class = 0;
    config.device_protocol = 0;
    config.composite_with_iads = false;

    static CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESC: StaticCell<[u8; 64]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();
    static DEVICE_STATE: StaticCell<DeviceState> = StaticCell::new();
    static GAMEPAD_STATE: StaticCell<hid::State> = StaticCell::new();
    static KEYBOARD_STATE: StaticCell<hid::State> = StaticCell::new();

    let mut builder = Builder::new(
        driver,
        config,
        CONFIG_DESC.init([0; 256]),
        BOS_DESC.init([0; 64]),
        &mut [],
        CONTROL_BUF.init([0; 64]),
    );
    builder.handler(DEVICE_STATE.init(DeviceState));

    let gamepad = GAMEPAD_ENABLED.then(|| {
        HidReaderWriter::new(
            &mut builder,
            GAMEPAD_STATE.init(hid::State::new()),
            hid::Config {
                report_descriptor: GAMEPAD_DESCRIPTOR,
                request_handler: None,
                poll_ms: 5,
                max_packet_size: 64,
                hid_subclass: HidSubclass::No,
                hid_boot_protocol: hid::HidBootProtocol::None,
            },
        )
    });

    let keyboard = KEYBOARD_ENABLED.then(|| {
        HidReaderWriter::new(
            &mut builder,
            KEYBOARD_STATE.init(hid::State::new()),
            hid::Config {
                report_descriptor: KEYBOARD_DESCRIPTOR,
                request_handler: None,
                poll_ms: 5,
                max_packet_size: 8,
                hid_subclass: HidSubclass::Boot,
                hid_boot_protocol: hid::HidBootProtocol::Keyboard,
            },
        )
    });

    UsbParts {
        device: builder.build(),
        gamepad,
        keyboard,
    }
}

#[embassy_executor::task]
pub async fn usb_device_task(mut device: UsbDevice<'static, UsbDriver>) -> ! {
    device.run().await
}

#[embassy_executor::task]
pub async fn gamepad_task(hid: HidReaderWriter<'static, UsbDriver, 8, 8>) {
    let (reader, mut writer) = hid.split();
    let forward = async {
        loop {
            let report = GAMEPAD_REPORTS.receive().await;
            if !is_configured() {
                continue; // nothing plugged in; don't queue stale input
            }
            if let Err(e) = writer.write(&report.to_bytes()).await {
                warn!("gamepad write failed: {e:?}");
            }
        }
    };
    join(reader.run(false, &mut IgnoreRequests), forward).await;
}

#[embassy_executor::task]
pub async fn keyboard_task(hid: HidReaderWriter<'static, UsbDriver, 1, 8>) {
    let (reader, mut writer) = hid.split();
    let forward = async {
        loop {
            let report = KEYBOARD_REPORTS.receive().await;
            if !is_configured() {
                continue;
            }
            if let Err(e) = writer.write(&report.to_bytes()).await {
                warn!("keyboard write failed: {e:?}");
            }
        }
    };
    join(reader.run(false, &mut IgnoreRequests), forward).await;
}
