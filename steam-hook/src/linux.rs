//! Linux: an rtld-audit module (LD_AUDIT) that Steam loads at launch, added to steam.sh by
//! paddle-keys. Steam on Linux reads the 8BitDo through its bundled SDL3's evdev backend (kernel
//! xpad). Through `la_symbind`, the hook catches only the libc calls libSDL3 makes (`open`,
//! `ioctl`, `read`, `close`) and, for the 8BitDo's event node, makes it look like a wired Xbox
//! Elite Series 2 the way xpad presents one:
//!  - `EVIOCGID`: 2DC8:310B -> 045E:0B00 (bus and version kept)
//!  - `EVIOCGNAME`: "Xbox Elite Wireless Controller Series 2"
//!  - `EVIOCGBIT(EV_KEY)`: adds BTN_GRIPR, BTN_GRIPR2, BTN_GRIPL, BTN_GRIPL2 (xpad's Elite paddles,
//!    which SDL maps to paddle1..4)
//!  - `EVIOCGKEY` and `read`: the paddle state paddle-keys shares, as those four keys
//!
//! Everything else, rumble included, goes to xpad unchanged. Only fds opened while Elite mode was
//! enabled are touched; other processes (games, Proton, the web helper) get no bindings at all.

use std::ffi::{CStr, c_char, c_int, c_long, c_uint, c_ulong, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicI32, AtomicPtr, AtomicU32, AtomicUsize, Ordering};

use crate::shm::{self, Shared};

const LAV_CURRENT: c_uint = 2;
const LA_FLG_BINDTO: c_uint = 0x01;
const LA_FLG_BINDFROM: c_uint = 0x02;
const LA_SYMB_NOPLTENTER: c_uint = 0x01;
const LA_SYMB_NOPLTEXIT: c_uint = 0x02;
const LM_ID_BASE: c_long = 0;

const VID: u16 = 0x2DC8;
const PID: u16 = 0x310B;
const ELITE_VID: u16 = 0x045E;
const ELITE_PID: u16 = 0x0B00;
const ELITE_NAME: &[u8] = b"Xbox Elite Wireless Controller Series 2";

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const SYN_REPORT: u16 = 0;
/// shm bit order P1..P4 (upper right, lower right, upper left, lower left), as xpad reports them.
pub const PADDLE_KEYS: [u16; 4] = [0x225 /* BTN_GRIPR */, 0x227 /* BTN_GRIPR2 */, 0x224 /* BTN_GRIPL */, 0x226 /* BTN_GRIPL2 */];

// ioctl numbers: dir << 30 | size << 16 | 'E' << 8 | nr
const IOC_READ: c_ulong = 2;
const IOC_WRITE: c_ulong = 1;
const fn ioc_nr(req: c_ulong) -> c_ulong {
    req & 0xff
}
const fn ioc_type(req: c_ulong) -> c_ulong {
    (req >> 8) & 0xff
}
const fn ioc_size(req: c_ulong) -> usize {
    ((req >> 16) & 0x3fff) as usize
}
const fn ioc_dir(req: c_ulong) -> c_ulong {
    (req >> 30) & 0x3
}
const EVIOC_GID: c_ulong = 0x02;
const EVIOC_GNAME: c_ulong = 0x06;
const EVIOC_GKEY: c_ulong = 0x18;
const EVIOC_GBIT_KEY: c_ulong = 0x20 + EV_KEY as c_ulong;
const EVIOC_SCLOCKID: c_ulong = 0xa0;

/// What an ioctl on a tracked fd needs after the real call succeeded.
#[derive(Debug, PartialEq, Eq)]
pub enum Patch {
    Id,
    Name(usize),
    KeyBits(usize),
    KeyState(usize),
    ClockId,
    None,
}

pub fn classify(req: c_ulong) -> Patch {
    if ioc_type(req) != b'E' as c_ulong {
        return Patch::None;
    }
    match (ioc_dir(req), ioc_nr(req)) {
        (IOC_READ, EVIOC_GID) if ioc_size(req) == 8 => Patch::Id,
        (IOC_READ, EVIOC_GNAME) => Patch::Name(ioc_size(req)),
        (IOC_READ, EVIOC_GKEY) => Patch::KeyState(ioc_size(req)),
        (IOC_READ, EVIOC_GBIT_KEY) => Patch::KeyBits(ioc_size(req)),
        (IOC_WRITE, EVIOC_SCLOCKID) => Patch::ClockId,
        _ => Patch::None,
    }
}

/// struct input_id: bustype, vendor, product, version.
pub fn patch_id(id: &mut [u16; 4]) {
    id[1] = ELITE_VID;
    id[2] = ELITE_PID;
}

/// Writes the Elite's name like the kernel does (NUL-terminated, truncated to `len`); returns the
/// ioctl's result, the number of bytes written.
pub fn patch_name(buf: &mut [u8]) -> c_int {
    if buf.is_empty() {
        return 0;
    }
    let n = ELITE_NAME.len().min(buf.len() - 1);
    buf[..n].copy_from_slice(&ELITE_NAME[..n]);
    buf[n] = 0;
    (n + 1) as c_int
}

/// Sets the bits of the four paddle keys whose shm bit is in `paddles` (all four for 0x0f) in a
/// kernel key bitmap (bit n is byte n / 8, bit n % 8).
pub fn set_key_bits(bits: &mut [u8], paddles: u32) {
    for (i, &code) in PADDLE_KEYS.iter().enumerate() {
        let (byte, bit) = (code as usize / 8, code % 8);
        if paddles & (1 << i) != 0 && byte < bits.len() {
            bits[byte] |= 1 << bit;
        }
    }
}

const BTN_SOUTH: usize = 0x130;

/// A key bitmap from EVIOCGBIT(EV_KEY) that has the gamepad A button.
pub fn is_gamepad(bits: &[u8]) -> bool {
    bits.get(BTN_SOUTH / 8).is_some_and(|b| b & (1 << (BTN_SOUTH % 8)) != 0)
}

/// The kernel's struct input_event for this ABI (struct timeval is two longs).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputEvent {
    pub sec: c_long,
    pub usec: c_long,
    pub kind: u16,
    pub code: u16,
    pub value: i32,
}

/// Key events for each paddle that changed from `old` to `new`, then a SYN_REPORT. Empty if none.
pub fn paddle_events(old: u32, new: u32, sec: c_long, usec: c_long) -> ([InputEvent; 5], usize) {
    let mut out = [InputEvent { sec, usec, kind: EV_SYN, code: SYN_REPORT, value: 0 }; 5];
    let mut n = 0;
    for (i, &code) in PADDLE_KEYS.iter().enumerate() {
        let bit = 1 << i;
        if (old ^ new) & bit != 0 {
            out[n] = InputEvent { sec, usec, kind: EV_KEY, code, value: (new & bit != 0) as i32 };
            n += 1;
        }
    }
    if n > 0 {
        n += 1; // the SYN_REPORT already in place
    }
    (out, n)
}

// ---- runtime state ---------------------------------------------------------------------------

type OpenFn = unsafe extern "C" fn(*const c_char, c_int, c_uint) -> c_int;
type OpenatFn = unsafe extern "C" fn(c_int, *const c_char, c_int, c_uint) -> c_int;
type IoctlFn = unsafe extern "C" fn(c_int, c_ulong, *mut c_void) -> c_int;
type ReadFn = unsafe extern "C" fn(c_int, *mut c_void, usize) -> isize;
type CloseFn = unsafe extern "C" fn(c_int) -> c_int;

static REAL_OPEN: AtomicUsize = AtomicUsize::new(0);
static REAL_OPEN64: AtomicUsize = AtomicUsize::new(0);
static REAL_OPENAT: AtomicUsize = AtomicUsize::new(0);
static REAL_OPENAT64: AtomicUsize = AtomicUsize::new(0);
static REAL_IOCTL: AtomicUsize = AtomicUsize::new(0);
static REAL_READ: AtomicUsize = AtomicUsize::new(0);
static REAL_CLOSE: AtomicUsize = AtomicUsize::new(0);

/// The link_map of Steam's libSDL3, the only object whose bindings we take.
static SDL_COOKIE: AtomicUsize = AtomicUsize::new(0);
static SHM: AtomicPtr<Shared> = AtomicPtr::new(std::ptr::null_mut());

const SLOTS: usize = 16;
/// Tracked fds (-1 free), the paddles each was last told about, and its evdev clock.
static FDS: [AtomicI32; SLOTS] = [const { AtomicI32::new(-1) }; SLOTS];
static SENT: [AtomicU32; SLOTS] = [const { AtomicU32::new(0) }; SLOTS];
static CLOCK: [AtomicI32; SLOTS] = [const { AtomicI32::new(libc::CLOCK_REALTIME) }; SLOTS];

fn guard<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(fallback)
}

fn slot(fd: c_int) -> Option<usize> {
    if fd < 0 {
        return None;
    }
    FDS.iter().position(|s| s.load(Ordering::Relaxed) == fd)
}

fn is_steam() -> bool {
    static STEAM: OnceLock<bool> = OnceLock::new();
    *STEAM.get_or_init(|| {
        std::fs::read_link("/proc/self/exe").is_ok_and(|exe| {
            let s = exe.to_string_lossy();
            s.ends_with("/ubuntu12_32/steam") || s.ends_with("/ubuntu12_64/steam")
        })
    })
}

fn log(msg: &str) {
    use std::io::Write;
    let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") else { return };
    let path = std::path::Path::new(&dir).join("paddle-keys-hook.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} pid {} {msg}", shm::now_ms(), std::process::id());
    }
}

/// The shared memory paddle-keys created, mapped on first use; None while paddle-keys never ran.
fn shared() -> Option<&'static Shared> {
    let mut p = SHM.load(Ordering::Acquire);
    if p.is_null() {
        let path = std::ffi::CString::new(shm::path()).ok()?;
        // our own namespace's libc, not the hooked one
        unsafe {
            let fd = libc::open(path.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC);
            if fd < 0 {
                return None;
            }
            let m = libc::mmap(std::ptr::null_mut(), 4096, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0);
            libc::close(fd);
            if m == libc::MAP_FAILED {
                return None;
            }
            p = m as *mut Shared;
        }
        if let Err(other) = SHM.compare_exchange(std::ptr::null_mut(), p, Ordering::AcqRel, Ordering::Acquire) {
            unsafe { libc::munmap(p as *mut c_void, 4096) };
            p = other;
        }
    }
    let s = unsafe { &*p };
    (s.magic.load(Ordering::Relaxed) == shm::MAGIC && s.version.load(Ordering::Relaxed) == shm::VERSION).then_some(s)
}

/// Paddles to show Steam now: none if paddle-keys stopped publishing.
fn current_paddles() -> u32 {
    let Some(s) = shared() else { return 0 };
    let age = shm::now_ms().saturating_sub(s.heartbeat.load(Ordering::Relaxed));
    if age > shm::HEARTBEAT_MS { 0 } else { s.paddles.load(Ordering::Relaxed) & 0x0f }
}

unsafe fn real_ioctl(fd: c_int, req: c_ulong, arg: *mut c_void) -> c_int {
    match REAL_IOCTL.load(Ordering::Relaxed) {
        0 => -1,
        f => unsafe { std::mem::transmute::<usize, IoctlFn>(f)(fd, req, arg) },
    }
}

/// Called after SDL opened `path` as `fd`: start tracking it if it is the 8BitDo and Elite mode is on.
unsafe fn after_open(path: *const c_char, fd: c_int) {
    if fd < 0 || path.is_null() {
        return;
    }
    let p = unsafe { CStr::from_ptr(path) }.to_bytes();
    if !p.starts_with(b"/dev/input/event") {
        return;
    }
    let mut id = [0u16; 4];
    // EVIOCGID: _IOR('E', 0x02, struct input_id)
    let req = (IOC_READ << 30) | (8 << 16) | ((b'E' as c_ulong) << 8) | EVIOC_GID;
    if unsafe { real_ioctl(fd, req, id.as_mut_ptr() as *mut c_void) } < 0 || id[1] != VID || id[2] != PID {
        return;
    }
    // The pad's keyboard and mouse interfaces share its VID/PID; only xpad's gamepad node has BTN_SOUTH.
    let mut keys = [0u8; 96];
    let req = (IOC_READ << 30) | (96 << 16) | ((b'E' as c_ulong) << 8) | EVIOC_GBIT_KEY;
    let n = unsafe { real_ioctl(fd, req, keys.as_mut_ptr() as *mut c_void) };
    if n <= 0 || !is_gamepad(&keys[..n as usize]) {
        return;
    }
    let Some(s) = shared() else { return };
    if s.enabled.load(Ordering::Relaxed) == 0 {
        return;
    }
    let Some(i) = FDS.iter().position(|s| s.compare_exchange(-1, fd, Ordering::AcqRel, Ordering::Relaxed).is_ok()) else {
        log("no free slot");
        return;
    };
    SENT[i].store(0, Ordering::Relaxed);
    CLOCK[i].store(libc::CLOCK_REALTIME, Ordering::Relaxed);
    s.hook_pid.store(std::process::id(), Ordering::Relaxed);
    s.hook_generation.store(s.generation.load(Ordering::Relaxed), Ordering::Relaxed);
    log(&format!("took over {} as fd {fd}", String::from_utf8_lossy(p)));
}

// ---- hooked functions (called by libSDL3 in Steam) ---------------------------------------------

unsafe extern "C" fn hook_open(path: *const c_char, flags: c_int, mode: c_uint) -> c_int {
    let real: OpenFn = unsafe { std::mem::transmute(REAL_OPEN.load(Ordering::Relaxed)) };
    let fd = unsafe { real(path, flags, mode) };
    guard((), || unsafe { after_open(path, fd) });
    fd
}

unsafe extern "C" fn hook_open64(path: *const c_char, flags: c_int, mode: c_uint) -> c_int {
    let real: OpenFn = unsafe { std::mem::transmute(REAL_OPEN64.load(Ordering::Relaxed)) };
    let fd = unsafe { real(path, flags, mode) };
    guard((), || unsafe { after_open(path, fd) });
    fd
}

unsafe extern "C" fn hook_openat(dir: c_int, path: *const c_char, flags: c_int, mode: c_uint) -> c_int {
    let real: OpenatFn = unsafe { std::mem::transmute(REAL_OPENAT.load(Ordering::Relaxed)) };
    let fd = unsafe { real(dir, path, flags, mode) };
    guard((), || unsafe { after_open(path, fd) });
    fd
}

unsafe extern "C" fn hook_openat64(dir: c_int, path: *const c_char, flags: c_int, mode: c_uint) -> c_int {
    let real: OpenatFn = unsafe { std::mem::transmute(REAL_OPENAT64.load(Ordering::Relaxed)) };
    let fd = unsafe { real(dir, path, flags, mode) };
    guard((), || unsafe { after_open(path, fd) });
    fd
}

unsafe extern "C" fn hook_ioctl(fd: c_int, req: c_ulong, arg: *mut c_void) -> c_int {
    let r = unsafe { real_ioctl(fd, req, arg) };
    if r < 0 || arg.is_null() {
        return r;
    }
    guard(r, || {
        let Some(i) = slot(fd) else { return r };
        unsafe {
            match classify(req) {
                Patch::Id => {
                    patch_id(&mut *(arg as *mut [u16; 4]));
                    r
                }
                Patch::Name(len) => patch_name(std::slice::from_raw_parts_mut(arg as *mut u8, len)),
                Patch::KeyBits(len) => {
                    set_key_bits(std::slice::from_raw_parts_mut(arg as *mut u8, (r as usize).min(len)), 0x0f);
                    r
                }
                Patch::KeyState(len) => {
                    // a resync: report the held paddles, and remember SDL now knows them
                    let p = current_paddles();
                    set_key_bits(std::slice::from_raw_parts_mut(arg as *mut u8, (r as usize).min(len)), p);
                    SENT[i].store(p, Ordering::Relaxed);
                    r
                }
                Patch::ClockId => {
                    CLOCK[i].store(*(arg as *const c_int), Ordering::Relaxed);
                    r
                }
                Patch::None => r,
            }
        }
    })
}

unsafe extern "C" fn hook_read(fd: c_int, buf: *mut c_void, count: usize) -> isize {
    let synthetic = guard(None, || {
        let i = slot(fd)?;
        let (old, new) = (SENT[i].load(Ordering::Relaxed), current_paddles());
        if old == new || buf.is_null() {
            return None;
        }
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        unsafe { libc::clock_gettime(CLOCK[i].load(Ordering::Relaxed), &mut ts) };
        let (events, n) = paddle_events(old, new, ts.tv_sec as c_long, (ts.tv_nsec / 1000) as c_long);
        let bytes = n * size_of::<InputEvent>();
        if n == 0 || bytes > count {
            return None;
        }
        unsafe { std::ptr::copy_nonoverlapping(events.as_ptr() as *const u8, buf as *mut u8, bytes) };
        SENT[i].store(new, Ordering::Relaxed);
        if let Some(s) = shared() {
            s.hook_frames.fetch_add(1, Ordering::Relaxed);
        }
        Some(bytes as isize)
    });
    // SDL reads until EAGAIN, so the pad's real events follow on its next call.
    if let Some(n) = synthetic {
        return n;
    }
    let real: ReadFn = unsafe { std::mem::transmute(REAL_READ.load(Ordering::Relaxed)) };
    unsafe { real(fd, buf, count) }
}

unsafe extern "C" fn hook_close(fd: c_int) -> c_int {
    guard((), || {
        if let Some(i) = slot(fd) {
            FDS[i].store(-1, Ordering::Release);
        }
    });
    let real: CloseFn = unsafe { std::mem::transmute(REAL_CLOSE.load(Ordering::Relaxed)) };
    unsafe { real(fd) }
}

// ---- rtld-audit interface ------------------------------------------------------------------------

#[repr(C)]
pub struct LinkMap {
    l_addr: usize,
    l_name: *const c_char,
    l_ld: *const c_void,
    l_next: *const LinkMap,
    l_prev: *const LinkMap,
}

#[unsafe(no_mangle)]
pub extern "C" fn la_version(version: c_uint) -> c_uint {
    version.min(LAV_CURRENT)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn la_objopen(map: *mut LinkMap, lmid: c_long, cookie: *mut usize) -> c_uint {
    guard(0, || {
        if lmid != LM_ID_BASE || map.is_null() || !is_steam() {
            return 0;
        }
        let name = unsafe { (*map).l_name };
        if name.is_null() {
            return LA_FLG_BINDTO;
        }
        let name = unsafe { CStr::from_ptr(name) }.to_bytes();
        if name.ends_with(b"/ubuntu12_32/libSDL3.so.0") || name.ends_with(b"/ubuntu12_64/libSDL3.so.0") {
            SDL_COOKIE.store(unsafe { *cookie }, Ordering::Relaxed);
            log(&format!("bound to {}", String::from_utf8_lossy(name)));
            LA_FLG_BINDFROM | LA_FLG_BINDTO
        } else {
            LA_FLG_BINDTO
        }
    })
}

/// Swaps in a hook for one of SDL's libc imports. Returns the address SDL should call.
unsafe fn symbind(value: usize, refcook: *mut usize, flags: *mut c_uint, name: *const c_char) -> usize {
    guard(value, || {
        let sdl = SDL_COOKIE.load(Ordering::Relaxed);
        if sdl == 0 || refcook.is_null() || unsafe { *refcook } != sdl || name.is_null() {
            return value;
        }
        let (real, hook): (&AtomicUsize, usize) = match unsafe { CStr::from_ptr(name) }.to_bytes() {
            b"open" => (&REAL_OPEN, hook_open as OpenFn as usize),
            b"open64" => (&REAL_OPEN64, hook_open64 as OpenFn as usize),
            b"openat" => (&REAL_OPENAT, hook_openat as OpenatFn as usize),
            b"openat64" => (&REAL_OPENAT64, hook_openat64 as OpenatFn as usize),
            b"ioctl" => (&REAL_IOCTL, hook_ioctl as IoctlFn as usize),
            b"read" => (&REAL_READ, hook_read as ReadFn as usize),
            b"close" => (&REAL_CLOSE, hook_close as CloseFn as usize),
            _ => return value,
        };
        real.store(value, Ordering::Relaxed);
        if !flags.is_null() {
            unsafe { *flags |= LA_SYMB_NOPLTENTER | LA_SYMB_NOPLTEXIT };
        }
        log(&format!("hooked {}", unsafe { CStr::from_ptr(name) }.to_string_lossy()));
        hook
    })
}

#[cfg(target_pointer_width = "32")]
#[repr(C)]
pub struct Elf32Sym {
    st_name: u32,
    st_value: u32,
    st_size: u32,
    st_info: u8,
    st_other: u8,
    st_shndx: u16,
}

#[cfg(target_pointer_width = "64")]
#[repr(C)]
pub struct Elf64Sym {
    st_name: u32,
    st_info: u8,
    st_other: u8,
    st_shndx: u16,
    st_value: u64,
    st_size: u64,
}

#[cfg(target_pointer_width = "32")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn la_symbind32(
    sym: *mut Elf32Sym,
    _ndx: c_uint,
    refcook: *mut usize,
    _defcook: *mut usize,
    flags: *mut c_uint,
    name: *const c_char,
) -> usize {
    if sym.is_null() {
        return 0;
    }
    unsafe { symbind((*sym).st_value as usize, refcook, flags, name) }
}

#[cfg(target_pointer_width = "64")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn la_symbind64(
    sym: *mut Elf64Sym,
    _ndx: c_uint,
    refcook: *mut usize,
    _defcook: *mut usize,
    flags: *mut c_uint,
    name: *const c_char,
) -> usize {
    if sym.is_null() {
        return 0;
    }
    unsafe { symbind((*sym).st_value as usize, refcook, flags, name) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn ior(nr: c_ulong, size: c_ulong) -> c_ulong {
        (IOC_READ << 30) | (size << 16) | ((b'E' as c_ulong) << 8) | nr
    }

    #[test]
    fn ioctl_numbers() {
        // values from linux/input.h
        assert_eq!(ior(0x02, 8), 0x8008_4502); // EVIOCGID
        assert_eq!(classify(0x8008_4502), Patch::Id);
        assert_eq!(classify(ior(0x06, 128)), Patch::Name(128)); // EVIOCGNAME(128)
        assert_eq!(classify(ior(0x18, 96)), Patch::KeyState(96)); // EVIOCGKEY(96)
        assert_eq!(classify(ior(0x21, 96)), Patch::KeyBits(96)); // EVIOCGBIT(EV_KEY, 96)
        assert_eq!(classify(0x4004_45a0), Patch::ClockId); // EVIOCSCLOCKID
        assert_eq!(classify(ior(0x23, 8)), Patch::None); // EVIOCGBIT(EV_ABS)
        assert_eq!(classify(ior(0x40, 44)), Patch::None); // EVIOCGABS(0)
        assert_eq!(classify(0x4030_4580), Patch::None); // EVIOCSFF
        assert_eq!(classify(0x8004_6a13), Patch::None); // JSIOCGNAME-like, other type
    }

    #[test]
    fn id_becomes_elite() {
        let mut id = [0x0003, 0x2DC8, 0x310B, 0x0114];
        patch_id(&mut id);
        assert_eq!(id, [0x0003, 0x045E, 0x0B00, 0x0114]);
    }

    #[test]
    fn name_is_terminated_and_truncated() {
        let mut b = [0xffu8; 128];
        assert_eq!(patch_name(&mut b) as usize, ELITE_NAME.len() + 1);
        assert_eq!(&b[..ELITE_NAME.len()], ELITE_NAME);
        assert_eq!(b[ELITE_NAME.len()], 0);
        let mut small = [0xffu8; 5];
        assert_eq!(patch_name(&mut small), 5);
        assert_eq!(&small, b"Xbox\0");
        assert_eq!(patch_name(&mut []), 0);
    }

    #[test]
    fn key_bits_match_the_kernel_layout() {
        let mut bits = [0u8; 96];
        set_key_bits(&mut bits, 0x0f);
        // 0x224..0x227 are byte 68, bits 4..7
        assert_eq!(bits[68], 0xf0);
        assert_eq!(bits.iter().filter(|&&b| b != 0).count(), 1);

        let mut bits = [0u8; 96];
        set_key_bits(&mut bits, shm::P1); // upper right = BTN_GRIPR 0x225
        assert_eq!(bits[68], 1 << 5);
        let mut bits = [0u8; 96];
        set_key_bits(&mut bits, shm::P4); // lower left = BTN_GRIPL2 0x226
        assert_eq!(bits[68], 1 << 6);

        let mut short = [0u8; 10];
        set_key_bits(&mut short, 0x0f); // too short to hold the paddles: untouched
        assert_eq!(short, [0u8; 10]);
    }

    #[test]
    fn only_the_gamepad_node() {
        let mut bits = [0u8; 96];
        assert!(!is_gamepad(&bits), "keyboard or mouse node");
        bits[0x130 / 8] |= 1 << (0x130 % 8);
        assert!(is_gamepad(&bits));
        assert!(!is_gamepad(&bits[..10]));
    }

    #[test]
    fn events_for_changes_only() {
        let (_, n) = paddle_events(0, 0, 1, 2);
        assert_eq!(n, 0);

        let (ev, n) = paddle_events(0, shm::P1 | shm::P3, 7, 9);
        assert_eq!(n, 3);
        assert_eq!(ev[0], InputEvent { sec: 7, usec: 9, kind: EV_KEY, code: 0x225, value: 1 });
        assert_eq!(ev[1], InputEvent { sec: 7, usec: 9, kind: EV_KEY, code: 0x224, value: 1 });
        assert_eq!(ev[2], InputEvent { sec: 7, usec: 9, kind: EV_SYN, code: SYN_REPORT, value: 0 });

        let (ev, n) = paddle_events(shm::P1 | shm::P3, shm::P3 | shm::P2, 0, 0);
        assert_eq!(n, 3);
        assert_eq!((ev[0].code, ev[0].value), (0x225, 0));
        assert_eq!((ev[1].code, ev[1].value), (0x227, 1));

        let (_, n) = paddle_events(0, 0x0f, 0, 0);
        assert_eq!(n, 5);
    }

    #[test]
    fn input_event_is_the_kernel_size() {
        assert_eq!(size_of::<InputEvent>(), if cfg!(target_pointer_width = "32") { 16 } else { 24 });
    }
}
