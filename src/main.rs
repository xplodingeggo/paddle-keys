#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod config;
mod decode;
mod device;
mod keys;
mod steam;
mod tray;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::w;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--probe") {
        // Release builds have no console; attach to the parent's so output shows.
        unsafe {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
        let secs = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);
        if let Err(e) = device::probe(secs) {
            eprintln!("probe failed: {e}");
            std::process::exit(1);
        }
        return;
    }

    // Held until process exit; a second instance sees ERROR_ALREADY_EXISTS.
    let _mutex = unsafe { CreateMutexW(None, false, w!(r"Local\paddle-keys")) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return;
    }

    let path = std::env::var_os("APPDATA")
        .map(|a| PathBuf::from(a).join("paddle-keys").join("config.toml"));
    let bindings = path.as_deref().map(config::load).unwrap_or_default();
    let (stop, paused) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let state = steam::State::load();
    let poller = {
        let (s, p, st) = (stop.clone(), paused.clone(), state.clone());
        std::thread::spawn(move || device::run(keys::Output::new(bindings), s, p, st))
    };
    {
        let (s, st) = (stop.clone(), state.clone());
        std::thread::spawn(move || steam::run(st, s));
    }
    tray::run(paused, stop, poller, path, state);
}
