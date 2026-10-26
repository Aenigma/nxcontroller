#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]
//! nxcontroller on the ESP32-S3: brings up USB, the radio and flash, then hands
//! over to `nxcontroller-core`.

#[cfg(all(feature = "ble", feature = "wifi", not(feature = "coex")))]
compile_error!("`ble` and `wifi` together need `coex`: they share the radio");
#[cfg(all(feature = "coex", not(all(feature = "ble", feature = "wifi"))))]
compile_error!("`coex` is only for builds with both `ble` and `wifi`");

mod storage;
#[cfg(feature = "wifi")]
mod wifi_radio;

use embassy_executor::Spawner;
use esp_backtrace as _;
use esp_bootloader_esp_idf::partitions::FlashStorage;
use esp_hal::clock::CpuClock;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::usb::otg::{Usb, embassy_usb_device};
use nxcontroller_core::Board;
use nxcontroller_core::settings::{SectorStorage, Settings, Storage};
use static_cell::StaticCell;

extern crate alloc;

nxcontroller_core::usb_tasks!(esp_hal::usb::otg::embassy_usb_device::Driver<'static>);

// This creates a default app-descriptor required by the esp-idf bootloader.
esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    // Console logging, plus a RAM copy of recent output for LOGS.
    nxcontroller_core::init(Board {
        reset,
        console: Some(console),
    });

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 73744);
    // BLE: the controller (and WiFi/BT coexistence) allocate from the heap.
    #[cfg(feature = "ble")]
    esp_alloc::heap_allocator!(size: 64 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    nxcontroller_core::log_boot("ESP32-S3", env!("CARGO_PKG_VERSION"));

    // --- USB HID (native USB port: GPIO20 = D+, GPIO19 = D-) ---
    static EP_OUT_BUFFER: StaticCell<[u8; 1024]> = StaticCell::new();
    let usb_peripheral = Usb::new_fs(peripherals.USB_FS, peripherals.GPIO20, peripherals.GPIO19);
    let driver = embassy_usb_device::Driver::new(
        usb_peripheral,
        EP_OUT_BUFFER.init([0; 1024]),
        embassy_usb_device::Config::default(),
    );
    static STORAGE: StaticCell<SectorStorage<FlashStorage<'static>>> = StaticCell::new();
    let storage = storage::nvs(FlashStorage::new(peripherals.FLASH))
        .map(|nvs| STORAGE.init(nvs) as &'static mut dyn Storage);
    let settings = Settings::new(storage);
    usb_tasks::spawn(spawner, driver, &settings);
    let settings = nxcontroller_core::share(settings);

    // --- WiFi (feature `wifi`) ---
    // Initialised before BLE: with both, they share the radio through `coex`.
    #[cfg(feature = "wifi")]
    let wifi = wifi::init(peripherals.WIFI, settings).await;

    // --- Bluetooth LE (feature `ble`) ---
    #[cfg(feature = "ble")]
    {
        let connector =
            esp_radio::ble::controller::BleConnector::new(peripherals.BT, Default::default())
                .expect("failed to initialize BLE");
        spawner.spawn(ble::task(connector, settings).unwrap());
    }

    #[cfg(feature = "wifi")]
    wifi::start(spawner, wifi, settings);

    loop {
        embassy_time::Timer::after(embassy_time::Duration::from_secs(3600)).await;
    }
}

fn reset() -> ! {
    esp_hal::system::software_reset()
}

fn console(line: core::fmt::Arguments<'_>) {
    esp_println::println!("{line}");
}

/// The core's BLE loop, over the chip's own controller. (Tasks can't be generic.)
#[cfg(feature = "ble")]
#[allow(
    clippy::large_stack_frames,
    reason = "a task's future lives in static task storage, not on a stack"
)]
mod ble {
    use esp_hal::efuse::{InterfaceMacAddress, interface_mac_address};
    use esp_radio::ble::controller::BleConnector;
    use nxcontroller_core::SharedSettings;

    #[embassy_executor::task]
    pub async fn task(connector: BleConnector<'static>, settings: SharedSettings) {
        // The chip's Bluetooth MAC, so the address stays the same across reboots.
        let mac = interface_mac_address(InterfaceMacAddress::Bluetooth);
        let id = mac
            .as_bytes()
            .try_into()
            .expect("a MAC address has 6 bytes");
        nxcontroller_core::ble::run(connector, id, settings).await
    }
}

#[cfg(feature = "wifi")]
mod wifi {
    use embassy_executor::Spawner;
    use embassy_net::{Ipv4Cidr, StackResources, StaticConfigV4};
    use esp_hal::peripherals::WIFI;
    use esp_hal::rng::Rng;
    use esp_radio::wifi::sta::StationConfig;
    use esp_radio::wifi::{
        AuthenticationMethodConfig, Config as WifiConfig, ControllerConfig, Interface,
        PowerSaveMode, WifiController,
    };
    use log::{info, warn};
    use nxcontroller_core::SharedSettings;
    use nxcontroller_core::config::{WIFI_PASSWORD, WIFI_SSID};
    use nxcontroller_core::{net, setup};

    use crate::wifi_radio;
    use static_cell::StaticCell;

    pub struct Wifi {
        controller: WifiController<'static>,
        station: Option<StationConfig>,
    }

    /// Picks the network (saved credentials win, then ones built in from
    /// firmware/.env, else none: setup hotspot) and brings up the controller.
    #[allow(
        clippy::large_stack_frames,
        reason = "runs once at boot, on main's stack"
    )]
    pub async fn init(device: WIFI<'static>, settings: SharedSettings) -> Wifi {
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
            device,
            ControllerConfig::default().with_initial_config(initial),
        )
        .expect("failed to initialize WiFi");
        // Power saving adds tens of milliseconds of latency to every packet.
        controller
            .set_power_saving(PowerSaveMode::None)
            .expect("failed to disable WiFi power saving");
        Wifi {
            controller,
            station,
        }
    }

    /// Network stacks and tasks: station (DHCP) for control, plus the setup hotspot.
    #[allow(
        clippy::large_stack_frames,
        reason = "runs once at boot, on main's stack"
    )]
    pub fn start(spawner: Spawner, wifi: Wifi, settings: SharedSettings) {
        let rng = Rng::new();
        let seed = (u64::from(rng.random()) << 32) | u64::from(rng.random());

        static RESOURCES: StaticCell<StackResources<4>> = StaticCell::new();
        let (stack, runner) = embassy_net::new(
            Interface::station(),
            embassy_net::Config::dhcpv4(Default::default()),
            RESOURCES.init(StackResources::new()),
            seed,
        );
        // Fixed address; only active while the hotspot is up.
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

        spawner.spawn(wifi_radio::wifi_task(wifi.controller, wifi.station).unwrap());
        spawner.spawn(wifi_radio::net_task(runner).unwrap());
        spawner.spawn(net::control_server_task(stack, settings).unwrap());
        spawner.spawn(net::discovery_task(stack).unwrap());
        spawner.spawn(wifi_radio::net_task(ap_runner).unwrap());
        spawner.spawn(setup::http_task(ap_stack, settings).unwrap());
        spawner.spawn(setup::dhcp_task(ap_stack).unwrap());
        spawner.spawn(setup::dns_task(ap_stack).unwrap());
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
}
