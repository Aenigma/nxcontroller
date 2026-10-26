//! Writes the chip's memory map and the matching Rust constants from one table,
//! so the program can't grow into the settings sector at the end of flash.

use std::env;
use std::path::PathBuf;

/// The last sector of flash holds the settings; the program must stop before it.
const SETTINGS_SIZE: u32 = 4096;

fn main() {
    let rp2040 = env::var_os("CARGO_FEATURE_RP2040").is_some();
    let rp2350 = env::var_os("CARGO_FEATURE_RP2350").is_some();
    // (memory map template, flash size): Pico W 2 MiB, Pico 2 W 4 MiB.
    let (template, flash_size) = match (rp2040, rp2350) {
        (true, false) => ("memory-rp2040.x", 2 * 1024 * 1024),
        (false, true) => ("memory-rp2350.x", 4 * 1024 * 1024),
        _ => {
            panic!("build with exactly one chip feature: `rp2040` (Pico W) or `rp2350` (Pico 2 W)")
        }
    };

    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let memory = std::fs::read_to_string(template)
        .unwrap()
        .replace("{FLASH_SIZE}", &flash_size.to_string())
        .replace("{SETTINGS_SIZE}", &SETTINGS_SIZE.to_string());
    std::fs::write(out.join("memory.x"), memory).unwrap();
    std::fs::write(
        out.join("board.rs"),
        format!("const FLASH_SIZE: usize = {flash_size};\nconst SETTINGS_SIZE: usize = {SETTINGS_SIZE};\n"),
    )
    .unwrap();
    println!("cargo::rustc-link-search={}", out.display());
    println!("cargo::rerun-if-changed={template}");

    println!("cargo::rustc-link-arg-bins=--nmagic");
    println!("cargo::rustc-link-arg-bins=-Tlink.x");
    if rp2040 {
        println!("cargo::rustc-link-arg-bins=-Tlink-rp.x"); // second-stage bootloader
    }
}
