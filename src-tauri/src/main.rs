// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().nth(1).as_deref() == Some("qoder-companion-hook") {
        wb_switch_core::modules::qoder::companion::run_hook(
            std::env::args().nth(2).as_deref() == Some("ai"),
        );
        return;
    }
    wb_switch_rust_lib::run()
}
