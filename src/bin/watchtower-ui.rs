//! The desktop entry point, independent of the console command lifecycle.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    watchtower::app::run();
}
