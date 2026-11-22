#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

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

#[cfg(windows)]
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
#[cfg(windows)]
use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
#[cfg(windows)]
use windows::Win32::System::Threading::CreateMutexW;
#[cfg(windows)]
use windows::core::w;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--probe") {
        // Release builds have no console; attach to the parent's so output shows.
        #[cfg(windows)]
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

    #[cfg(target_os = "linux")]
    if let Some(cmd) = args.get(1).and_then(|a| a.strip_prefix("--")).filter(|a| a.ends_with("-steam-hook")) {
        let r = match cmd {
            "install-steam-hook" => steam::install_hook(),
            "remove-steam-hook" => steam::remove_hook(),
            _ => Err(format!("unknown option --{cmd}")),
        };
        match r {
            Ok(msg) => println!("{msg}"),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }

    // Held until process exit; a second instance sees ERROR_ALREADY_EXISTS.
    #[cfg(windows)]
    let _mutex = unsafe { CreateMutexW(None, false, w!(r"Local\paddle-keys")) };
    #[cfg(windows)]
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return;
    }
    #[cfg(target_os = "linux")]
    let Some(_lock) = single_instance() else {
        return;
    };

    let path = config_dir().map(|d| d.join("config.toml"));
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

/// %APPDATA%\paddle-keys on Windows, $XDG_CONFIG_HOME/paddle-keys (or ~/.config/paddle-keys) on Linux.
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "linux")]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    base.map(|b| b.join("paddle-keys"))
}

/// An flock held for the life of the process; None when another copy holds it.
#[cfg(target_os = "linux")]
fn single_instance() -> Option<std::fs::File> {
    use std::os::fd::AsRawFd;
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join("paddle-keys.lock")).ok()?;
    (unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0).then_some(f)
}
