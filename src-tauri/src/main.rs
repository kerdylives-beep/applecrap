#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // A previous version relaunched as a watchdog over a fresh update: no
    // window, no app — just see the update through or undo it.
    if let Some(code) = applecrap_alpha_lib::run_update_watchdog_if_requested() {
        std::process::exit(code);
    }
    applecrap_alpha_lib::run();
}
