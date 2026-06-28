# pico-blink — Track A.5: USB Serial Logging

The Pico blinks its onboard LED at 1 Hz and, on every toggle, prints a log
line to a USB CDC serial port.  No debug probe required — the same USB cable
used for power and flashing is used for logging.

```
tick 1 -- LED on
tick 2 -- LED off
tick 3 -- LED on
...
```

---

## Hardware

- **Raspberry Pi Pico** (standard, not Pico W — the Pico W wires GPIO 25 to the
  wireless chip rather than the LED).
- A USB-A to Micro-USB cable.
- A laptop/desktop running Linux, macOS, or Windows 10+.

---

## Prerequisites

```sh
# Install the bare-metal Rust target for Cortex-M0+
rustup target add thumbv6m-none-eabi

# Install the UF2 flashing tool (one-time)
cargo install elf2uf2-rs --locked
```

---

## Build

```sh
cargo build --release
```

---

## Flash (BOOTSEL / UF2 method)

1. **Hold the BOOTSEL button** on the Pico.
2. **Plug the USB cable** into the Pico (while still holding BOOTSEL).
3. **Release BOOTSEL** — the board mounts as a mass-storage drive called `RPI-RP2`.
4. Run:
   ```sh
   cargo run --release
   ```
   `elf2uf2-rs -d` (the Cargo runner) converts the ELF to UF2, finds the
   `RPI-RP2` drive, and copies the firmware onto it automatically.
5. The Pico reboots.  The `RPI-RP2` drive disappears and a USB serial device
   appears in its place.  The LED starts blinking.

---

## Open the serial port

USB CDC ignores the configured baud rate — any value works.

### Linux

```sh
# Find the device
ls /dev/ttyACM*

# Open (Ctrl-A then K to quit screen, or Ctrl-] for tio)
tio /dev/ttyACM0
# or
screen /dev/ttyACM0 115200
```

**Permission denied?** Add yourself to the `dialout` group and re-login:
```sh
sudo usermod -aG dialout $USER
```

### macOS

```sh
ls /dev/tty.usbmodem*
screen /dev/tty.usbmodem* 115200
```

### Windows

1. Open **Device Manager** → **Ports (COM & LPT)** — note the `COMx` number.
2. Open **PuTTY** → Connection type: **Serial** → Serial line: `COMx` → Speed: `115200` → Open.
   - Or use the Windows Terminal / PowerShell approach below:
     ```powershell
     # List COM ports
     [System.IO.Ports.SerialPort]::GetPortNames()
     # Then open with any terminal app (PuTTY, Tera Term, etc.)
     ```

---

## Expected output

After the port is open you should see one line per second:

```
tick 1 -- LED on
tick 2 -- LED off
tick 3 -- LED on
tick 4 -- LED off
```

---

## Troubleshooting

**No serial device appears after flashing.**
Confirm the board is no longer mounted as `RPI-RP2` (if it is, the flash failed
— retry the BOOTSEL procedure).  The USB clock is sourced from `init_clocks_and_plls`
which is called first in `main`; if that call were missing, the USB hardware
would have no reference clock and would never enumerate.

**Device enumerates but no text appears.**
Open the serial terminal *before* resetting the Pico, or reset the board
after the terminal is already open.  The firmware only writes on a blink
toggle; you may have missed the first few ticks while opening the port.

**Text was streaming but then stopped.**
The main loop's `serial.read()` drain keeps the CDC RX buffer clear.
If you removed that call, the USB stack's flow control stalls the TX path
once the RX buffer fills.  Make sure the drain is present in the loop.

**Linux: `screen` leaves the port locked.**
Kill a stuck `screen` session with `screen -ls` then `screen -X -S <id> quit`,
or just use `tio` which cleans up automatically.

**Windows: no COM port appears in Device Manager.**
Windows 10 and 11 include a built-in CDC-ACM driver that installs silently.
If you see an "Unknown Device" instead, right-click → Update driver → let
Windows search automatically.
