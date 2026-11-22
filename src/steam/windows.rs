//! Windows: injects the hook DLL into steam.exe (CreateRemoteThread + LoadLibraryW).

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, STILL_ACTIVE};
use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW, PROCESSENTRY32W, Process32FirstW,
    Process32NextW, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_ALL_ACCESS, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, MapViewOfFile, PAGE_READWRITE,
    VirtualAllocEx, VirtualFreeEx,
};
use windows::Win32::System::Services::{
    CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatus, SC_MANAGER_CONNECT, SERVICE_QUERY_STATUS,
    SERVICE_RUNNING, SERVICE_STATUS,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::{
    CreateRemoteThread, GetExitCodeProcess, GetExitCodeThread, OpenProcess, PROCESS_CREATE_THREAD,
    PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ,
    PROCESS_VM_WRITE, WaitForSingleObject,
};
use windows::core::{HSTRING, s, w};
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};


use super::{State, Status, setting_on, shm};

const DLL: &str = "paddle_keys_steam.dll";
const SETTINGS: &str = r"Software\paddle-keys";

/// (Steam Elite mode on, paddle mode)
pub fn load_settings() -> (bool, u8) {
    let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(SETTINGS, KEY_READ).ok();
    let get = |name: &str| key.as_ref().and_then(|k| k.get_value::<u32, _>(name).ok());
    (get("SteamElite") == Some(1), get("PaddleMode").unwrap_or(0) as u8)
}

pub fn save_settings(enabled: bool, mode: u8) {
    if let Ok((key, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey_with_flags(SETTINGS, KEY_WRITE) {
        let _ = key.set_value("SteamElite", &(enabled as u32));
        let _ = key.set_value("PaddleMode", &(mode as u32));
    }
}

pub struct Link {
    pub s: &'static shm::Shared,
}

impl Link {
    pub fn open() -> Option<Link> {
        unsafe {
            let h = CreateFileMappingW(INVALID_HANDLE_VALUE, None, PAGE_READWRITE, 0, 4096, &HSTRING::from(shm::NAME)).ok()?;
            let v = MapViewOfFile(h, FILE_MAP_ALL_ACCESS, 0, 0, 0);
            // The mapping lives for the whole process; the handle is intentionally never closed.
            let s = (v.Value as *const shm::Shared).as_ref()?;
            // A hook from an earlier run may still hold the mapping; keep its counters.
            if s.magic.load(Ordering::Relaxed) != shm::MAGIC {
                s.generation.store(1, Ordering::Relaxed);
                s.version.store(shm::VERSION, Ordering::Relaxed);
                s.magic.store(shm::MAGIC, Ordering::Release);
            }
            Some(Link { s })
        }
    }

    pub fn publish(&self, paddles: u32) {
        self.s.paddles.store(paddles, Ordering::Relaxed);
        self.s.heartbeat.store(unsafe { GetTickCount64() }, Ordering::Relaxed);
    }

    /// The hook took over the controller's current connection, in a Steam that is still running.
    pub fn live(&self) -> bool {
        let g = self.s.generation.load(Ordering::Relaxed);
        g != 0 && self.s.hook_generation.load(Ordering::Relaxed) == g && alive(self.s.hook_pid.load(Ordering::Relaxed))
    }

    pub fn hook_loaded(&self) -> bool {
        alive(self.s.hook_pid.load(Ordering::Relaxed))
    }
}

fn alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code).is_ok() && code == STILL_ACTIVE.0 as u32;
        let _ = CloseHandle(h);
        ok
    }
}

struct Snapshot(HANDLE);

impl Drop for Snapshot {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn name_of(raw: &[u16]) -> String {
    let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
    String::from_utf16_lossy(&raw[..end])
}

fn steam_pids() -> Vec<u32> {
    let mut pids = Vec::new();
    unsafe {
        let Ok(h) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return pids };
        let snap = Snapshot(h);
        let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap.0, &mut e).is_ok();
        while ok {
            if name_of(&e.szExeFile).eq_ignore_ascii_case("steam.exe") {
                pids.push(e.th32ProcessID);
            }
            ok = Process32NextW(snap.0, &mut e).is_ok();
        }
    }
    pids
}

/// Some(true) when a copy of the hook is loaded in `pid`; None when the module list can't be read yet.
fn has_hook(pid: u32) -> Option<bool> {
    unsafe {
        let snap = Snapshot(CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid).ok()?);
        let mut e = MODULEENTRY32W { dwSize: size_of::<MODULEENTRY32W>() as u32, ..Default::default() };
        let mut ok = Module32FirstW(snap.0, &mut e).is_ok();
        let mut any = false;
        while ok {
            any = true;
            if name_of(&e.szModule).to_ascii_lowercase().starts_with("paddle_keys_steam") {
                return Some(true);
            }
            ok = Module32NextW(snap.0, &mut e).is_ok();
        }
        any.then_some(false)
    }
}

/// Loads `dll` into `pid` with LoadLibraryW on a remote thread.
fn inject(pid: u32, dll: &Path) -> Result<(), String> {
    let path: Vec<u16> = dll.as_os_str().encode_wide_nul();
    let bytes = path.len() * 2;
    unsafe {
        let access = PROCESS_CREATE_THREAD | PROCESS_QUERY_INFORMATION | PROCESS_VM_OPERATION | PROCESS_VM_WRITE | PROCESS_VM_READ;
        let proc = OpenProcess(access, false, pid).map_err(|e| e.to_string())?;
        let result = (|| {
            let mem = VirtualAllocEx(proc, None, bytes, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
            if mem.is_null() {
                return Err("VirtualAllocEx failed".to_string());
            }
            let r = (|| {
                WriteProcessMemory(proc, mem, path.as_ptr() as *const c_void, bytes, None).map_err(|e| e.to_string())?;
                // kernel32 sits at the same address in every process of a session.
                let k32 = GetModuleHandleW(w!("kernel32.dll")).map_err(|e| e.to_string())?;
                let load = GetProcAddress(k32, s!("LoadLibraryW")).ok_or("LoadLibraryW not found")?;
                let start: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(load);
                let th = CreateRemoteThread(proc, None, 0, Some(start), Some(mem), 0, None).map_err(|e| e.to_string())?;
                WaitForSingleObject(th, 10_000);
                let mut module = 0u32;
                let _ = GetExitCodeThread(th, &mut module);
                let _ = CloseHandle(th);
                if module == 0 { Err("LoadLibraryW returned NULL".to_string()) } else { Ok(()) }
            })();
            let _ = VirtualFreeEx(proc, mem, 0, MEM_RELEASE);
            r
        })();
        let _ = CloseHandle(proc);
        result
    }
}

trait EncodeWideNul {
    fn encode_wide_nul(&self) -> Vec<u16>;
}

impl EncodeWideNul for std::ffi::OsStr {
    fn encode_wide_nul(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain([0]).collect()
    }
}

/// A loaded DLL is locked until Steam exits, so Steam gets a per-build copy and updates never fail.
fn staged_dll() -> Option<PathBuf> {
    let src = std::env::current_exe().ok()?.with_file_name(DLL);
    // keyed on content: rebuilds often keep the same padded PE size, and a loaded copy can't be overwritten
    let mut h = std::hash::DefaultHasher::new();
    std::hash::Hasher::write(&mut h, &std::fs::read(&src).ok()?);
    let dir = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("paddle-keys");
    let dst = dir.join(format!("paddle_keys_steam-{}-{:016x}.dll", env!("CARGO_PKG_VERSION"), std::hash::Hasher::finish(&h)));
    if !dst.exists() {
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::copy(&src, &dst).ok()?;
    }
    Some(dst)
}

fn driver_running() -> bool {
    unsafe {
        let Ok(scm) = OpenSCManagerW(None, None, SC_MANAGER_CONNECT) else { return false };
        let mut running = false;
        if let Ok(svc) = OpenServiceW(scm, w!("steamxbox"), SERVICE_QUERY_STATUS) {
            let mut st = SERVICE_STATUS::default();
            running = QueryServiceStatus(svc, &mut st).is_ok() && st.dwCurrentState == SERVICE_RUNNING;
            let _ = CloseServiceHandle(svc);
        }
        let _ = CloseServiceHandle(scm);
        running
    }
}

fn xbox_setting_on() -> bool {
    let Ok(steam) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam") else { return false };
    let Ok(dir) = steam.get_value::<String, _>("SteamPath") else { return false };
    let dir = Path::new(&dir);
    // Newer Steam keeps it per user in userdata\<id>\config\localconfig.vdf, older in config\config.vdf.
    let users = std::fs::read_dir(dir.join("userdata"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path().join("config").join("localconfig.vdf"));
    std::iter::once(dir.join("config").join("config.vdf"))
        .chain(users)
        .any(|p| std::fs::read_to_string(p).is_ok_and(|t| setting_on(&t)))
}

/// Injects the hook into every steam.exe while Steam Elite mode is on and keeps the status current.
pub fn run(state: Arc<State>, stop: Arc<AtomicBool>) {
    let mut done: HashSet<u32> = HashSet::new();
    // pid -> (attempts, last attempt); a just-started steam.exe can refuse the first tries
    let mut failed: HashMap<u32, (u32, Instant)> = HashMap::new();
    let mut setup = (false, false);
    let mut checked = Instant::now() - Duration::from_secs(60);
    while !stop.load(Ordering::Relaxed) {
        let enabled = state.enabled.load(Ordering::Relaxed);
        if let Some(l) = &state.link {
            l.s.enabled.store(enabled as u32, Ordering::Relaxed);
        }
        let status = if !enabled {
            Status::Off
        } else {
            if checked.elapsed() >= Duration::from_secs(5) {
                setup = (driver_running(), xbox_setting_on());
                checked = Instant::now();
            }
            let pids = steam_pids();
            done.retain(|p| pids.contains(p));
            failed.retain(|p, _| pids.contains(p));
            for &pid in &pids {
                if done.contains(&pid) || failed.get(&pid).is_some_and(|&(n, t)| n >= 5 || t.elapsed() < Duration::from_secs(2)) {
                    continue;
                }
                match has_hook(pid) {
                    Some(true) => {
                        done.insert(pid);
                    }
                    Some(false) => match staged_dll().ok_or("hook DLL missing".to_string()).and_then(|d| inject(pid, &d)) {
                        Ok(()) => {
                            done.insert(pid);
                        }
                        Err(_) => {
                            let n = failed.get(&pid).map_or(0, |f| f.0);
                            failed.insert(pid, (n + 1, Instant::now()));
                        }
                    },
                    None => {}
                }
            }
            // The setup checks only explain why it is not live; they never block a live takeover.
            if state.link.as_ref().is_some_and(Link::live) {
                Status::Active
            } else if !setup.0 {
                Status::NeedDriver
            } else if !setup.1 {
                Status::NeedSetting
            } else if pids.is_empty() {
                Status::NoSteam
            } else if done.is_empty() && failed.values().any(|f| f.0 >= 5) {
                Status::InjectFailed
            } else if state.link.as_ref().is_some_and(Link::hook_loaded) {
                Status::Reconnect
            } else {
                Status::NoSteam
            }
        };
        state.live.store(status == Status::Active, Ordering::Relaxed);
        state.status.store(status as u8, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(250));
    }
}

