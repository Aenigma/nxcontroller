//! Puts the chip's memory map where the linker finds it, and adds the link scripts.

use std::env;
use std::path::PathBuf;

fn main() {
    let rp2040 = env::var_os("CARGO_FEATURE_RP2040").is_some();
    let rp2350 = env::var_os("CARGO_FEATURE_RP2350").is_some();
    let memory = match (rp2040, rp2350) {
        (true, false) => "memory-rp2040.x",
        (false, true) => "memory-rp2350.x",
        _ => {
            panic!("build with exactly one chip feature: `rp2040` (Pico W) or `rp2350` (Pico 2 W)")
        }
    };

    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    std::fs::copy(memory, out.join("memory.x")).unwrap();
    println!("cargo::rustc-link-search={}", out.display());
    println!("cargo::rerun-if-changed={memory}");

    println!("cargo::rustc-link-arg-bins=--nmagic");
    println!("cargo::rustc-link-arg-bins=-Tlink.x");
    if rp2040 {
        println!("cargo::rustc-link-arg-bins=-Tlink-rp.x"); // second-stage bootloader
    }
}
