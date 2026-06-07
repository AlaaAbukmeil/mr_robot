//! Build script for pico-blink.
//!
//! `cortex-m-rt`'s `link.x` does `INCLUDE memory.x`.  The linker searches
//! only the directories listed via `-L`; without adding our project root
//! here the linker exits with "cannot find linker script memory.x".
//!
//! In older versions of rp2040-hal the crate's own build.rs did this job.
//! Starting with rp2040-hal 0.10 / rp-pico 0.9 it was removed, so we do
//! it ourselves.

fn main() {
    // Re-run this script if memory.x changes.
    println!("cargo:rerun-if-changed=memory.x");

    // Add the crate root (the directory that contains memory.x) to the
    // linker search path.
    println!(
        "cargo:rustc-link-search={}",
        std::env::current_dir().unwrap().display()
    );
}
