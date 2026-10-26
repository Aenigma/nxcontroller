/* Pico W (RP2040). build.rs fills in the flash size, minus the settings sector at the end. */
MEMORY {
    BOOT2 : ORIGIN = 0x10000000, LENGTH = 0x100
    FLASH : ORIGIN = 0x10000100, LENGTH = {FLASH_SIZE} - 0x100 - {SETTINGS_SIZE}
    RAM   : ORIGIN = 0x20000000, LENGTH = 264K
}
