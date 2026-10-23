#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use embassy_executor::Spawner;
use embassy_net::StackResources;
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
use log::info;
use nxcontroller_firmware::config::{DEVICE_NAME, WIFI_PASSWORD, WIFI_SSID};
use nxcontroller_firmware::settings::Settings;
use nxcontroller_firmware::{net, usb};
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
    let mut settings = Settings::new(FlashStorage::new(peripherals.FLASH));
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
    let auth = if WIFI_PASSWORD.is_empty() {
        AuthenticationMethodConfig::Open
    } else {
        AuthenticationMethodConfig::Wpa2Personal(
            WIFI_PASSWORD.try_into().expect("WIFI_PASSWORD is too long"),
        )
    };
    let station = StationConfig::default()
        .with_ssid(WIFI_SSID.try_into().expect("WIFI_SSID is too long"))
        .with_authentication(auth);
    let mut controller = WifiController::new(
        peripherals.WIFI,
        ControllerConfig::default().with_initial_config(WifiConfig::Station(station)),
    )
    .expect("failed to initialize WiFi");
    // Power saving adds tens of milliseconds of latency to every packet.
    controller
        .set_power_saving(PowerSaveMode::None)
        .expect("failed to disable WiFi power saving");

    let rng = Rng::new();
    let seed = (u64::from(rng.random()) << 32) | u64::from(rng.random());

    static RESOURCES: StaticCell<StackResources<4>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(
        Interface::station(),
        embassy_net::Config::dhcpv4(Default::default()),
        RESOURCES.init(StackResources::new()),
        seed,
    );

    spawner.spawn(net::wifi_task(controller).unwrap());
    spawner.spawn(net::net_task(runner).unwrap());
    spawner.spawn(net::control_server_task(stack, settings).unwrap());
    spawner.spawn(net::discovery_task(stack).unwrap());

    loop {
        embassy_time::Timer::after(embassy_time::Duration::from_secs(3600)).await;
    }
}
