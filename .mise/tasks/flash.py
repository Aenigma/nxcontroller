#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# ///
#MISE description="Build and flash the ESP32-S3, then show its log (-- --help for options)"
#MISE env={_.file = "{{config_root}}/firmware/.env", _.source = "{{env.HOME}}/export-esp.sh"}
"""Build and flash the ESP32-S3, then show its log.

With both of the board's USB ports plugged into this computer, it flashes
through the native USB port (the chip's ROM must be in download mode: hold
BOOT, tap RST) and reads the log through the UART port, which also restarts
the board into the new firmware. Flashing through the UART port fails on macOS
with the CH343 chip.

Arguments it doesn't know go to `cargo build`, e.g. `-- --features ...`.
"""

import argparse
import os
import subprocess
import sys
import time

FIRMWARE = "firmware/esp32s3"
ELF = f"{FIRMWARE}/target/xtensa-esp32s3-none-elf/release/nxcontroller-firmware"


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("-p", "--port", help="flash through PORT instead, with espflash's own reset")
    parser.add_argument("--monitor-port", help="the UART port for the log (default: found automatically)")
    parser.add_argument("--no-monitor", action="store_true", help="restart into the firmware, without the log")
    args, cargo_args = parser.parse_known_args()

    subprocess.run(["cargo", "build", "--release", *cargo_args], cwd=FIRMWARE, check=True)

    if args.port:
        espflash("flash", "--port", args.port, ELF)
    else:
        port = wait_for_download_mode()
        print(f"==> flashing via {port}")
        espflash("flash", "--before", "no-reset", "--after", "no-reset", "--port", port, ELF)

    log_port = args.monitor_port or uart_port()
    if not log_port:
        print("Flashed. No UART port found: press RST to start the firmware.")
    elif args.no_monitor:
        restart_via(log_port)
    else:
        # Opening the monitor resets the chip into the new firmware.
        os.execvp("espflash", ["espflash", "monitor", "--chip", "esp32s3", "--no-stub",
                               "--port", log_port, "--elf", ELF])


def espflash(command, *args):
    """Runs espflash, giving up after two minutes (a wedged port can hang it)."""
    subprocess.run(["espflash", command, "--chip", "esp32s3", *args],
                   check=True, timeout=120)


# USB vendor IDs: the chip's own USB port, and the usual UART bridge chips on dev boards.
ESPRESSIF = "303A"
UART_BRIDGES = {"1A86": "WCH CH34x", "10C4": "Silicon Labs CP210x", "0403": "FTDI"}


def usb_ports():
    """Every USB serial port, as (port, vendor ID), from espflash: /dev/... or COM3 alike.
    (espflash's default list leaves out some bridge chips, like the CH343; hence all of them.)"""
    listing = subprocess.run(["espflash", "list-ports", "--list-all-ports"], capture_output=True, text=True).stdout
    ports = []
    for line in listing.splitlines():
        fields = line.split()
        # "<port>  <product id>:<vendor id>  <description>"; skip non-USB ports and Macs' /dev/tty.* twins.
        if len(fields) >= 2 and ":" in fields[1] and not fields[0].startswith("/dev/tty."):
            ports.append((fields[0], fields[1].split(":")[1].upper()))
    return ports


def native_port():
    """The chip's native USB port, there only while its ROM is in download mode."""
    return next((port for port, vendor in usb_ports() if vendor == ESPRESSIF), None)


def uart_port():
    """The UART bridge's port, which carries the log."""
    return next((port for port, vendor in usb_ports() if vendor in UART_BRIDGES), None)


def wait_for_download_mode():
    """The native port, asking for BOOT + RST and waiting up to a minute if it isn't there yet."""
    if port := native_port():
        return port
    print("\n>>> Put the board in download mode: hold BOOT, tap RST, release BOOT.\n"
          "    (its native USB port must be plugged into this computer)")
    for _ in range(60):
        time.sleep(1)
        if port := native_port():
            return port
    sys.exit("The board didn't show up in download mode.")


def restart_via(port):
    """Opens the monitor just long enough to reset the chip, then closes it."""
    monitor = subprocess.Popen(["espflash", "monitor", "--chip", "esp32s3", "--no-stub", "--non-interactive",
                                "--port", port], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(2)
    monitor.terminate()
    print(f"Flashed and restarted via {port}.")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as e:
        sys.exit(e.returncode)
    except subprocess.TimeoutExpired:
        sys.exit("espflash didn't finish within two minutes (wedged port?)")
