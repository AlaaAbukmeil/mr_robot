# pico-blink — Track C: Single-Motor PWM Bench Test

Drives one DC motor through a TB6612FNG motor driver under PWM speed/direction
control.  The firmware runs a fixed automatic sequence (ramp up, hold, slow, coast,
reverse, coast, repeat) and prints every state change to a USB CDC serial port.
The onboard LED blinks at 1 Hz as a heartbeat.  No interaction needed — just flash,
open the serial port, and watch the motor move.

---

## Hardware

- **Raspberry Pi Pico** (standard, not Pico W)
- **TB6612FNG** dual H-bridge motor driver
- One DC motor (secured to the bench — it will spin fast with no load)
- 7.4 V battery pack with a rocker switch
- USB-A to Micro-USB cable (powers the Pico and carries the serial log)

---

## Wiring — TB6612FNG motor driver

### Two separate power inputs — do NOT confuse them

| TB6612 pin | Connect to              | Purpose                          |
|------------|-------------------------|----------------------------------|
| `VM`       | Battery + (7.4 V) via rocker switch | Motor power (up to 15 V) |
| `VCC`      | Pico **3V3** (pin 36)   | Logic supply for the IC          |
| `GND`      | Battery −, Pico GND, common rail | Common ground — mandatory |

**Common ground is mandatory.** Without it the logic signals have no reference
and the motor behaves randomly or not at all.

The Pico is powered over USB during this test.  The battery powers `VM` only.
Their grounds must be tied together at the TB6612 `GND` terminal.

### Signal connections — Motor A (active in this test)

| Pico GPIO | TB6612 pin | Signal                     |
|-----------|------------|----------------------------|
| GP2       | AIN1       | Motor A direction bit 1    |
| GP3       | AIN2       | Motor A direction bit 2    |
| GP4       | PWMA       | Motor A speed (PWM)        |
| GP9       | STBY       | Driver enable (HIGH = on)  |

Motor leads → TB6612 `AO1` / `AO2`.

### Reserved pins (not wired in this test)

| Pico GPIO | Future use          |
|-----------|---------------------|
| GP6       | BIN1 — Motor B dir  |
| GP7       | BIN2 — Motor B dir  |
| GP8       | PWMB — Motor B speed (PWM slice 4) |
| GP10–GP14 | Line sensor OUT1–OUT5 |

---

## TB6612 channel A truth table

The firmware uses Forward, Reverse, and Coast.  Brake is not used.

| AIN1 | AIN2 | PWMA | Motor A behaviour        |
|------|------|------|--------------------------|
| H    | L    | duty | **Forward**              |
| L    | H    | duty | **Reverse**              |
| L    | L    | any  | **Coast** (free spin)    |
| H    | H    | any  | Brake (motor shorted)    |

`STBY` must be HIGH whenever the motor is active.  LOW puts the whole IC to sleep.

---

## Prerequisites

```sh
rustup target add thumbv6m-none-eabi
cargo install elf2uf2-rs --locked
```

---

## Build

```sh
cargo build --release
```

---

## Flash (BOOTSEL / UF2)

1. Hold **BOOTSEL** on the Pico.
2. Plug the USB cable into the Pico (while still holding BOOTSEL).
3. Release BOOTSEL — the board mounts as `RPI-RP2`.
4. Run:
   ```sh
   cargo run --release
   ```
   `elf2uf2-rs -d` converts the ELF to UF2, finds the `RPI-RP2` drive, and
   copies it automatically.
5. The Pico reboots.  `RPI-RP2` disappears; a USB serial device appears.
6. **Switch the battery on** so `VM` is powered before the motor sequence starts.

---

## Motor test sequence (automatic, no interaction)

The firmware loops this sequence forever:

| Step | Direction | Duty | Duration |
|------|-----------|------|----------|
| 1    | Forward   | 0 → 100 % ramp | 2 s |
| 2    | Forward   | 100 %          | 1 s |
| 3    | Forward   | 30 %           | 1 s |
| 4    | Coast     | —              | 1 s |
| 5    | Reverse   | 50 %           | 2 s |
| 6    | Coast     | —              | 1 s |
| 7    | Repeat    |                |     |

One full cycle = **8 seconds**.

---

## Open the serial port

USB CDC ignores the configured baud rate — any value works.

### Linux

```sh
ls /dev/ttyACM*
tio /dev/ttyACM0
# or
screen /dev/ttyACM0 115200   # Ctrl-A then K to quit
```

**Permission denied?**
```sh
sudo usermod -aG dialout $USER   # then log out and back in
```

### macOS

```sh
ls /dev/tty.usbmodem*
screen /dev/tty.usbmodem* 115200
```

### Windows

Device Manager → **Ports (COM & LPT)** → note the `COMx` number →
open **PuTTY**: Connection type Serial, line `COMx`, speed 115200.

---

## Expected serial output

```
t=0.0s  dir=FWD  duty=0%
t=0.2s  dir=FWD  duty=10%
t=0.4s  dir=FWD  duty=20%
...
t=1.8s  dir=FWD  duty=90%
t=2.0s  dir=FWD  duty=100%
t=3.0s  dir=FWD  duty=30%
t=4.0s  dir=STOP
t=5.0s  dir=REV  duty=50%
t=7.0s  dir=STOP
t=8.0s  dir=FWD  duty=0%
...
```

---

## Troubleshooting

**Motor doesn't move at all.**
- `STBY` must be HIGH — check GP9 is wired and the firmware started (LED blinking?).
- `VM` must be powered — is the rocker switch on and the battery charged?
- Common ground — is the battery − tied to Pico GND at the TB6612 `GND` pin?

**Motor spins one direction only / won't reverse.**
- `AIN1` or `AIN2` is swapped or disconnected — recheck GP2 → AIN1, GP3 → AIN2.

**Pico resets or serial drops when the motor starts.**
- Motor inrush current is causing noise on the power rail.
- Confirm motor power comes from the battery on `VM`, not through the Pico USB.
- Keep grounds tied together.
- A 100 µF capacitor across `VM`/`GND` close to the TB6612 helps with inrush.

**No serial device appears.**
- Confirm the board is no longer mounted as `RPI-RP2` (flashing failed if it is).
- `init_clocks_and_plls` configures the 48 MHz USB clock — if it didn't run,
  the USB hardware has no reference clock and the host never enumerates.

**Serial device appears but no text.**
- Open the terminal before resetting the Pico, or power-cycle the board with the
  terminal already open.  The firmware only writes on motor state changes.

**Linux: screen leaves the port locked.**
```sh
screen -ls          # find the session ID
screen -X -S <id> quit
```
`tio` cleans up automatically on exit.

**Windows: no COM port in Device Manager.**
Windows 10 / 11 includes a built-in CDC-ACM driver — it should install silently.
If you see "Unknown Device", right-click → Update driver → search automatically.
