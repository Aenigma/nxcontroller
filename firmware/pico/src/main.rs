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
use nxcontroller_core::settings::{SectorStorage, Settings};
use nxcontroller_core::{Board, SharedSettings};
use static_cell::StaticCell;

// FLASH_SIZE and SETTINGS_SIZE, written by build.rs from the same numbers as memory.x.
include!(concat!(env!("OUT_DIR"), "/board.rs"));
/// The last sector, which the memory map keeps out of the program's way.
const SETTINGS_OFFSET: u32 = (FLASH_SIZE - SETTINGS_SIZE) as u32;
const _: () = assert!(
    SETTINGS_SIZE >= ERASE_SIZE,
    "settings must fill whole erase sectors"
);

type BoardFlash = Flash<'static, FLASH, Blocking, FLASH_SIZE>;

nxcontroller_core::usb_tasks!(embassy_rp::usb::Driver<'static, embassy_rp::peripherals::USB>);

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
    DMA_IRQ_0 => dma::InterruptHandler<DMA_CH0>, dma::InterruptHandler<DMA_CH1>;
    USBCTRL_IRQ => rp_usb::InterruptHandler<USB>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    nxcontroller_core::init(Board {
        reset,
        console: None,
    });
    let p = embassy_rp::init(Default::default());

    let board = if cfg!(feature = "rp2040") {
        "Pico W"
    } else {
        "Pico 2 W"
    };
    nxcontroller_core::log_boot(board, env!("CARGO_PKG_VERSION"));

    // --- Flash: settings, and an id for the Bluetooth address ---
    let mut flash = BoardFlash::new_blocking(p.FLASH);
    let id = board_id(&mut flash);
    static STORAGE: StaticCell<SectorStorage<BoardFlash>> = StaticCell::new();
    let settings = Settings::new(Some(
        STORAGE.init(SectorStorage::new(flash, SETTINGS_OFFSET)),
    ));

    // --- USB HID (the micro-USB port) ---
    usb_tasks::spawn(spawner, rp_usb::Driver::new(p.USB, Irqs), &settings);
    let settings = nxcontroller_core::share(settings);

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

/// 6 bytes unique to this board, for a Bluetooth address that survives reboots:
/// the flash chip's id on the RP2040, the chip id in OTP on the RP2350.
fn board_id(flash: &mut BoardFlash) -> [u8; 6] {
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
    reset()
}

/// The radio and the core's BLE loop. (Tasks can't be generic.)
mod tasks {
    use super::*;

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
