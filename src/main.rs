#![no_std]
#![no_main]

use panic_halt as _;

use rp_pico::entry;
use rp_pico::hal;
use rp_pico::hal::pac;
use rp_pico::hal::clocks::init_clocks_and_plls;
use rp_pico::XOSC_CRYSTAL_FREQ;
use rp_pico::hal::sio::Sio;
use rp_pico::hal::watchdog::Watchdog;
use rp_pico::hal::pwm;

use embedded_hal::digital::v2::OutputPin;
use embedded_hal::PwmPin;

use hal::usb::UsbBus;
use usb_device::{class_prelude::*, prelude::*};
use usbd_serial::SerialPort;

use heapless::String as HString;
use core::fmt::Write;

// TB6612 truth table (channel A):
//   AIN1=H, AIN2=L → Forward
//   AIN1=L, AIN2=H → Reverse
//   AIN1=L, AIN2=L → Coast (free spin)
//   STBY must be HIGH while the driver is active
#[derive(Clone, Copy, PartialEq)]
enum Dir { Fwd, Rev, Coast }

// Automatic demo sequence, driven by hardware timer (no blocking delays).
#[derive(Clone, Copy, PartialEq)]
enum Step {
    RampFwd,  // 0→100% fwd, 2 s
    HoldFull, // 100% fwd,   1 s
    HoldLow,  //  30% fwd,   1 s
    CoastA,   // coast,      1 s
    Reverse,  //  50% rev,   2 s
    CoastB,   // coast,      1 s
}

impl Step {
    fn duration_us(self) -> u64 {
        match self {
            Step::RampFwd  => 2_000_000,
            Step::HoldFull => 1_000_000,
            Step::HoldLow  => 1_000_000,
            Step::CoastA   => 1_000_000,
            Step::Reverse  => 2_000_000,
            Step::CoastB   => 1_000_000,
        }
    }

    fn next(self) -> Self {
        match self {
            Step::RampFwd  => Step::HoldFull,
            Step::HoldFull => Step::HoldLow,
            Step::HoldLow  => Step::CoastA,
            Step::CoastA   => Step::Reverse,
            Step::Reverse  => Step::CoastB,
            Step::CoastB   => Step::RampFwd,
        }
    }
}

#[entry]
fn main() -> ! {
    let mut pac = pac::Peripherals::take().unwrap();
    let mut watchdog = Watchdog::new(pac.WATCHDOG);

    // PLL_SYS → 125 MHz, PLL_USB → 48 MHz (required for USB enumeration)
    let clocks = init_clocks_and_plls(
        XOSC_CRYSTAL_FREQ,
        pac.XOSC, pac.CLOCKS, pac.PLL_SYS, pac.PLL_USB,
        &mut pac.RESETS, &mut watchdog,
    )
    .ok()
    .unwrap();

    // 64-bit µs counter — used for all timing so we never block the USB poll
    let timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);

    // usb_bus must outlive serial and usb_dev (Rust drops in reverse order)
    let usb_bus = UsbBusAllocator::new(UsbBus::new(
        pac.USBCTRL_REGS, pac.USBCTRL_DPRAM,
        clocks.usb_clock,
        true, // force VBUS detect — Pico has no hardware VBUS sense pin
        &mut pac.RESETS,
    ));
    let mut serial  = SerialPort::new(&usb_bus);
    let mut usb_dev = UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x16c0, 0x27dd))
        .strings(&[StringDescriptors::default()
            .manufacturer("Mr Robot Lab")
            .product("Pico Motor Test")
            .serial_number("0001")])
        .unwrap()
        .device_class(usbd_serial::USB_CLASS_CDC)
        .build();

    let sio  = Sio::new(pac.SIO);
    let pins = rp_pico::Pins::new(pac.IO_BANK0, pac.PADS_BANK0, sio.gpio_bank0, &mut pac.RESETS);

    let mut led_pin = pins.led.into_push_pull_output();

    // STBY HIGH → driver enabled
    let mut stby = pins.gpio9.into_push_pull_output();
    stby.set_high().ok();

    // Motor A direction (coast until the sequence starts)
    let mut ain1 = pins.gpio2.into_push_pull_output();
    let mut ain2 = pins.gpio3.into_push_pull_output();

    // Motor B — reserved for later (pins parked low)
    let _bin1      = pins.gpio6.into_push_pull_output();
    let _bin2      = pins.gpio7.into_push_pull_output();
    let _pwmb_gpio = pins.gpio8.into_push_pull_output(); // slice 4 ch A reserved

    // Line sensor — reserved, not wired
    let _s = (
        pins.gpio10.into_floating_input(),
        pins.gpio11.into_floating_input(),
        pins.gpio12.into_floating_input(),
        pins.gpio13.into_floating_input(),
        pins.gpio14.into_floating_input(),
    );

    // PWMA on GP4 = slice 2 channel A. GP8 (PWMB) = slice 4 channel A (reserved).
    // Different slices → independent duties for each motor later.
    let mut pwm_slices = pwm::Slices::new(pac.PWM, &mut pac.RESETS);
    pwm_slices.pwm2.enable();
    let _pwma_pin = pwm_slices.pwm2.channel_a.output_to(pins.gpio4);
    pwm_slices.pwm2.channel_a.set_duty(0u16);

    let mut seq_step      = Step::RampFwd;
    let mut step_start_us = timer.get_counter().ticks();
    let mut last_dir      = Dir::Coast;
    let mut last_log_pct: u8 = 255; // sentinel → forces first log print

    let mut led_on      = false;
    let mut last_led_us = timer.get_counter().ticks();

    loop {
        // Poll USB every iteration — skipping causes the host to disconnect
        if usb_dev.poll(&mut [&mut serial]) {
            let mut rx = [0u8; 64];
            let _ = serial.read(&mut rx); // drain RX so TX flow control doesn't stall
        }

        let now_us = timer.get_counter().ticks();

        // 1 Hz LED heartbeat
        if now_us.wrapping_sub(last_led_us) >= 500_000 {
            last_led_us = now_us;
            led_on = !led_on;
            if led_on { led_pin.set_high().ok(); } else { led_pin.set_low().ok(); }
        }

        let elapsed = now_us.wrapping_sub(step_start_us);

        let (dir, pct): (Dir, u8) = match seq_step {
            Step::RampFwd  => (Dir::Fwd, ((elapsed.min(2_000_000) * 100) / 2_000_000) as u8),
            Step::HoldFull => (Dir::Fwd, 100),
            Step::HoldLow  => (Dir::Fwd,  30),
            Step::CoastA   => (Dir::Coast,  0),
            Step::Reverse  => (Dir::Rev,   50),
            Step::CoastB   => (Dir::Coast,  0),
        };

        // Apply TB6612 truth table
        match dir {
            Dir::Fwd   => { ain1.set_high().ok(); ain2.set_low().ok(); }
            Dir::Rev   => { ain1.set_low().ok();  ain2.set_high().ok(); }
            Dir::Coast => { ain1.set_low().ok();  ain2.set_low().ok(); }
        }
        pwm_slices.pwm2.channel_a.set_duty((pct as u32 * 65_535 / 100) as u16);

        // Log on direction change or every 10% duty step during ramp
        if dir != last_dir || (pct / 10) != (last_log_pct / 10) {
            last_dir     = dir;
            last_log_pct = pct;

            let t_sec   = now_us / 1_000_000;
            let t_tenth = (now_us % 1_000_000) / 100_000;
            let mut buf: HString<64> = HString::new();
            match dir {
                Dir::Coast => { write!(buf, "t={}.{}s  dir=STOP\r\n", t_sec, t_tenth).ok(); }
                Dir::Fwd   => { write!(buf, "t={}.{}s  dir=FWD  duty={}%\r\n", t_sec, t_tenth, pct).ok(); }
                Dir::Rev   => { write!(buf, "t={}.{}s  dir=REV  duty={}%\r\n", t_sec, t_tenth, pct).ok(); }
            }
            let _ = serial.write(buf.as_bytes());
        }

        // Advance to next step when duration expires; 255 sentinel forces a log on entry
        if elapsed >= seq_step.duration_us() {
            seq_step      = seq_step.next();
            step_start_us = now_us;
            last_log_pct  = 255;
        }
    }
}
