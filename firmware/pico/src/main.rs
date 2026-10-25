#![no_std]
#![no_main]
//! nxcontroller on the Raspberry Pi Pico W (RP2040) and Pico 2 W (RP2350): brings
//! up USB, the CYW43439 radio (Bluetooth only) and flash, then hands over to
//! `nxcontroller-core`. There's no serial console: the log is kept in RAM and
//! read over Bluetooth (LOGS), like on the ESP32-S3.

use cyw43::{Cyw43439, aligned_bytes};
use cyw43_pio::PioSpi;
use embassy_executor::Spawner;
use embassy_rp::flash::{Blocking, ERASE_SIZE, Flash};
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::{DMA_CH0, DMA_CH1, FLASH, PIO0, USB};
use embassy_rp::pio::{self, Pio};
use embassy_rp::{bind_interrupts, dma, usb as rp_usb};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use log::info;
use nxcontroller_core::config::DEVICE_NAME;
use nxcontroller_core::settings::{Settings, SettingsError, Storage};
use nxcontroller_core::{Board, SharedSettings, usb};
use static_cell::StaticCell;

#[cfg(all(feature = "rp2040", feature = "rp2350"))]
compile_error!("pick one chip: `rp2040` (Pico W) or `rp2350` (Pico 2 W)");

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
    DMA_IRQ_0 => dma::InterruptHandler<DMA_CH0>, dma::InterruptHandler<DMA_CH1>;
    USBCTRL_IRQ => rp_usb::InterruptHandler<USB>;
});

type UsbDriver = rp_usb::Driver<'static, USB>;

/// Flash size of the board (the memory map in memory-*.x matches).
#[cfg(feature = "rp2040")]
const FLASH_SIZE: usize = 2 * 1024 * 1024;
#[cfg(feature = "rp2350")]
const FLASH_SIZE: usize = 4 * 1024 * 1024;
/// The last sector, which the memory map keeps out of the program's way.
const SETTINGS_OFFSET: u32 = (FLASH_SIZE - ERASE_SIZE) as u32;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    nxcontroller_core::init(Board {
        reset,
        console: None,
    });
    let p = embassy_rp::init(Default::default());

    info!(
        "nxcontroller v{} '{}' booting on {} (ble: true, wifi: false)",
        env!("CARGO_PKG_VERSION"),
        DEVICE_NAME,
        if cfg!(feature = "rp2040") {
            "Pico W"
        } else {
            "Pico 2 W"
        },
    );

    // --- Flash: settings, and an id for the Bluetooth address ---
    let mut flash = Flash::<_, Blocking, FLASH_SIZE>::new_blocking(p.FLASH);
    let id = board_id(&mut flash);
    static STORAGE: StaticCell<FlashStorage> = StaticCell::new();
    let settings = Settings::new(Some(STORAGE.init(FlashStorage(flash))));

    // --- USB HID (the micro-USB port) ---
    let mode = settings.mode();
    info!("USB mode: {mode:?}");
    let keyboard_poll_ms = settings.keyboard_poll_ms();
    if mode.has_keyboard() {
        info!("keyboard poll interval: {keyboard_poll_ms} ms");
    }
    let parts = usb::build(rp_usb::Driver::new(p.USB, Irqs), mode, keyboard_poll_ms);
    spawner.spawn(tasks::usb_device_task(parts.device).unwrap());
    if let Some(gamepad) = parts.gamepad {
        spawner.spawn(tasks::gamepad_task(gamepad).unwrap());
    }
    if let Some(keyboard) = parts.keyboard {
        spawner.spawn(tasks::keyboard_task(keyboard).unwrap());
    }

    static SETTINGS: StaticCell<Mutex<CriticalSectionRawMutex, Settings>> = StaticCell::new();
    let settings: SharedSettings = SETTINGS.init(Mutex::new(settings));

    // --- CYW43439 radio, Bluetooth only (wired to PIO0 on both boards) ---
    let fw = aligned_bytes!("../cyw43-firmware/43439A0.bin");
    let clm = aligned_bytes!("../cyw43-firmware/43439A0_clm.bin");
    let btfw = aligned_bytes!("../cyw43-firmware/43439A0_btfw.bin");
    let nvram = aligned_bytes!("../cyw43-firmware/nvram_rp2040.bin");

    let pwr = Output::new(p.PIN_23, Level::Low);
    let cs = Output::new(p.PIN_25, Level::High);
    let mut pio = Pio::new(p.PIO0, Irqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        // The Pico 2 W carries the radio on an RM2 module, which wants a slower clock.
        if cfg!(feature = "rp2350") {
            cyw43_pio::RM2_CLOCK_DIVIDER
        } else {
            cyw43_pio::DEFAULT_CLOCK_DIVIDER
        },
        pio.irq0,
        cs,
        p.PIN_24,
        p.PIN_29,
        dma::Channel::new(p.DMA_CH0, Irqs),
        dma::Channel::new(p.DMA_CH1, Irqs),
    );
    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let (_net_device, bt_device, mut control, runner) =
        cyw43::new_with_bluetooth(STATE.init(cyw43::State::new()), pwr, spi, fw, btfw, nvram).await;
    spawner.spawn(tasks::cyw43_task(runner).unwrap());
    control.init(clm).await;

    spawner.spawn(tasks::ble_task(bt_device, id, settings).unwrap());
}

/// Settings live in the last flash sector.
struct FlashStorage(Flash<'static, FLASH, Blocking, FLASH_SIZE>);

impl Storage for FlashStorage {
    fn read(&mut self, record: &mut [u8]) -> Result<(), SettingsError> {
        self.0
            .blocking_read(SETTINGS_OFFSET, record)
            .map_err(|_| SettingsError::Flash)
    }

    fn write(&mut self, record: &[u8]) -> Result<(), SettingsError> {
        self.0
            .blocking_erase(SETTINGS_OFFSET, SETTINGS_OFFSET + ERASE_SIZE as u32)
            .map_err(|_| SettingsError::Flash)?;
        self.0
            .blocking_write(SETTINGS_OFFSET, record)
            .map_err(|_| SettingsError::Flash)
    }
}

/// 6 bytes unique to this board, for a Bluetooth address that survives reboots:
/// the flash chip's id on the RP2040, the chip id in OTP on the RP2350.
fn board_id(flash: &mut Flash<'static, FLASH, Blocking, FLASH_SIZE>) -> [u8; 6] {
    let mut uid = [0u8; 8];
    #[cfg(feature = "rp2040")]
    let _ = flash.blocking_unique_id(&mut uid);
    #[cfg(feature = "rp2350")]
    {
        let _ = flash;
        if let Ok(chip_id) = embassy_rp::otp::get_chipid() {
            uid = chip_id.to_be_bytes();
        }
    }
    // Fold all 8 bytes into 6.
    let mut id = [0u8; 6];
    for (i, b) in uid.iter().enumerate() {
        id[i % 6] ^= b;
    }
    id
}

fn reset() -> ! {
    cortex_m::peripheral::SCB::sys_reset()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    // No console to report it on: start over.
    cortex_m::peripheral::SCB::sys_reset()
}

/// Tasks can't be generic, so the core's USB and BLE loops run in these.
mod tasks {
    use embassy_rp::gpio::Output;
    use embassy_usb::UsbDevice;
    use embassy_usb::class::hid::{HidReaderWriter, HidWriter};

    use super::*;

    #[embassy_executor::task]
    pub async fn usb_device_task(device: UsbDevice<'static, UsbDriver>) -> ! {
        usb::run_device(device).await
    }

    #[embassy_executor::task]
    pub async fn gamepad_task(hid: HidReaderWriter<'static, UsbDriver, 8, 8>) {
        usb::run_gamepad(hid).await
    }

    #[embassy_executor::task]
    pub async fn keyboard_task(writer: HidWriter<'static, UsbDriver, 8>) {
        usb::run_keyboard(writer).await
    }

    #[embassy_executor::task]
    pub async fn cyw43_task(
        runner: cyw43::Runner<
            'static,
            cyw43::SpiBus<Output<'static>, PioSpi<'static, PIO0, 0>>,
            Cyw43439,
        >,
    ) -> ! {
        runner.run().await
    }

    #[embassy_executor::task]
    pub async fn ble_task(
        bt: cyw43::bluetooth::BtDriver<'static>,
        id: [u8; 6],
        settings: SharedSettings,
    ) {
        nxcontroller_core::ble::run(bt, id, settings).await
    }
}
