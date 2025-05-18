// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install default rustls provider");

    unstoppableswap_gui_rs_lib::run()
}
