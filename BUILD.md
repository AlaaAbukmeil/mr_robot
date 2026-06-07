# Pico Blink — Build & Flash Guide

Blinks the onboard LED on a **Raspberry Pi Pico (RP2040)** at 1 Hz to verify
the embedded Rust toolchain works end to end.

> **Board assumed:** Plain Raspberry Pi Pico with RP2040.
> The onboard LED is wired to **GPIO 25** on this board.
>
> | Board | Onboard LED | Works? |
> |---|---|---|
> | Raspberry Pi Pico (standard) | GPIO 25 | ✅ this project targets this |
> | Pico W | CYW43 wireless chip | ❌ GPIO 25 does nothing — ask before proceeding |
> | Pico 2 / RP2350 | GPIO 25 | ❌ different chip, different target triple |

---

## 1 — One-time setup

### 1.1  Install Rust

If Rust is not already installed:

**Linux / macOS:**
```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**Windows:** Download and run the installer from <https://rustup.rs>.

Restart your terminal after installation so `cargo` and `rustup` are on `PATH`.

### 1.2  Add the Cortex-M0+ cross-compilation target

The RP2040 uses an ARM Cortex-M0+ core.  Tell Rust's toolchain to compile for it:

```sh
rustup target add thumbv6m-none-eabi
```

> `rust-toolchain.toml` in this repo also declares the target, so rustup will
> install it automatically the first time you run any `cargo` command here.

### 1.3  Install the UF2 flash tool

```sh
cargo install elf2uf2-rs --locked
```

`elf2uf2-rs` converts the ELF binary the compiler produces into the `.uf2`
format the Pico's USB bootloader accepts.  The `-d` flag (already set in
`.cargo/config.toml`) then copies the file to the board automatically.

---

## 2 — Put the Pico into BOOTSEL mode

**You must do this every time you want to flash new firmware.**

1. Unplug the USB cable from the Pico (or from your computer — either end).
2. **Hold down the BOOTSEL button.**
   It is the single small button on the Pico PCB, located near the USB connector.
3. While still holding BOOTSEL, plug the USB cable back in.
4. **Release** the BOOTSEL button.

The board should now appear on your computer as a USB mass-storage drive
named **`RPI-RP2`** (similar to a USB thumb drive).

If it does not appear within a few seconds, unplug and try again — it is easy
to release the button a fraction too early.

> **Why does this work?**
> The RP2040 has a small ROM bootloader built into the chip itself.  Holding
> BOOTSEL at power-on tells the ROM to stay in bootloader mode and present
> itself as a mass-storage device over USB.  When a valid `.uf2` file is
> written to the drive the ROM verifies it, writes it to flash, and reboots
> automatically into your program.

---

## 3 — Build and flash (automatic — recommended)

With the board in BOOTSEL mode and the `RPI-RP2` drive visible:

```sh
cargo run --release
```

What happens step by step:

1. `cargo build --release` compiles `src/main.rs` for `thumbv6m-none-eabi`.
2. The linker produces `target/thumbv6m-none-eabi/release/pico-blink` (ELF).
3. The **runner** configured in `.cargo/config.toml` (`elf2uf2-rs -d`):
   - converts the ELF to a `.uf2` file,
   - finds the `RPI-RP2` drive and copies the file onto it.
4. The board ejects, reboots, and runs your code.

If everything worked, the onboard LED should start blinking once per second.

> You can also run `cargo run` (debug) for faster compilation iteration.
> The binary is larger and the build slower, but the blink will still work.

---

## 4 — Manual flash path

If `cargo run` fails at the copy step (wrong drive letter, permission issue,
`RPI-RP2` not found), you can do it manually:

```sh
# Step 1: build only
cargo build --release

# Step 2: convert ELF → UF2
#   Windows path:  target\thumbv6m-none-eabi\release\pico-blink
#   Linux/macOS:   target/thumbv6m-none-eabi/release/pico-blink
elf2uf2-rs target/thumbv6m-none-eabi/release/pico-blink pico-blink.uf2
```

Then open your file manager, find the **`RPI-RP2`** drive, and drag
`pico-blink.uf2` onto it.  The board will reboot automatically.

---

## 5 — Reflashing after a code change

There is no way to re-enter BOOTSEL mode from software in this project —
you must use the hardware button each time.  (A later phase adds a debug
probe which allows flashing without this step.)

1. Put the Pico into BOOTSEL mode again (unplug → hold BOOTSEL → replug → release).
2. `cargo run --release`

---

## 6 — Project structure

```
mr-robot/
├── .cargo/
│   └── config.toml       # default target + elf2uf2-rs runner
├── src/
│   └── main.rs           # blink program, heavily commented
├── Cargo.toml            # dependencies and build profiles
├── rust-toolchain.toml   # pins stable toolchain + thumbv6m target
└── BUILD.md              # this file
```

**Why is there no `memory.x` in this project?**
The `rp2040-hal` crate ships its own `memory.x` (defining the RP2040's
2 MB flash + 264 KB SRAM layout) and copies it to the linker search path
via its Cargo build script.  `cortex-m-rt`'s `link.x` linker script then
`INCLUDE`s it automatically.  You only need a local `memory.x` if you are
using a bare `rp2040-hal` without the BSP, or if you want to customise the
memory layout (e.g. to reserve a region for persistent storage).

---

## 7 — Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| `RPI-RP2` drive does not appear | Released BOOTSEL too early | Unplug and try again |
| `error: linker 'arm-none-eabi-ld' not found` | Wrong linker invocation | Check `.cargo/config.toml` rustflags |
| `elf2uf2-rs: no device found` | Pico not in BOOTSEL mode | Put board into BOOTSEL mode first |
| LED does not blink | Pico W board | GP25 is routed to the wireless chip on Pico W — this blink won't work |
| `cannot find crate for 'std'` | Missing `#![no_std]` or wrong target | Verify target is `thumbv6m-none-eabi` |
| Version conflict errors | Dependency mismatch | Run `cargo update` then `cargo build` |
