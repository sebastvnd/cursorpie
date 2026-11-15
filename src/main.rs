//! Binary entry point. All logic lives in the `cursorpie` library crate (see `src/lib.rs`).

// Release builds on Windows are tray apps: no console window. (Debug builds keep it for diagnostics.)
// This is a crate-level attribute of the binary: it cannot live anywhere else, and it is the only
// OS condition outside `platform` and `tap`.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> eframe::Result<()> {
    cursorpie::run()
}
