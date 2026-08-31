//! Entry point only; every command and the `Builder` live in `lib.rs` so the
//! mock-runtime smoke test can build the exact same app (spec §11, task 0015).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    fm_desktop::run();
}
