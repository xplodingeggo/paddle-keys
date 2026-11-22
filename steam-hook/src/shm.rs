//! Shared-memory layout between paddle-keys (creates it, owns the paddle state) and the hook
//! inside Steam. Included by both crates so the layout has a single source.

use std::sync::atomic::{AtomicU32, AtomicU64};

#[cfg(windows)]
pub const NAME: &str = r"Local\paddle-keys-steam";

/// A per-user file in /dev/shm, mapped by both sides. Same layout on 32- and 64-bit.
#[cfg(target_os = "linux")]
pub fn path() -> String {
    format!("/dev/shm/paddle-keys-steam-{}", unsafe { libc::getuid() })
}

/// Milliseconds on CLOCK_MONOTONIC, the clock both sides stamp `heartbeat` with on Linux.
#[cfg(target_os = "linux")]
pub fn now_ms() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1000 + ts.tv_nsec as u64 / 1_000_000
}
pub const MAGIC: u32 = 0x5044_4b53; // "PDKS"
pub const VERSION: u32 = 1;
/// Paddle state older than this counts as released, so a dead paddle-keys never leaves one held.
pub const HEARTBEAT_MS: u64 = 250;

/// Elite paddle bits as SDL reads them: P1 upper right, P2 lower right, P3 upper left, P4 lower left.
pub const P1: u32 = 0x01;
pub const P2: u32 = 0x02;
pub const P3: u32 = 0x04;
pub const P4: u32 = 0x08;

#[repr(C)]
pub struct Shared {
    // written by paddle-keys
    pub magic: AtomicU32,
    pub version: AtomicU32,
    /// 1 = take over newly connected pads. Pads already taken over keep being translated.
    pub enabled: AtomicU32,
    pub paddles: AtomicU32,
    /// GetTickCount64 (Windows) or `now_ms` (Linux) of the last paddle write.
    pub heartbeat: AtomicU64,
    /// Bumped when paddle-keys loses the controller, so a stale takeover is never reported live.
    pub generation: AtomicU32,
    // written by the hook
    pub hook_pid: AtomicU32,
    /// `generation` at the time the hook last took over a pad.
    pub hook_generation: AtomicU32,
    pub hook_frames: AtomicU64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_the_same_on_32_and_64_bit() {
        assert_eq!(std::mem::offset_of!(Shared, heartbeat), 16);
        assert_eq!(std::mem::offset_of!(Shared, generation), 24);
        assert_eq!(std::mem::offset_of!(Shared, hook_frames), 40);
        assert_eq!(std::mem::size_of::<Shared>(), 48);
    }
}
