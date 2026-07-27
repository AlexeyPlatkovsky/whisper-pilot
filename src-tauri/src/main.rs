// Prevent an extra console window on Windows in release.
// Gated to target_os so it does not warn on Linux CI.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

fn main() {
    whisperpilot_lib::run()
}
