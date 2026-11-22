//! The hook paddle-keys gets into the Steam client so Steam Input sees the 8BitDo Ultimate 2 as an
//! Xbox Elite Series 2 with real paddles.
//!  - Windows (`windows.rs`): a DLL injected into steam.exe that rewrites Valve's steamxbox sideband.
//!  - Linux (`linux.rs`): an rtld-audit module (LD_AUDIT) that morphs the pad's evdev node for
//!    Steam's bundled SDL3.

pub mod gip;
pub mod shm;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;
