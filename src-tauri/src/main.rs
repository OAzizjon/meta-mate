// No console window in release builds on Windows; keep this line.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    metamate_lib::run()
}
