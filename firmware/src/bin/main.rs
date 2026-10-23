#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use embassy_executor::Spawner;
use embassy_net::{Ipv4Cidr, StackResources, StaticConfigV4};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use esp_backtrace as _;
use esp_bootloader_esp_idf::partitions::FlashStorage;
use esp_hal::clock::CpuClock;
use esp_hal::rng::Rng;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::usb::otg::{Usb, embassy_usb_device};
use esp_radio::wifi::sta::StationConfig;
use esp_radio::wifi::{
    AuthenticationMethodConfig, Config as WifiConfig, ControllerConfig, Interface, PowerSaveMode,
    WifiController,
};
use log::{info, warn};
use nxcontroller_firmware::SharedSettings;
use nxcontroller_firmware::config::{DEVICE_NAME, WIFI_PASSWORD, WIFI_SSID};
use nxcontroller_firmware::settings::Settings;
use nxcontroller_firmware::{net, setup, usb};
use static_cell::StaticCell;

extern crate alloc;

// This creates a default app-descriptor required by the esp-idf bootloader.
esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 73744);
    // BLE: the controller and WiFi/BT coexistence allocate from the heap.
    #[cfg(feature = "ble")]
    esp_alloc::heap_allocator!(size: 64 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    info!(
        "nxcontroller v{} '{}' booting",
        env!("CARGO_PKG_VERSION"),
        DEVICE_NAME
    );

    // --- USB HID (native USB port: GPIO20 = D+, GPIO19 = D-) ---
    static EP_OUT_BUFFER: StaticCell<[u8; 1024]> = StaticCell::new();
    let usb_peripheral = Usb::new_fs(peripherals.USB_FS, peripherals.GPIO20, peripherals.GPIO19);
    let driver = embassy_usb_device::Driver::new(
        usb_peripheral,
        EP_OUT_BUFFER.init([0; 1024]),
        embassy_usb_device::Config::default(),
    );
    let settings = Settings::new(FlashStorage::new(peripherals.FLASH));
    let mode = settings.mode();
    info!("USB mode: {mode:?}");
    let parts = usb::build(driver, mode);
    spawner.spawn(usb::usb_device_task(parts.device).unwrap());
    if let Some(gamepad) = parts.gamepad {
        spawner.spawn(usb::gamepad_task(gamepad).unwrap());
    }
    if let Some(keyboard) = parts.keyboard {
        spawner.spawn(usb::keyboard_task(keyboard).unwrap());
    }

    // --- WiFi ---
    // Saved credentials win; then ones built in from firmware/.env; else setup hotspot.
    static SETTINGS: StaticCell<Mutex<CriticalSectionRawMutex, Settings>> = StaticCell::new();
    let settings: SharedSettings = SETTINGS.init(Mutex::new(settings));
    let station = {
        let saved = settings.lock().await;
        let (credentials, source) = match saved.wifi() {
            Some(c) => (Some(c), "saved on device"),
            None => (WIFI_SSID.map(|ssid| (ssid, WIFI_PASSWORD)), "built in"),
        };
        credentials.and_then(|(ssid, password)| {
            info!("WiFi: network '{ssid}' ({source})");
            let station = station_config(ssid, password);
            if station.is_none() {
                warn!("WiFi: credentials for '{ssid}' are invalid, ignoring them");
            }
            station
        })
    };

    let initial = match &station {
        Some(station) => WifiConfig::Station(station.clone()),
        None => WifiConfig::Station(StationConfig::default()),
    };
    let mut controller = WifiController::new(
        peripherals.WIFI,
        ControllerConfig::default().with_initial_config(initial),
    )
    .expect("failed to initialize WiFi");
    // Power saving adds tens of milliseconds of latency to every packet.
    controller
        .set_power_saving(PowerSaveMode::None)
        .expect("failed to disable WiFi power saving");

    // --- Bluetooth LE (optional second control transport) ---
    // After WiFi: both share the radio through esp-radio's `coex`.
    #[cfg(feature = "ble")]
    {
        let connector =
            esp_radio::ble::controller::BleConnector::new(peripherals.BT, Default::default())
                .expect("failed to initialize BLE");
        spawner.spawn(nxcontroller_firmware::ble::ble_task(connector).unwrap());
    }

    let rng = Rng::new();
    let seed = (u64::from(rng.random()) << 32) | u64::from(rng.random());

    // Network stack on your WiFi (DHCP client)...
    static RESOURCES: StaticCell<StackResources<4>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(
        Interface::station(),
        embassy_net::Config::dhcpv4(Default::default()),
        RESOURCES.init(StackResources::new()),
        seed,
    );
    // ...and on the setup hotspot (fixed address; only active while the hotspot is up).
    static AP_RESOURCES: StaticCell<StackResources<4>> = StaticCell::new();
    let (ap_stack, ap_runner) = embassy_net::new(
        Interface::access_point(),
        embassy_net::Config::ipv4_static(StaticConfigV4 {
            address: Ipv4Cidr::new(setup::AP_ADDRESS, 24),
            gateway: None,
            dns_servers: Default::default(),
        }),
        AP_RESOURCES.init(StackResources::new()),
        seed.rotate_left(32),
    );

    spawner.spawn(net::wifi_task(controller, station).unwrap());
    spawner.spawn(net::net_task(runner).unwrap());
    spawner.spawn(net::control_server_task(stack, settings).unwrap());
    spawner.spawn(net::discovery_task(stack).unwrap());
    spawner.spawn(net::net_task(ap_runner).unwrap());
    spawner.spawn(setup::http_task(ap_stack, settings).unwrap());
    spawner.spawn(setup::dhcp_task(ap_stack).unwrap());
    spawner.spawn(setup::dns_task(ap_stack).unwrap());

    loop {
        embassy_time::Timer::after(embassy_time::Duration::from_secs(3600)).await;
    }
}

#[allow(
    clippy::large_stack_frames,
    reason = "runs once at boot, on main's stack"
)]
fn station_config(ssid: &str, password: &str) -> Option<StationConfig> {
    let auth = if password.is_empty() {
        AuthenticationMethodConfig::Open
    } else {
        AuthenticationMethodConfig::Wpa2Personal(password.try_into().ok()?)
    };
    Some(
        StationConfig::default()
            .with_ssid(ssid.try_into().ok()?)
            .with_authentication(auth),
    )
}
