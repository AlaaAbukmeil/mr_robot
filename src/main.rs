// ── Crate-level attributes ────────────────────────────────────────────────────
//
// #![no_std]
//   We are running on bare metal — there is no operating system beneath us.
//   Rust's standard library (`std`) assumes an OS for things like heap
//   allocation, threads, file I/O, and environment variables.  None of those
//   exist on a microcontroller, so we tell the compiler not to link `std`.
//   We still have access to `core`, which is the OS-agnostic subset of the
//   Rust library (math, iterators, slices, formatting, etc.).
#![no_std]
//
// #![no_main]
//   In a normal Rust binary the OS calls `main()` after doing some C runtime
//   set-up.  On bare metal there is no OS, so we handle the entry point
//   ourselves via the `#[entry]` attribute below.  `#![no_main]` tells the
//   compiler not to expect the standard `fn main()` signature.
#![no_main]

// ── Crate imports ─────────────────────────────────────────────────────────────

// `panic_halt` provides a minimal panic handler.
// When Rust code panics (e.g. an `.unwrap()` on `None`), it calls this
// handler instead of unwinding the stack.  `panic_halt` simply spins in a
// tight loop forever, which is the safest bare-metal option — no terminal,
// no reset, just a safe stop.  The `as _` means "import for its side-effect
// (registering the panic handler) without binding it to a name."
use panic_halt as _;

// `entry` is re-exported from `cortex-m-rt`.  Applying `#[entry]` to a
// function:
//   • Marks it as the CPU's reset handler (the first function that runs
//     after power-on or reset).
//   • Generates the ARM vector table entry that the ROM bootloader reads
//     to find our code.
//   • Runs cortex-m-rt's startup stub (zeroes BSS, copies .data) before
//     transferring control to our function.
use rp_pico::entry;

// HAL (Hardware Abstraction Layer) items from the rp2040-hal crate,
// accessed through the rp-pico BSP re-export.
use rp_pico::hal::{
    // `init_clocks_and_plls` configures the RP2040's clock tree.
    // Without this the chip runs on its slow internal ring oscillator
    // (~6 MHz).  After calling it we get the full 125 MHz system clock.
    //
    // `Clock` is a trait that adds the `.freq()` method to clock structs.
    // We need it in scope to call `clocks.system_clock.freq()` below.
    clocks::{init_clocks_and_plls, Clock},

    // PAC = Peripheral Access Crate.  Auto-generated from the RP2040
    // datasheet, it gives us type-safe structs for every hardware register.
    // We use it to "take" exclusive ownership of peripherals — you can only
    // call `.take()` once; a second call returns `None` (compile-time safety).
    pac,

    // SIO = Single-cycle IO block.  The RP2040's high-speed GPIO controller.
    // We pass it to `Pins::new` to get access to individual GPIO pins.
    sio::Sio,

    // Watchdog timer hardware.  The clock-init function requires it as an
    // argument (it uses the watchdog to sequence PLL lock-up).  We don't
    // configure the watchdog for automatic resets here.
    watchdog::Watchdog,
};

// The RP2040 has a 12 MHz crystal (XOSC) on the Pico PCB.
// `XOSC_CRYSTAL_FREQ` is the constant `12_000_000_u32` defined in the BSP.
// We pass it to `init_clocks_and_plls` so the PLL can calculate the correct
// multiplier to reach 125 MHz.
use rp_pico::XOSC_CRYSTAL_FREQ;

// `OutputPin` is an embedded-hal trait that provides `set_high()` and
// `set_low()`.  Trait methods are only callable when the trait is in scope,
// even if you never write `OutputPin::` explicitly — Rust needs it to
// resolve the method call.
use embedded_hal::digital::v2::OutputPin;

// `DelayMs` is an embedded-hal trait that provides `delay_ms(ms: u32)`.
// `cortex_m::delay::Delay` implements this trait using the SysTick timer.
use embedded_hal::blocking::delay::DelayMs;

// ── Entry point ───────────────────────────────────────────────────────────────

/// The program entry point.
///
/// The `-> !` return type means "this function never returns."  On bare metal
/// there is nothing to return to — if `main` returned, the CPU would execute
/// whatever garbage is in memory next.  We prevent that with an infinite loop.
#[entry]
fn main() -> ! {
    // ── 1. Take ownership of all hardware peripherals ─────────────────────────
    //
    // The PAC enforces that each peripheral is owned by exactly one place in
    // the code at a time.  `Peripherals::take()` returns `Some(p)` the very
    // first time it is called and `None` on every subsequent call.
    // `.unwrap()` turns `None` into a panic (→ halt).  In practice this
    // can only be `None` if you call `take()` twice, which would be a
    // programming error — so `.unwrap()` is correct here.
    let mut pac = pac::Peripherals::take().unwrap();

    // CorePeripherals are the Cortex-M standard peripherals (SysTick, NVIC,
    // etc.) as opposed to the RP2040-specific ones above.
    let core = pac::CorePeripherals::take().unwrap();

    // ── 2. Initialise the watchdog (required by clock init) ───────────────────
    let mut watchdog = Watchdog::new(pac.WATCHDOG);

    // ── 3. Start the system clocks ────────────────────────────────────────────
    //
    // This configures the Phase-Locked Loop (PLL) so the CPU runs at 125 MHz
    // instead of the default ~6 MHz ring oscillator.  A correct clock is
    // important for accurate delays and USB timing.
    //
    // `.ok()` converts the `Result` to `Option`, and `.unwrap()` halts if
    // clock setup failed.  On real hardware with a good crystal this will
    // always succeed.
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

    // ── 4. Set up the SysTick-based delay ─────────────────────────────────────
    //
    // `Delay::new` takes the SysTick hardware counter and the system clock
    // frequency (in Hz).  It uses these to calculate how many SysTick ticks
    // equal one millisecond, giving us accurate blocking delays.
    //
    // `.to_Hz()` converts the fugit `HertzU32` clock frequency to a plain u32.
    let mut delay = cortex_m::delay::Delay::new(core.SYST, clocks.system_clock.freq().to_Hz());

    // ── 5. Configure GPIO pins ────────────────────────────────────────────────
    //
    // SIO (Single-cycle IO) is the RP2040's dedicated GPIO controller.
    let sio = Sio::new(pac.SIO);

    // `rp_pico::Pins` is a struct defined by the BSP that gives every GPIO
    // a human-readable name matching the Pico's silkscreen.  Under the hood
    // it configures IO_BANK0 and PADS_BANK0 — the RP2040 hardware blocks
    // that control pin multiplexing and electrical characteristics.
    let pins = rp_pico::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    // `pins.led` is the BSP alias for GPIO 25.
    // On a standard Raspberry Pi Pico, GPIO 25 is connected directly to
    // the onboard green LED.  (On a Pico W it connects to the wireless chip
    // instead — blink would NOT work on a Pico W without modification.)
    //
    // `.into_push_pull_output()` reconfigures the pin as a digital output in
    // "push-pull" mode: the pin actively drives 3.3 V (high) or 0 V (low).
    // This is the standard mode for driving an LED.
    let mut led_pin = pins.led.into_push_pull_output();

    // ── 6. Blink loop ─────────────────────────────────────────────────────────
    //
    // `loop` with no break is an infinite loop.  On bare metal this is normal:
    // the CPU must always be executing something.  If `main` returned, the
    // behaviour is undefined (the CPU would run off into random memory).
    loop {
        // Drive GPIO 25 to 3.3 V → current flows through the LED → LED ON.
        // `.unwrap()` is safe here: `set_high` on an rp2040-hal output pin
        // returns `Ok(())` unconditionally (it's an infallible operation).
        led_pin.set_high().unwrap();

        // Block for 500 ms.  Combined with the 500 ms below, the total
        // period is 1000 ms = 1 Hz blink rate.
        delay.delay_ms(500_u32);

        // Drive GPIO 25 to 0 V → no current → LED OFF.
        led_pin.set_low().unwrap();

        delay.delay_ms(500_u32);

        // Back to the top: LED ON again.  This repeats forever.
    }
}
