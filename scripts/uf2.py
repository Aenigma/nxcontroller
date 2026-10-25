#!/usr/bin/env python3
"""Convert a Pico firmware ELF to UF2, the format the Pico's BOOTSEL drive accepts.

    scripts/uf2.py rp2040 firmware.elf out.uf2    # Pico W
    scripts/uf2.py rp2350 firmware.elf out.uf2    # Pico 2 W

Only the standard library. Writes every loadable segment at its load (physical)
address, in 256-byte blocks, tagged with the chip's UF2 family ID.
UF2 format: https://github.com/microsoft/uf2
"""

import struct
import sys

FAMILIES = {
    "rp2040": 0xE48BFF56,
    "rp2350": 0xE48BFF59,  # RP2350, Arm secure
}
FLASH_START, FLASH_END = 0x10000000, 0x14000000
MAGIC_START0, MAGIC_START1, MAGIC_END = 0x0A324655, 0x9E5D5157, 0x0AB16F30
FLAG_FAMILY_ID = 0x00002000
PAYLOAD = 256


def load_segments(elf: bytes):
    """(address, bytes) for each loadable segment of a 32-bit little-endian ELF."""
    if elf[:4] != b"\x7fELF" or elf[4] != 1 or elf[5] != 1:
        sys.exit("not a 32-bit little-endian ELF")
    phoff, = struct.unpack_from("<I", elf, 0x1C)
    phentsize, phnum = struct.unpack_from("<HH", elf, 0x2A)
    for i in range(phnum):
        p_type, p_offset, _vaddr, p_paddr, p_filesz = struct.unpack_from("<IIIII", elf, phoff + i * phentsize)
        if p_type == 1 and p_filesz:  # PT_LOAD with bytes in the file
            yield p_paddr, elf[p_offset:p_offset + p_filesz]


def to_uf2(elf: bytes, family: int) -> bytes:
    # Gather bytes by flash page, so segments that share a page share a block.
    pages: dict[int, bytearray] = {}
    for addr, data in load_segments(elf):
        if not (FLASH_START <= addr and addr + len(data) <= FLASH_END):
            sys.exit(f"segment at {addr:#x} is outside flash")
        for i, byte in enumerate(data):
            a = addr + i
            pages.setdefault(a & ~(PAYLOAD - 1), bytearray(PAYLOAD))[a % PAYLOAD] = byte
    if not pages:
        sys.exit("no loadable segments")

    out = bytearray()
    targets = sorted(pages)
    for n, target in enumerate(targets):
        header = struct.pack("<IIIIIIII", MAGIC_START0, MAGIC_START1, FLAG_FAMILY_ID,
                             target, PAYLOAD, n, len(targets), family)
        data = bytes(pages[target]).ljust(476, b"\0")
        out += header + data + struct.pack("<I", MAGIC_END)
    return bytes(out)


def main() -> None:
    if len(sys.argv) != 4 or sys.argv[1] not in FAMILIES:
        sys.exit(__doc__)
    chip, elf_path, uf2_path = sys.argv[1:]
    with open(elf_path, "rb") as f:
        uf2 = to_uf2(f.read(), FAMILIES[chip])
    with open(uf2_path, "wb") as f:
        f.write(uf2)
    print(f"{uf2_path}: {len(uf2) // 512} blocks, {len(uf2) // 512 * PAYLOAD // 1024} KiB of flash")


if __name__ == "__main__":
    main()
