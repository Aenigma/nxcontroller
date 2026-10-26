#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# ///
#MISE description="ESP32-S3 installer images (Bluetooth only) and manifest, into web/public/"
#MISE env={_.source = "{{env.HOME}}/export-esp.sh", WIFI_SSID = false, WIFI_PASSWORD = false, NXC_NAME = false}
"""The web installer's ESP32-S3 firmware: Bluetooth only, never with your WiFi or
name. ESP Web Tools writes three parts: the bootloader, the partition table and
the app. A single merged image would also overwrite the settings partition
between them, wiping the saved mode and WiFi on every update.
"""

import json
import os
import subprocess
import tomllib

FIRMWARE = "firmware/esp32s3"
ELF = f"{FIRMWARE}/target/xtensa-esp32s3-none-elf/release/nxcontroller-firmware"
OUT = "web/public"


def run(*cmd, **kwargs):
    subprocess.run(cmd, check=True, **kwargs)


run("cargo", "build", "--release", "--no-default-features", "--features", "ble", cwd=FIRMWARE)

# The merged image is the whole flash from 0x0; cut the bootloader and partition table out of it.
os.makedirs(f"{OUT}/firmware", exist_ok=True)
run("espflash", "save-image", "--chip", "esp32s3", "--merge", ELF, f"{OUT}/firmware/merged.bin",
    stdout=subprocess.DEVNULL)
with open(f"{OUT}/firmware/merged.bin", "rb") as f:
    merged = f.read()
with open(f"{OUT}/firmware/bootloader.bin", "wb") as f:
    f.write(merged[0x0:0x8000])
with open(f"{OUT}/firmware/partitions.bin", "wb") as f:
    f.write(merged[0x8000:0x9000])
os.remove(f"{OUT}/firmware/merged.bin")
run("espflash", "save-image", "--chip", "esp32s3", ELF, f"{OUT}/firmware/app.bin", stdout=subprocess.DEVNULL)

with open(f"{FIRMWARE}/Cargo.toml", "rb") as f:
    version = tomllib.load(f)["package"]["version"]
manifest = {
    "name": "nxcontroller",
    "version": version,
    "new_install_prompt_erase": True,
    "builds": [{
        "chipFamily": "ESP32-S3",
        "parts": [
            {"path": "firmware/bootloader.bin", "offset": 0x0},
            {"path": "firmware/partitions.bin", "offset": 0x8000},
            {"path": "firmware/app.bin", "offset": 0x10000},
        ],
    }],
}
with open(f"{OUT}/manifest.json", "w") as f:
    json.dump(manifest, f, indent=2)
print(f"ESP32-S3 firmware v{version}: images and manifest in {OUT}/")
