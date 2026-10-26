/* Pico 2 W (RP2350). build.rs fills in the flash size, minus the settings sector at the end. */
MEMORY {
    FLASH : ORIGIN = 0x10000000, LENGTH = {FLASH_SIZE} - {SETTINGS_SIZE}
    RAM   : ORIGIN = 0x20000000, LENGTH = 512K
    SRAM4 : ORIGIN = 0x20080000, LENGTH = 4K
    SRAM5 : ORIGIN = 0x20081000, LENGTH = 4K
}

SECTIONS {
    /* Boot ROM info: keep it in the first 4K of flash, where the boot ROM finds it. */
    .start_block : ALIGN(4)
    {
        __start_block_addr = .;
        KEEP(*(.start_block));
        KEEP(*(.boot_info));
    } > FLASH
} INSERT AFTER .vector_table;

/* .text starts after the boot info. */
_stext = ADDR(.start_block) + SIZEOF(.start_block);

SECTIONS {
    /* picotool 'binary info' entries. */
    .bi_entries : ALIGN(4)
    {
        __bi_entries_start = .;
        KEEP(*(.bi_entries));
        . = ALIGN(4);
        __bi_entries_end = .;
    } > FLASH
} INSERT AFTER .text;

SECTIONS {
    /* Boot ROM extra info, after everything else. */
    .end_block : ALIGN(4)
    {
        __end_block_addr = .;
        KEEP(*(.end_block));
    } > FLASH
} INSERT AFTER .uninit;

PROVIDE(start_to_end = __end_block_addr - __start_block_addr);
PROVIDE(end_to_start = __start_block_addr - __end_block_addr);
