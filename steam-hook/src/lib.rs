//! Loaded into steam.exe by paddle-keys. Steam reads Xbox pads through Valve's steamxbox driver and
//! hands the packets to its bundled SDL3. This hook makes SDL's GIP (Xbox One) driver own the 8BitDo
//! Ultimate 2 as an Xbox Elite 2, so Steam Input gets real paddle buttons:
//!  - 0x2ae808 device info reply: VID/PID 2DC8:310B -> 045E:0B00
//!  - 0x2ae80c interface reply: class ff/5d/01 (Xbox 360) -> ff/47/d0 (GIP); endpoints untouched
//!  - 0x2aec04 reads: 360 reports -> GIP input with the paddles from paddle-keys, plus hello and guide
//!  - 0x2aac08 writes: GIP rumble -> 360 rumble; other GIP commands never reach the pad
//!  - while enabled, SDL ignores 2DC8:310B, so the pad's own XInput and DirectInput entries don't
//!    list it again next to the Elite (SDL only hides those when a HIDAPI device shares their VID/PID)
//! Everything is keyed to the pad's 8-byte sideband id, so other controllers are never touched.

pub mod gip;
pub mod shm;

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use windows_sys::Win32::Foundation::{
    BOOL, ERROR_ALREADY_EXISTS, ERROR_IO_PENDING, GetLastError, HANDLE, HMODULE, SetLastError, TRUE,
};
use windows_sys::Win32::System::IO::OVERLAPPED;
use windows_sys::Win32::System::LibraryLoader::{DisableThreadLibraryCalls, GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Memory::{FILE_MAP_ALL_ACCESS, MapViewOfFile, OpenFileMappingW};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::{CreateMutexW, CreateThread, GetCurrentProcessId, Sleep};

use gip::{INPUT_MSG, PREFIX, Pad360};
use shm::Shared;

const IOCTL_INFO: u32 = 0x2ae808;
const IOCTL_IFACE: u32 = 0x2ae80c;
const IOCTL_READ: u32 = 0x2aec04;
const IOCTL_WRITE: u32 = 0x2aac08;
const STATUS_PENDING: usize = 0x103;

type DeviceIoControlFn =
    unsafe extern "system" fn(HANDLE, u32, *const c_void, u32, *mut c_void, u32, *mut u32, *mut OVERLAPPED) -> BOOL;
type GetOverlappedResultFn = unsafe extern "system" fn(HANDLE, *const OVERLAPPED, *mut u32, BOOL) -> BOOL;
type SetHintWithPriorityFn = unsafe extern "C" fn(*const u8, *const u8, i32) -> bool;
type ResetHintFn = unsafe extern "C" fn(*const u8) -> bool;

static ORIG_DIOC: AtomicUsize = AtomicUsize::new(0);
static ORIG_GOR: AtomicUsize = AtomicUsize::new(0);
static SHM: AtomicPtr<Shared> = AtomicPtr::new(std::ptr::null_mut());
static HINT_SET: AtomicBool = AtomicBool::new(false);
/// 0 unset, 1 ignoring 2DC8:310B, 2 not ignoring
static IGNORING: AtomicU8 = AtomicU8::new(0);

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Info,
    Iface,
    Read,
}

struct Pending {
    ovl: usize,
    buf: usize,
    cap: u32,
    kind: Kind,
}

struct Pad {
    id: [u8; 8],
    seq: u8,
    need_hello: bool,
    guide: bool,
    last: Option<[u8; INPUT_MSG]>,
    rumble: (u8, u8),
}

impl Pad {
    fn new(id: [u8; 8]) -> Pad {
        Pad { id, seq: 0, need_hello: true, guide: false, last: None, rumble: (0, 0) }
    }

    fn next_seq(&mut self) -> u8 {
        self.seq = self.seq.wrapping_add(1);
        self.seq
    }
}

static PENDING: Mutex<Vec<Pending>> = Mutex::new(Vec::new());
static PADS: Mutex<Vec<Pad>> = Mutex::new(Vec::new());

// A panic must never unwind into Steam, so poisoned locks are taken over rather than unwrapped.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn guard(f: impl FnOnce()) {
    let _ = catch_unwind(AssertUnwindSafe(f));
}

fn shared() -> Option<&'static Shared> {
    let p = SHM.load(Ordering::Acquire);
    // SAFETY: the view is mapped once and never unmapped.
    let s = unsafe { p.as_ref()? };
    (s.magic.load(Ordering::Relaxed) == shm::MAGIC && s.version.load(Ordering::Relaxed) == shm::VERSION).then_some(s)
}

fn enabled() -> bool {
    shared().is_some_and(|s| s.enabled.load(Ordering::Relaxed) == 1)
}

fn current_paddles() -> u8 {
    let Some(s) = shared() else { return 0 };
    let age = unsafe { GetTickCount64() }.saturating_sub(s.heartbeat.load(Ordering::Relaxed));
    if age > shm::HEARTBEAT_MS { 0 } else { (s.paddles.load(Ordering::Relaxed) & 0x0f) as u8 }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn ensure_hint() {
    if HINT_SET.load(Ordering::Relaxed) {
        return;
    }
    unsafe {
        let sdl = GetModuleHandleW(wide("SDL3.dll").as_ptr());
        if sdl.is_null() {
            return;
        }
        let Some(f) = GetProcAddress(sdl, c"SDL_SetHintWithPriority".as_ptr().cast()) else { return };
        let set: SetHintWithPriorityFn = std::mem::transmute(f);
        // Without this SDL resets the pad when it never answers the metadata request; with it SDL
        // fakes the metadata. OVERRIDE so Steam's own hint calls cannot undo it.
        if set(c"SDL_JOYSTICK_HIDAPI_GIP_RESET_FOR_METADATA".as_ptr().cast(), c"0".as_ptr().cast(), 2) {
            HINT_SET.store(true, Ordering::Relaxed);
        }
    }
}

/// Follows the enabled flag. Steam sets no ignore list of its own, so the hint is ours to reset.
fn sync_ignore() {
    let want = if enabled() { 1 } else { 2 };
    if IGNORING.load(Ordering::Relaxed) == want {
        return;
    }
    unsafe {
        let sdl = GetModuleHandleW(wide("SDL3.dll").as_ptr());
        if sdl.is_null() {
            return;
        }
        let name = c"SDL_GAMECONTROLLER_IGNORE_DEVICES".as_ptr().cast();
        let done = if want == 1 {
            let Some(f) = GetProcAddress(sdl, c"SDL_SetHintWithPriority".as_ptr().cast()) else { return };
            let set: SetHintWithPriorityFn = std::mem::transmute(f);
            set(name, c"0x2DC8/0x310B".as_ptr().cast(), 2)
        } else {
            let Some(f) = GetProcAddress(sdl, c"SDL_ResetHint".as_ptr().cast()) else { return };
            let reset: ResetHintFn = std::mem::transmute(f);
            reset(name)
        };
        if done {
            IGNORING.store(want, Ordering::Relaxed);
        }
    }
}

fn read_id(buf: &[u8]) -> Option<[u8; 8]> {
    buf.get(..8)?.try_into().ok()
}

fn tracked(id: &[u8; 8]) -> bool {
    lock(&PADS).iter().any(|p| &p.id == id)
}

/// Device info reply: take over the 8BitDo by presenting it as an Elite 2.
fn patch_info(buf: &mut [u8]) {
    let Some(id) = read_id(buf) else { return };
    if buf.len() < 12 || buf[8..12] != [0xc8, 0x2d, 0x0b, 0x31] {
        return;
    }
    let mut pads = lock(&PADS);
    if !pads.iter().any(|p| p.id == id) {
        if !enabled() {
            return;
        }
        ensure_hint();
        sync_ignore();
        pads.push(Pad::new(id));
    }
    buf[8..12].copy_from_slice(&[0x5e, 0x04, 0x00, 0x0b]);
}

/// Interface reply: report the Xbox One (GIP) class so SDL's GIP driver owns the pad.
fn patch_iface(buf: &mut [u8]) {
    if let Some(id) = read_id(buf)
        && buf.len() >= 13
        && buf[10..13] == [0xff, 0x5d, 0x01]
        && tracked(&id)
    {
        buf[11..13].copy_from_slice(&[0x47, 0xd0]);
    }
}

fn put(out: &mut [u8; 64], msg: &[u8]) -> usize {
    out[..msg.len()].copy_from_slice(msg);
    msg.len()
}

/// One sideband read: replace the 360 message with the GIP message SDL expects. Returns the new length.
fn translate_read(buf: &mut [u8], n: usize) -> Option<usize> {
    let id = read_id(buf)?;
    if n < PREFIX + 2 || n > buf.len() {
        return None;
    }
    let paddles = current_paddles();
    let mut pads = lock(&PADS);
    let pad = pads.iter_mut().find(|p| p.id == id)?;
    let mut out = [0u8; 64];
    let len = if pad.need_hello {
        pad.need_hello = false;
        let seq = pad.next_seq();
        if let Some(s) = shared() {
            s.hook_generation.store(s.generation.load(Ordering::Relaxed), Ordering::Relaxed);
        }
        put(&mut out, &gip::hello(seq))
    } else if let Some(p) = Pad360::parse(&buf[PREFIX..n]) {
        let seq = pad.next_seq();
        let input = gip::input(seq, &p, paddles);
        pad.last = Some(input);
        if p.guide() != pad.guide {
            // One read carries one message; the input above goes out with the next read.
            pad.guide = p.guide();
            put(&mut out, &gip::guide(seq, pad.guide))
        } else {
            put(&mut out, &input)
        }
    } else {
        // LED, status and other 360 messages would be misread as GIP; repeat the last input instead.
        let seq = pad.next_seq();
        let neutral = Pad360 { b2: 0, b3: 0, lt: 0, rt: 0, sticks: [0; 8] };
        let last = pad.last.unwrap_or_else(|| gip::input(seq, &neutral, 0));
        put(&mut out, &gip::restamp(last, seq, paddles))
    };
    drop(pads);
    buf.get_mut(PREFIX..PREFIX + len)?.copy_from_slice(&out[..len]);
    if let Some(s) = shared() {
        s.hook_frames.fetch_add(1, Ordering::Relaxed);
    }
    Some(PREFIX + len)
}

/// One write to the pad: GIP rumble becomes 360 rumble, anything else repeats the current rumble.
fn translate_write(inp: &[u8]) -> Option<Vec<u8>> {
    let id = read_id(inp)?;
    if inp.len() < PREFIX + 4 {
        return None;
    }
    let mut pads = lock(&PADS);
    let pad = pads.iter_mut().find(|p| p.id == id)?;
    if let Some(levels) = gip::rumble_levels(&inp[PREFIX..]) {
        pad.rumble = levels;
    }
    let mut out = inp[..PREFIX].to_vec();
    out[9] = (PREFIX + 8) as u8; // total size
    out.extend_from_slice(&gip::rumble_360(pad.rumble.0, pad.rumble.1));
    Some(out)
}

/// SAFETY: `buf` must be valid for `cap` bytes.
unsafe fn process(kind: Kind, buf: usize, cap: u32, count: &mut u32) {
    if buf == 0 {
        return;
    }
    let n = (*count).min(cap) as usize;
    let all = unsafe { std::slice::from_raw_parts_mut(buf as *mut u8, cap as usize) };
    match kind {
        Kind::Info => patch_info(&mut all[..n]),
        Kind::Iface => patch_iface(&mut all[..n]),
        Kind::Read => {
            if let Some(len) = translate_read(all, n) {
                *count = len as u32;
            }
        }
    }
}

fn take_pending(ovl: usize) -> Option<Pending> {
    let mut v = lock(&PENDING);
    let i = v.iter().position(|p| p.ovl == ovl)?;
    Some(v.swap_remove(i))
}

#[allow(clippy::too_many_arguments)]
unsafe extern "system" fn device_io_control(
    h: HANDLE,
    code: u32,
    inb: *const c_void,
    insz: u32,
    outb: *mut c_void,
    outsz: u32,
    ret: *mut u32,
    ovl: *mut OVERLAPPED,
) -> BOOL {
    let orig: DeviceIoControlFn = unsafe { std::mem::transmute(ORIG_DIOC.load(Ordering::Relaxed)) };
    if code == IOCTL_WRITE && !inb.is_null() {
        let mut replaced = None;
        guard(|| {
            let inp = unsafe { std::slice::from_raw_parts(inb as *const u8, insz as usize) };
            replaced = translate_write(inp);
        });
        // METHOD_BUFFERED: the input is copied during the call, so a local buffer is fine.
        if let Some(b) = replaced {
            return unsafe { orig(h, code, b.as_ptr().cast(), b.len() as u32, outb, outsz, ret, ovl) };
        }
        return unsafe { orig(h, code, inb, insz, outb, outsz, ret, ovl) };
    }
    let kind = match code {
        IOCTL_INFO => Kind::Info,
        IOCTL_IFACE => Kind::Iface,
        IOCTL_READ => Kind::Read,
        _ => return unsafe { orig(h, code, inb, insz, outb, outsz, ret, ovl) },
    };
    if !ovl.is_null() {
        let mut v = lock(&PENDING);
        v.retain(|p| p.ovl != ovl as usize);
        v.push(Pending { ovl: ovl as usize, buf: outb as usize, cap: outsz, kind });
    }
    let r = unsafe { orig(h, code, inb, insz, outb, outsz, ret, ovl) };
    let err = unsafe { GetLastError() };
    if r != 0 {
        let _ = take_pending(ovl as usize);
        if !ret.is_null() {
            guard(|| unsafe { process(kind, outb as usize, outsz, &mut *ret) });
        }
    } else if err == ERROR_IO_PENDING && kind != Kind::Read && !ovl.is_null() {
        // Steam may read the reply without GetOverlappedResult, so wait briefly and patch it here.
        // Only the Internal status is polled; Steam's event is left alone.
        guard(|| {
            for i in 0..150 {
                let o = unsafe { std::ptr::read_volatile(ovl) };
                if o.Internal != STATUS_PENDING {
                    if let Some(p) = take_pending(ovl as usize) {
                        let mut n = o.InternalHigh as u32;
                        unsafe { process(p.kind, p.buf, p.cap, &mut n) };
                    }
                    break;
                }
                unsafe { Sleep(if i < 50 { 0 } else { 1 }) };
            }
        });
    }
    unsafe { SetLastError(err) };
    r
}

unsafe extern "system" fn get_overlapped_result(h: HANDLE, ovl: *const OVERLAPPED, n: *mut u32, wait: BOOL) -> BOOL {
    let orig: GetOverlappedResultFn = unsafe { std::mem::transmute(ORIG_GOR.load(Ordering::Relaxed)) };
    let r = unsafe { orig(h, ovl, n, wait) };
    if r != 0 && !n.is_null() {
        let err = unsafe { GetLastError() };
        if let Some(p) = take_pending(ovl as usize) {
            guard(|| unsafe { process(p.kind, p.buf, p.cap, &mut *n) });
        }
        unsafe { SetLastError(err) };
    }
    r
}

unsafe fn hook(module: &str, name: &std::ffi::CStr, detour: *mut c_void, orig: &AtomicUsize) -> bool {
    unsafe {
        let m: HMODULE = GetModuleHandleW(wide(module).as_ptr());
        if m.is_null() {
            return false;
        }
        let Some(target) = GetProcAddress(m, name.as_ptr().cast()) else { return false };
        match minhook::MinHook::create_hook(target as *mut c_void, detour) {
            Ok(trampoline) => {
                orig.store(trampoline as usize, Ordering::Relaxed);
                true
            }
            Err(_) => false,
        }
    }
}

fn map_shared() {
    if !SHM.load(Ordering::Relaxed).is_null() {
        return;
    }
    unsafe {
        let h = OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, wide(shm::NAME).as_ptr());
        if h.is_null() {
            return;
        }
        let v = MapViewOfFile(h, FILE_MAP_ALL_ACCESS, 0, 0, std::mem::size_of::<Shared>());
        if !v.Value.is_null() {
            SHM.store(v.Value.cast(), Ordering::Release);
        }
        // The handle stays open so the mapping outlives a paddle-keys restart.
    }
}

fn init() {
    unsafe {
        let name = wide(&format!(r"Local\paddle-keys-steam-hook-{}", GetCurrentProcessId()));
        CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return; // another copy of the hook is already active in this process
        }
        let ok = hook("kernel32.dll", c"DeviceIoControl", device_io_control as *mut c_void, &ORIG_DIOC)
            && hook("kernelbase.dll", c"GetOverlappedResult", get_overlapped_result as *mut c_void, &ORIG_GOR);
        if !ok || minhook::MinHook::enable_all_hooks().is_err() {
            return;
        }
        loop {
            map_shared();
            ensure_hint();
            sync_ignore();
            if let Some(s) = shared() {
                s.hook_pid.store(GetCurrentProcessId(), Ordering::Relaxed);
            }
            Sleep(500);
        }
    }
}

unsafe extern "system" fn init_thread(_: *mut c_void) -> u32 {
    guard(init);
    0
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
unsafe extern "system" fn DllMain(module: HMODULE, reason: u32, _: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        unsafe {
            DisableThreadLibraryCalls(module);
            // Nothing else under the loader lock.
            CreateThread(std::ptr::null(), 0, Some(init_thread), std::ptr::null(), 0, std::ptr::null_mut());
        }
    }
    TRUE
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: [u8; 8] = [0x23, 0x64, 0x1b, 0x37, 0x13, 0xc0, 0x00, 0x00];

    fn frame(msg: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8; 269];
        b[..8].copy_from_slice(&ID);
        b[8..13].copy_from_slice(&[0x84, 0x0d, 0x01, 0x00, 0x00]);
        b[13..13 + msg.len()].copy_from_slice(msg);
        b
    }

    // One test, so the parallel test runner never races the shared PADS state.
    #[test]
    fn read_and_write_translation() {
        *lock(&PADS) = vec![Pad::new(ID)];
        let report = [0x00, 0x14, 0x00, 0x10, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0, 0, 0, 0];

        // first frame becomes the hello
        let mut b = frame(&report);
        assert_eq!(translate_read(&mut b, 33), Some(13 + 32));
        assert_eq!(b[13..17], [0x02, 0x20, 1, 0x1c]);
        assert_eq!(b[..13], frame(&[])[..13], "prefix untouched");

        // then GIP input with A held
        let mut b = frame(&report);
        assert_eq!(translate_read(&mut b, 33), Some(13 + 51));
        assert_eq!(b[13..17], [0x20, 0x00, 2, 47]);
        assert_eq!(b[17], 0x10);

        // guide press becomes a guide message
        let mut g = report;
        g[3] |= 0x04;
        let mut b = frame(&g);
        assert_eq!(translate_read(&mut b, 33), Some(13 + 6));
        assert_eq!(b[13..19], [0x07, 0x20, 3, 0x02, 1, 0x5b]);

        // a 360 LED message repeats the last input
        let mut b = frame(&[0x01, 0x03, 0x06]);
        assert_eq!(translate_read(&mut b, 16), Some(13 + 51));
        assert_eq!((b[13], b[17]), (0x20, 0x10));

        // untracked pads pass through
        let mut other = frame(&report);
        other[0] = 0xaa;
        assert_eq!(translate_read(&mut other, 33), None);

        // GIP rumble -> 360 rumble; an LED command repeats it
        let mut w = frame(&[0x09, 0x00, 0x00, 0x09, 0x00, 0x0f, 0x00, 0x00, 0x64, 0x00, 0xff, 0x00, 0xeb]);
        w.truncate(26);
        w[8..13].copy_from_slice(&[0x05, 0x1a, 0, 0, 0]);
        let out = translate_write(&w).unwrap();
        assert_eq!(out.len(), 21);
        assert_eq!(out[8..10], [0x05, 21]);
        assert_eq!(out[13..], [0x00, 0x08, 0x00, 255, 0, 0, 0, 0]);
        let mut led = w.clone();
        led[13..17].copy_from_slice(&[0x0e, 0x00, 0x00, 0x05]);
        assert_eq!(translate_write(&led).unwrap()[13..], [0x00, 0x08, 0x00, 255, 0, 0, 0, 0]);

        // interface class rewrite only for tracked pads
        let mut iface = frame(&[]);
        iface[8..16].copy_from_slice(&[0, 0, 0xff, 0x5d, 0x01, 0x02, 0x84, 0x05]);
        patch_iface(&mut iface[..44]);
        assert_eq!(iface[10..13], [0xff, 0x47, 0xd0]);
        iface[0] = 0xaa;
        iface[11..13].copy_from_slice(&[0x5d, 0x01]);
        patch_iface(&mut iface[..44]);
        assert_eq!(iface[11..13], [0x5d, 0x01]);
    }
}
