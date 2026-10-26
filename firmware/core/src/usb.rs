//! USB HID side: a Switch-compatible wired gamepad (HORIPAD-style descriptor,
//! which the Switch accepts without any handshake) and/or a boot keyboard,
//! depending on the runtime [`Mode`].
//!
//! Network tasks push reports into the channels below; the USB tasks forward
//! them to the host in order, so short taps are never coalesced away.

use portable_atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use embassy_futures::join::join;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Timer, with_timeout};
use embassy_usb::class::hid::HidSubclass;
use embassy_usb::class::hid::{
    self, HidProtocolMode, HidReaderWriter, HidWriter, ReportId, RequestHandler,
};
use embassy_usb::control::OutResponse;
use embassy_usb::driver::Driver;
use embassy_usb::{Builder, Handler, UsbDevice, UsbVersion};
use log::{info, warn};
use static_cell::StaticCell;

use crate::settings::{Mode, Settings};

static MODE: AtomicU8 = AtomicU8::new(Mode::DEFAULT as u8);

/// The mode the USB device was built with at boot.
pub fn mode() -> Mode {
    Mode::from_u8(MODE.load(Ordering::Relaxed)).unwrap_or(Mode::DEFAULT)
}

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

// Keyboard-only mode is a plain HID keyboard, which needs no special identity.
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
/// Everything the keyboard does, in order: raw states from KEYBOARD messages and
/// keystrokes from TYPE messages share one queue.
#[derive(Debug, Clone, Copy)]
enum KeyCmd {
    State(KeyboardReport),
    Tap {
        modifiers: u8,
        key: u8,
        press_ms: u8,
        release_ms: u8,
        rollover: bool,
    },
}

/// Room for well over the client's typing window (see TYPE in PROTOCOL.md).
const KEYBOARD_QUEUE: usize = 512;
static KEYBOARD_CMDS: Channel<CriticalSectionRawMutex, KeyCmd, KEYBOARD_QUEUE> = Channel::new();
/// Keystrokes finished since the control server last reported progress.
static TYPED: AtomicU32 = AtomicU32::new(0);
static TYPED_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// The host's keyboard LED state (its last SET_REPORT), and a wake-up for changes.
/// Caps Lock lives in the host; this is how we learn whether it's on.
static LEDS: AtomicU8 = AtomicU8::new(0);
static LEDS_CHANGED: Signal<CriticalSectionRawMutex, ()> = Signal::new();
const LED_CAPS_LOCK: u8 = 0x02;
const KEY_CAPS_LOCK: u8 = 0x39;
/// Longest we hold Caps Lock waiting for the host to confirm it's off. macOS
/// ignores very short Caps Lock presses, so we hold until the LED report says so.
const CAPS_LOCK_MAX_HOLD_MS: u64 = 400;
static CONFIGURED: AtomicBool = AtomicBool::new(false);

/// How long the gamepad task waits for new input before resending the current state.
/// The host's own polling (`poll_ms`) paces the actual writes.
const REPEAT_MS: u64 = 4;

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
    if KEYBOARD_CMDS.try_send(KeyCmd::State(report)).is_err() {
        warn!("keyboard queue full, dropping report");
    }
}

/// Drop anything queued and return every input to neutral.
pub fn release_all() {
    if mode().has_gamepad() {
        GAMEPAD_REPORTS.clear();
        submit_gamepad(GamepadReport::NEUTRAL);
    }
    if mode().has_keyboard() {
        KEYBOARD_CMDS.clear();
        TYPED.store(0, Ordering::Relaxed);
        TYPED_SIGNAL.reset();
        submit_keyboard(KeyboardReport::EMPTY);
    }
}

/// Queue keystrokes (`(modifiers, key)` pairs) for the keyboard task to type at
/// the pace the host reads them. All or nothing: returns false, queueing none,
/// if they don't fit.
pub fn queue_taps(flags: u8, press_ms: u8, release_ms: u8, keys: &[u8]) -> bool {
    let rollover = flags & crate::protocol::type_flags::ROLLOVER != 0;
    // The control server is the only producer, so the space can't shrink under us.
    if KEYBOARD_CMDS.free_capacity() < keys.len() / 2 {
        return false;
    }
    for &[modifiers, key] in keys.as_chunks::<2>().0 {
        let _ = KEYBOARD_CMDS.try_send(KeyCmd::Tap {
            modifiers,
            key,
            press_ms,
            release_ms,
            rollover,
        });
    }
    true
}

/// Wait until keystrokes have been typed, then return how many since last time.
pub async fn typed() -> u32 {
    TYPED_SIGNAL.wait().await;
    TYPED.swap(0, Ordering::Relaxed)
}

fn mark_typed() {
    TYPED.fetch_add(1, Ordering::Relaxed);
    TYPED_SIGNAL.signal(());
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
        LEDS.store(0, Ordering::Relaxed);
    }
}

struct IgnoreRequests;
impl RequestHandler for IgnoreRequests {}

/// Answers the keyboard's class requests the way a real keyboard does. Hosts
/// with simple keyboard drivers (consoles, BIOSes) switch to boot protocol and
/// set LEDs. embassy-usb rejects both by default, which can leave the keyboard
/// enumerated but ignored. Our reports already use the boot layout, so
/// accepting boot protocol costs nothing.
struct KeyboardRequests {
    protocol: HidProtocolMode,
    idle_ms: u32,
}

impl RequestHandler for KeyboardRequests {
    fn get_report(&mut self, id: ReportId, buf: &mut [u8]) -> Option<usize> {
        info!("keyboard: GET_REPORT {id:?}");
        let empty = KeyboardReport::EMPTY.to_bytes();
        let n = empty.len().min(buf.len());
        buf[..n].copy_from_slice(&empty[..n]);
        Some(n)
    }

    fn set_report(&mut self, id: ReportId, data: &[u8]) -> OutResponse {
        info!("keyboard: SET_REPORT {id:?} {data:02x?} (LEDs)");
        if let Some(&leds) = data.first() {
            LEDS.store(leds, Ordering::Relaxed);
            LEDS_CHANGED.signal(());
        }
        OutResponse::Accepted
    }

    fn get_protocol(&self) -> HidProtocolMode {
        self.protocol
    }

    fn set_protocol(&mut self, protocol: HidProtocolMode) -> OutResponse {
        info!("keyboard: SET_PROTOCOL {protocol:?}");
        self.protocol = protocol;
        OutResponse::Accepted
    }

    fn get_idle_ms(&mut self, _id: Option<ReportId>) -> Option<u32> {
        Some(self.idle_ms)
    }

    fn set_idle_ms(&mut self, id: Option<ReportId>, duration_ms: u32) {
        info!("keyboard: SET_IDLE {id:?} {duration_ms} ms");
        self.idle_ms = duration_ms;
    }
}

/// The USB device and its HID interfaces. Each board runs these with [`run_device`],
/// [`run_gamepad`] and [`run_keyboard`] in tasks of its own (tasks can't be generic).
pub struct UsbParts<D: Driver<'static>> {
    pub device: UsbDevice<'static, D>,
    pub gamepad: Option<HidReaderWriter<'static, D, 8, 8>>,
    pub keyboard: Option<HidWriter<'static, D, 8>>,
}

/// Builds the USB device for the saved mode. Boards call this through
/// [`usb_tasks!`](crate::usb_tasks), which also starts its tasks.
pub fn build<D: Driver<'static>>(driver: D, settings: &Settings) -> UsbParts<D> {
    let mode = settings.mode();
    let keyboard_poll_ms = settings.keyboard_poll_ms();
    info!("USB mode: {mode:?}");
    if mode.has_keyboard() {
        info!("keyboard poll interval: {keyboard_poll_ms} ms");
    }
    MODE.store(mode as u8, Ordering::Relaxed);
    let id = if mode.has_gamepad() {
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

    let gamepad = mode.has_gamepad().then(|| {
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

    // IN endpoint only, like a real keyboard; LED output arrives via SET_REPORT.
    static KEYBOARD_REQUESTS: StaticCell<KeyboardRequests> = StaticCell::new();
    let keyboard = mode.has_keyboard().then(|| {
        HidWriter::new(
            &mut builder,
            KEYBOARD_STATE.init(hid::State::new()),
            hid::Config {
                report_descriptor: KEYBOARD_DESCRIPTOR,
                request_handler: Some(KEYBOARD_REQUESTS.init(KeyboardRequests {
                    protocol: HidProtocolMode::Report,
                    idle_ms: 0,
                })),
                poll_ms: keyboard_poll_ms,
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

pub async fn run_device<D: Driver<'static>>(mut device: UsbDevice<'static, D>) -> ! {
    device.run().await
}

pub async fn run_gamepad<D: Driver<'static>>(hid: HidReaderWriter<'static, D, 8, 8>) {
    let (reader, mut writer) = hid.split();
    let forward = async {
        // Send the current state on every poll, not just on change: console
        // drivers can treat a pad that goes quiet as idle or disconnected.
        let mut current = GamepadReport::NEUTRAL;
        loop {
            if let Ok(report) =
                with_timeout(Duration::from_millis(REPEAT_MS), GAMEPAD_REPORTS.receive()).await
            {
                current = report;
            }
            if !is_configured() {
                current = GamepadReport::NEUTRAL; // nothing plugged in; drop stale input
                continue;
            }
            if let Err(e) = writer.write(&current.to_bytes()).await {
                warn!("gamepad write failed: {e:?}");
                Timer::after(Duration::from_millis(100)).await;
            }
        }
    };
    join(reader.run(false, &mut IgnoreRequests), forward).await;
}

pub async fn run_keyboard<D: Driver<'static>>(mut writer: HidWriter<'static, D, 8>) {
    const NO_KEYS: [u8; 6] = [0; 6];
    // What the host currently sees.
    let mut current = KeyboardReport::EMPTY;
    // Whether to correct for the host's Caps Lock. Off for the rest of a typing
    // run if the host doesn't confirm the change, so we never toggle it back and forth.
    let mut fix_caps_lock = true;
    loop {
        let cmd = KEYBOARD_CMDS.receive().await;
        if !is_configured() {
            // Nothing plugged in: drop it, but still count keystrokes so the
            // client's flow control doesn't stall.
            if let KeyCmd::Tap { .. } = cmd {
                mark_typed();
            }
            current = KeyboardReport::EMPTY;
            continue;
        }
        match cmd {
            KeyCmd::State(report) => send_keyboard(&mut writer, &mut current, report).await,
            KeyCmd::Tap {
                modifiers,
                key,
                press_ms,
                release_ms,
                rollover,
            } => {
                // Typed text comes out as written: if the host has Caps Lock on,
                // turn it off before a letter (Shift's effect under Caps Lock
                // differs between hosts, so swapping case wouldn't be reliable).
                if fix_caps_lock
                    && (0x04..=0x1D).contains(&key)
                    && LEDS.load(Ordering::Relaxed) & LED_CAPS_LOCK != 0
                {
                    fix_caps_lock = turn_caps_lock_off(&mut writer, &mut current).await;
                }
                // Each write completes only once the host has taken the report,
                // so every state below is seen by the host. Nothing is skipped
                // however fast the queue fills.
                let idle = KeyboardReport {
                    modifiers,
                    keys: NO_KEYS,
                };
                let mut down = idle;
                down.keys[0] = key;
                // With rollover, the previous key may still be down. A rollover
                // keystroke may go straight to its key in one report (previous up +
                // next down) when the modifiers match and it's a different key.
                let held = current.keys[0];
                let straight =
                    rollover && held != 0 && held != key && current.modifiers == modifiers;
                if !straight && current != idle {
                    // Release anything still down and change modifiers on their own,
                    // so the host applies Shift before the key arrives and a repeated
                    // key ("ll") registers twice.
                    send_keyboard(&mut writer, &mut current, idle).await;
                    if held != 0 {
                        hold(release_ms).await;
                    }
                }
                send_keyboard(&mut writer, &mut current, down).await;
                hold(press_ms).await;
                if !rollover {
                    send_keyboard(&mut writer, &mut current, idle).await;
                    hold(release_ms).await;
                }
                mark_typed();
                // Let go of everything once there's nothing more to type. While
                // more is queued, keep modifiers (and a rolled-over key) for the
                // next keystroke to deal with.
                if KEYBOARD_CMDS.is_empty() {
                    if current != KeyboardReport::EMPTY {
                        send_keyboard(&mut writer, &mut current, KeyboardReport::EMPTY).await;
                    }
                    fix_caps_lock = true; // next typing run tries again
                }
            }
        }
    }
}

/// Presses Caps Lock until the host reports it off (or a timeout), then releases
/// it. Returns false if the host didn't confirm.
async fn turn_caps_lock_off<D: Driver<'static>>(
    writer: &mut HidWriter<'static, D, 8>,
    current: &mut KeyboardReport,
) -> bool {
    if *current != KeyboardReport::EMPTY {
        send_keyboard(writer, current, KeyboardReport::EMPTY).await;
    }
    LEDS_CHANGED.reset();
    let mut caps = KeyboardReport::EMPTY;
    caps.keys[0] = KEY_CAPS_LOCK;
    send_keyboard(writer, current, caps).await;
    let confirmed = with_timeout(Duration::from_millis(CAPS_LOCK_MAX_HOLD_MS), async {
        while LEDS.load(Ordering::Relaxed) & LED_CAPS_LOCK != 0 {
            LEDS_CHANGED.wait().await;
        }
    })
    .await
    .is_ok();
    send_keyboard(writer, current, KeyboardReport::EMPTY).await;
    if confirmed {
        info!("keyboard: turned the host's Caps Lock off before typing");
    } else {
        warn!("keyboard: host didn't confirm Caps Lock off; typing without correcting");
    }
    confirmed
}

async fn send_keyboard<D: Driver<'static>>(
    writer: &mut HidWriter<'static, D, 8>,
    current: &mut KeyboardReport,
    report: KeyboardReport,
) {
    if let Err(e) = writer.write(&report.to_bytes()).await {
        warn!("keyboard write failed: {e:?}");
    }
    *current = report;
}

/// Keep the current key state for at least `ms` (on top of the host read that
/// already happened), for apps that only check the keyboard once per frame.
async fn hold(ms: u8) {
    if ms > 0 {
        Timer::after(Duration::from_millis(u64::from(ms))).await;
    }
}

/// Defines `mod usb_tasks` for a board's USB driver type (give it as a full path):
/// the device, gamepad and keyboard tasks (Embassy tasks can't be generic), and
/// `usb_tasks::spawn(spawner, driver, &settings)`, which builds the device for the
/// saved mode and starts them.
#[macro_export]
macro_rules! usb_tasks {
    ($driver:ty) => {
        #[allow(
            clippy::large_stack_frames,
            reason = "a task's future lives in static task storage, not on a stack"
        )]
        mod usb_tasks {
            use $crate::__embassy_usb::UsbDevice;
            use $crate::__embassy_usb::class::hid::{HidReaderWriter, HidWriter};
            use $crate::usb;

            type Driver = $driver;

            pub fn spawn(
                spawner: ::embassy_executor::Spawner,
                driver: Driver,
                settings: &$crate::settings::Settings,
            ) {
                let parts = usb::build(driver, settings);
                spawner.spawn(device_task(parts.device).unwrap());
                if let Some(gamepad) = parts.gamepad {
                    spawner.spawn(gamepad_task(gamepad).unwrap());
                }
                if let Some(keyboard) = parts.keyboard {
                    spawner.spawn(keyboard_task(keyboard).unwrap());
                }
            }

            #[::embassy_executor::task]
            async fn device_task(device: UsbDevice<'static, Driver>) -> ! {
                usb::run_device(device).await
            }

            #[::embassy_executor::task]
            async fn gamepad_task(hid: HidReaderWriter<'static, Driver, 8, 8>) {
                usb::run_gamepad(hid).await
            }

            #[::embassy_executor::task]
            async fn keyboard_task(writer: HidWriter<'static, Driver, 8>) {
                usb::run_keyboard(writer).await
            }
        }
    };
}
