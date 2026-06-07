/* RP2040 memory layout for the Raspberry Pi Pico (2 MB flash).
 *
 * BOOT2   — 256-byte second-stage bootloader the ROM loads first.
 *           It configures the QSPI flash for XIP (execute-in-place) mode,
 *           then jumps to the application at the start of FLASH.
 *           The rp2040-boot2 crate (a dependency of rp2040-hal) provides
 *           the actual binary; this region just tells the linker where to
 *           place it.
 *
 * FLASH   — The remaining ~2 MB of onboard flash where your program lives.
 *
 * RAM     — 264 KB of SRAM (two 128 KB banks + two 4 KB scratch banks).
 *           Only the main 256 KB bank is listed under RAM; the scratch
 *           banks are mapped separately below.
 *
 * SCRATCH — Two 4 KB banks used by the USB ROM bootloader.  Available
 *           to user code as well (e.g. for DMA scratch buffers).
 */
MEMORY {
    BOOT2     : ORIGIN = 0x10000000, LENGTH = 0x100
    FLASH     : ORIGIN = 0x10000100, LENGTH = 2048K - 0x100
    RAM       : ORIGIN = 0x20000000, LENGTH = 256K
    SCRATCH_A : ORIGIN = 0x20040000, LENGTH = 4K
    SCRATCH_B : ORIGIN = 0x20041000, LENGTH = 4K
}

/* Force the boot2 symbol to be linked even if nothing else references it.
 * rp2040-hal places the 256-byte second-stage bootloader binary in a static
 * named BOOT2_FIRMWARE with #[link_section = ".boot2"].  Without this line
 * and the SECTIONS block below, the linker silently discards the section and
 * the RP2040 ROM finds no valid bootloader at 0x10000000, so it falls back to
 * USB BOOTSEL mode on every power cycle instead of running your program. */
EXTERN(BOOT2_FIRMWARE)

SECTIONS {
    /* Place the second-stage bootloader at the very start of flash.
     * The RP2040 ROM loads these 256 bytes, checks the CRC in byte 255,
     * and only jumps into FLASH (0x10000100 onwards) if the CRC passes. */
    .boot2 ORIGIN(BOOT2) : {
        KEEP(*(.boot2));
    } > BOOT2
} INSERT BEFORE .text;
