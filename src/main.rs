// ── Crate-level attributes ────────────────────────────────────────────────────
//
// #![no_std]
//   We run on bare metal with no OS.  Rust's `std` assumes an OS for
//   heap allocation, threads, files, etc. — none of that exists here.
//   We still get `core` (the OS-agnostic subset: math, iterators, fmt, …).
#![no_std]
//
// #![no_main]
//   There is no OS to call our `main()`.  The `#[entry]` attribute below
//   designates our own function as the CPU's reset handler instead.
#![no_main]

// ── Imports ───────────────────────────────────────────────────────────────────

// Panic handler: on panic, spin forever in a tight loop.
// `as _` means "pull it in for its side-effect (registering the handler)
// without giving it a name we'd otherwise have to use."
use panic_halt as _;

// `entry` marks a function as the CPU reset vector — the first thing
// that runs after power-on or reset.
use rp_pico::entry;

// `hal` is rp2040-hal, the Hardware Abstraction Layer.  The BSP (rp-pico)
// re-exports it so we don't need a separate rp2040-hal dependency line.
use rp_pico::hal;

// PAC = Peripheral Access Crate — auto-generated register-level structs.
// `pac::Peripherals::take()` gives us ownership of every on-chip peripheral
// exactly once (a second call returns None), enforcing hardware safety.
use rp_pico::hal::pac;

// Clock initialisation helper.  Without this the chip runs on its slow
// internal oscillator (~6 MHz).  After calling it we get 125 MHz system
// clock AND the 48 MHz USB clock the USB hardware needs.
use rp_pico::hal::clocks::init_clocks_and_plls;

// The on-board crystal is 12 MHz; this constant is used by the PLL maths.
use rp_pico::XOSC_CRYSTAL_FREQ;

// GPIO controller (Single-cycle IO block) and watchdog timer.
use rp_pico::hal::sio::Sio;
use rp_pico::hal::watchdog::Watchdog;

// `OutputPin` is the embedded-hal trait that provides `set_high()`/`set_low()`.
// Trait methods only work when the trait is in scope, even if you never write
// `OutputPin::set_high` explicitly.
use embedded_hal::digital::v2::OutputPin;

// ── USB imports ───────────────────────────────────────────────────────────────
//
// USB CDC (Communications Device Class) is the protocol that makes the
// Pico appear as a virtual serial port on the host computer.  It needs
// three layers:
//
//   1. UsbBus      — the RP2040's USB hardware driver (from rp2040-hal).
//   2. usb-device  — the device-side USB stack (enumeration, descriptors).
//   3. usbd-serial — the CDC-ACM serial class built on top of usb-device.

// `hal::usb::UsbBus` is the rp2040-hal driver that talks to the USB hardware.
use hal::usb::UsbBus;

// `class_prelude` brings in `UsbBusAllocator` (shares the bus across USB
// classes) and `StringDescriptors` (manufacturer/product/serial strings).
// `prelude` brings in `UsbDeviceBuilder`, `UsbVidPid`, and `UsbDevice`.
use usb_device::{class_prelude::*, prelude::*};

// `SerialPort` implements the CDC-ACM serial class over usb-device.
use usbd_serial::SerialPort;

// ── Formatting imports ────────────────────────────────────────────────────────
//
// We format log lines into a fixed-size stack buffer — no heap allocation
// needed (and there is no heap without an allocator crate on bare metal).

// `heapless::String<N>` is a String backed by a `[u8; N]` array on the stack.
// It implements `core::fmt::Write` so the standard `write!` macro works.
use heapless::String as HString;

// The `Write` trait provides `write_fmt`, which is what `write!(buf, …)` calls.
// It must be in scope for the macro to compile.
use core::fmt::Write;

// ── Entry point ───────────────────────────────────────────────────────────────

/// Program entry point.  `-> !` means this function never returns — correct
/// for bare-metal where there is nothing to return to.
#[entry]
fn main() -> ! {
    // ── 1. Take ownership of all hardware peripherals ─────────────────────────
    //
    // The PAC guarantees exactly one owner per peripheral.  Calling `take()`
    // a second time returns `None`, so `.unwrap()` here is effectively a
    // compile-time guarantee: it can only panic if called twice in one program.
    let mut pac = pac::Peripherals::take().unwrap();
    // Note: we do NOT take CorePeripherals — the SysTick Delay is replaced
    // by the RP2040 hardware Timer below, so we never need SYST.

    // ── 2. Watchdog ───────────────────────────────────────────────────────────
    //
    // The watchdog peripheral is required by `init_clocks_and_plls` — it uses
    // the watchdog's tick generator to sequence PLL lock-up internally.
    // We are not enabling automatic watchdog resets here.
    let mut watchdog = Watchdog::new(pac.WATCHDOG);

    // ── 3. Clocks ─────────────────────────────────────────────────────────────
    //
    // This configures two PLLs:
    //   • PLL_SYS  →  125 MHz system clock  (CPU, AHB, APB, peripherals)
    //   • PLL_USB  →   48 MHz USB clock     (required for USB enumeration)
    //
    // IMPORTANT: If `init_clocks_and_plls` is not called before creating the
    // USB bus, the USB hardware has no reference clock and the host will never
    // detect the device.
    let clocks = init_clocks_and_plls(
        XOSC_CRYSTAL_FREQ, // 12 MHz crystal on the Pico PCB
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .ok()
    .unwrap();

    // ── 4. Hardware timer ─────────────────────────────────────────────────────
    //
    // The RP2040 has a 64-bit microsecond counter peripheral separate from
    // the Cortex-M SysTick.  We read it to decide when 500 ms have elapsed.
    //
    // WHY NOT delay_ms?
    //   `delay_ms(500)` blocks the CPU for half a second.  During that block,
    //   `usb_dev.poll()` is never called, so the USB host times out and
    //   re-enumerates the device every second — the serial port keeps
    //   disconnecting.  Reading the counter and looping is non-blocking, so
    //   the poll runs every iteration and USB stays happy.
    //
    // rp2040-hal 0.10 API: Timer::new takes THREE args — the third is &clocks.
    // The borrow of `clocks` ends immediately after new() returns (Timer stores
    // nothing), so `clocks.usb_clock` can safely be moved in the next step.
    let timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);

    // ── 5. USB bus allocator ──────────────────────────────────────────────────
    //
    // `UsbBusAllocator` wraps the hardware bus and parcels it out to USB
    // class instances (serial, HID, etc.) via shared references.
    //
    // Declaration order matters: `serial` and `usb_dev` below borrow from
    // `usb_bus`, and Rust drops locals in reverse declaration order, so the
    // borrowers are dropped before the bus — correct.
    //
    // `clocks.usb_clock` is moved (consumed) into the bus here.  This is a
    // field-level move; other fields of `clocks` remain accessible if needed.
    //
    // `force_vbus_detect_bit = true`: the standard Pico PCB has no VBUS
    // detection circuit, so we tell the USB controller to always assume 5 V
    // is present.  Without this the device won't enumerate from USB power.
    let usb_bus = UsbBusAllocator::new(UsbBus::new(
        pac.USBCTRL_REGS,
        pac.USBCTRL_DPRAM,
        clocks.usb_clock,
        true,
        &mut pac.RESETS,
    ));

    // ── 6. USB CDC serial port ────────────────────────────────────────────────
    //
    // `SerialPort` implements the CDC-ACM ("Communications Device Class –
    // Abstract Control Model") USB class.  This is what modern OSes recognise
    // as a virtual COM / tty port without needing extra drivers.
    let mut serial = SerialPort::new(&usb_bus);

    // ── 7. USB device descriptor ──────────────────────────────────────────────
    //
    // The descriptor tells the host who we are and what class we implement.
    //
    // VID 0x16c0 / PID 0x27dd: well-known "test" IDs (V-USB project).  The
    // CDC-ACM class driver ships with every modern OS so these IDs just work,
    // but they should NOT be used in production firmware shipped to customers.
    //
    // `.strings(…)` is the usb-device 0.3 builder API for device strings.
    // It returns Result<…>; the only error is >16 language entries, so
    // `.unwrap()` is safe here.
    //
    // `device_class(2)`: USB class code 0x02 = CDC, which tells the host to
    // load the CDC-ACM driver automatically.
    let mut usb_dev = UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x16c0, 0x27dd))
        .strings(&[StringDescriptors::default()
            .manufacturer("Mr Robot Lab")
            .product("Pico Serial")
            .serial_number("0001")])
        .unwrap()
        .device_class(usbd_serial::USB_CLASS_CDC)
        .build();

    // ── 8. GPIO: onboard LED ──────────────────────────────────────────────────
    let sio = Sio::new(pac.SIO);
    let pins = rp_pico::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );
    // GPIO 25 → onboard green LED on a standard Pico.
    // (On a Pico W, GPIO 25 goes to the wireless chip, not the LED.)
    let mut led_pin = pins.led.into_push_pull_output();

    // ── 9. State ──────────────────────────────────────────────────────────────
    let mut led_on = false;
    let mut tick: u32 = 0;

    // Seed the timer comparison with the current counter value.
    // `get_counter()` returns `TimerInstantU64` (a fugit Instant).
    // `.ticks()` extracts the raw u64 microsecond count.
    let mut last_toggle_us: u64 = timer.get_counter().ticks();

    // ── 10. Main loop ─────────────────────────────────────────────────────────
    //
    // The loop runs as fast as the CPU allows — no blocking anywhere.
    // Two things happen each iteration:
    //   a) USB is polled          → keeps enumeration alive, moves data
    //   b) Timestamp comparison   → toggles LED every 500 ms
    loop {
        // ── USB poll ──────────────────────────────────────────────────────────
        //
        // `poll()` must be called every loop iteration.  Internally it:
        //   • Handles control-endpoint traffic (SET_ADDRESS, GET_DESCRIPTOR…)
        //   • Copies received bytes from the USB FIFO into usbd-serial's buffer
        //   • Flushes any pending TX bytes to the host
        //
        // When `poll()` returns `true`, there are incoming bytes to read.
        // We drain them even if we don't use them — if the CDC RX buffer
        // fills up, the USB stack stalls our TX side too (flow control).
        if usb_dev.poll(&mut [&mut serial]) {
            let mut rx_buf = [0u8; 64];
            let _ = serial.read(&mut rx_buf);
        }

        // ── Non-blocking blink ────────────────────────────────────────────────
        //
        // Compare the current microsecond timestamp against the last toggle.
        // When the gap hits 500 000 µs (500 ms), toggle and log.
        //
        // `wrapping_sub` handles the theoretical timer rollover at 2^64 µs
        // (~585 000 years of uptime) — purely cosmetic safety.
        let now_us: u64 = timer.get_counter().ticks();
        if now_us.wrapping_sub(last_toggle_us) >= 500_000 {
            last_toggle_us = now_us;
            tick = tick.wrapping_add(1);
            led_on = !led_on;

            if led_on {
                led_pin.set_high().unwrap();
            } else {
                led_pin.set_low().unwrap();
            }

            // ── Write a log line to the serial port ───────────────────────────
            //
            // `HString<64>` is a heapless String backed by a 64-byte array.
            // The longest possible message is ~29 bytes ("tick 4294967295 -- LED off\r\n"),
            // so the buffer never overflows.
            //
            // `write!(…).ok()` discards fmt::Error; it can only occur on
            // overflow, which we've just ruled out.
            //
            // `serial.write()` returns Err(WouldBlock) when the host has no
            // terminal open (or the TX buffer is full).  We drop the result —
            // the LED keeps blinking whether or not anyone is listening.
            let state = if led_on { "on" } else { "off" };
            let mut buf: HString<64> = HString::new();
            write!(buf, "tick {} -- LED {}\r\n", tick, state).ok();
            let _ = serial.write(buf.as_bytes());
        }
    }
}
