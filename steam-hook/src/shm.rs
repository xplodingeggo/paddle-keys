//! Shared-memory layout between paddle-keys (creates it, owns the paddle state) and the hook DLL
//! inside steam.exe. Included by both crates so the layout has a single source.

use std::sync::atomic::{AtomicU32, AtomicU64};

pub const NAME: &str = r"Local\paddle-keys-steam";
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
    /// GetTickCount64 of the last paddle write.
    pub heartbeat: AtomicU64,
    /// Bumped when paddle-keys loses the controller, so a stale takeover is never reported live.
    pub generation: AtomicU32,
    // written by the hook
    pub hook_pid: AtomicU32,
    /// `generation` at the time the hook last took over a pad.
    pub hook_generation: AtomicU32,
    pub hook_frames: AtomicU64,
}
