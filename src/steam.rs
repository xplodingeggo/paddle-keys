//! Steam Elite mode: gets the hook into Steam and shares the paddle state with it.
//! Windows injects a DLL into steam.exe; Linux has Steam load an LD_AUDIT module at launch.

#[allow(dead_code)] // the hook's half of the layout
#[path = "../steam-hook/src/shm.rs"]
mod shm;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(windows)]
use windows as platform;

#[cfg(target_os = "linux")]
pub use linux::{install_hook, remove_hook};
pub use platform::run;
use platform::Link;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use crate::decode::Paddles;

/// Where paddle presses go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Steam while it has the controller as an Elite 2, keys otherwise.
    Auto = 0,
    Keys = 1,
    Steam = 2,
    Both = 3,
}

impl Mode {
    pub fn from_u8(v: u8) -> Mode {
        match v {
            1 => Mode::Keys,
            2 => Mode::Steam,
            3 => Mode::Both,
            _ => Mode::Auto,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Off = 0,
    NeedDriver = 1,
    NeedSetting = 2,
    NoSteam = 3,
    InjectFailed = 4,
    Reconnect = 5,
    Active = 6,
    /// Linux: the hook is not in steam.sh's LD_AUDIT.
    NeedHook = 7,
    /// Linux: Steam was started before the hook was added.
    NeedRestart = 8,
}

impl Status {
    pub fn text(self) -> &'static str {
        match self {
            Status::Off => "Status: off",
            Status::NeedDriver => "Setup: Steam > Settings > Controller > install Xbox Extended Feature Support, then reboot",
            Status::NeedSetting => "Setup: Steam > Settings > Controller > enable Steam Input for Xbox controllers",
            Status::NoSteam => "Status: waiting for Steam",
            Status::InjectFailed => "Status: could not load into Steam",
            Status::Reconnect => "Status: turn the controller off and on to activate",
            Status::Active => "Status: active, Steam sees an Elite 2",
            Status::NeedHook => "Setup: click Install Steam hook, then restart Steam",
            Status::NeedRestart => "Status: restart Steam to load the hook",
        }
    }

    fn from_u8(v: u8) -> Status {
        [
            Status::Off,
            Status::NeedDriver,
            Status::NeedSetting,
            Status::NoSteam,
            Status::InjectFailed,
            Status::Reconnect,
            Status::Active,
            Status::NeedHook,
            Status::NeedRestart,
        ]
            .get(v as usize)
            .copied()
            .unwrap_or(Status::Off)
    }
}

/// Settings and live state shared by the tray, the poller and the Steam thread.
pub struct State {
    pub enabled: AtomicBool,
    pub mode: AtomicU8,
    live: AtomicBool,
    status: AtomicU8,
    link: Option<Link>,
}

impl State {
    pub fn load() -> Arc<State> {
        let (enabled, mode) = platform::load_settings();
        Arc::new(State {
            enabled: AtomicBool::new(enabled),
            mode: AtomicU8::new(mode),
            live: AtomicBool::new(false),
            status: AtomicU8::new(Status::Off as u8),
            link: Link::open(),
        })
    }

    pub fn save(&self) {
        platform::save_settings(self.enabled.load(Ordering::Relaxed), self.mode.load(Ordering::Relaxed));
    }

    pub fn status(&self) -> Status {
        Status::from_u8(self.status.load(Ordering::Relaxed))
    }

    /// Splits one poll's paddles between the keyboard and Steam, and hands Steam its share.
    pub fn route(&self, p: Paddles) -> Paddles {
        let mode = Mode::from_u8(self.mode.load(Ordering::Relaxed));
        let (keys, steam) = route(self.enabled.load(Ordering::Relaxed), mode, self.live.load(Ordering::Relaxed));
        if let Some(l) = &self.link {
            l.publish(if steam { elite_bits(p) } else { 0 });
        }
        if keys { p } else { Paddles::default() }
    }

    /// paddle-keys is quitting: the hook stops taking over newly connected controllers.
    pub fn shutdown(&self) {
        if let Some(l) = &self.link {
            l.s.enabled.store(0, Ordering::Relaxed);
            l.publish(0);
        }
    }

    /// The poller lost the controller: Steam's takeover of that connection is over.
    pub fn lost(&self) {
        if let Some(l) = &self.link {
            l.s.generation.fetch_add(1, Ordering::Relaxed);
            l.publish(0);
        }
    }
}

/// (send keys, send to Steam)
pub fn route(enabled: bool, mode: Mode, live: bool) -> (bool, bool) {
    if !enabled {
        return (true, false);
    }
    match mode {
        Mode::Auto => (!live, live),
        Mode::Keys => (true, false),
        Mode::Steam => (false, true),
        Mode::Both => (true, true),
    }
}

/// 8BitDo paddles to Elite paddles: R4 and PR are the right pair, L4 and PL the left pair.
pub fn elite_bits(p: Paddles) -> u32 {
    [(p.r4, shm::P1), (p.pr, shm::P2), (p.l4, shm::P3), (p.pl, shm::P4)]
        .into_iter()
        .filter(|&(down, _)| down)
        .fold(0, |m, (_, bit)| m | bit)
}

fn setting_on(vdf: &str) -> bool {
    vdf.lines().any(|l| {
        let mut parts = l.split('"').filter(|s| !s.trim().is_empty());
        parts.next() == Some("SteamController_XBoxSupport") && parts.next() == Some("1")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(l4: bool, r4: bool, pl: bool, pr: bool) -> Paddles {
        Paddles { l4, r4, pl, pr }
    }

    #[test]
    fn routing() {
        assert_eq!(route(false, Mode::Steam, true), (true, false), "off means keys only");
        assert_eq!(route(true, Mode::Auto, true), (false, true));
        assert_eq!(route(true, Mode::Auto, false), (true, false));
        assert_eq!(route(true, Mode::Keys, true), (true, false));
        assert_eq!(route(true, Mode::Steam, false), (false, true));
        assert_eq!(route(true, Mode::Both, false), (true, true));
    }

    #[test]
    fn elite_paddle_order() {
        assert_eq!(elite_bits(p(false, true, false, false)), shm::P1);
        assert_eq!(elite_bits(p(false, false, false, true)), shm::P2);
        assert_eq!(elite_bits(p(true, false, false, false)), shm::P3);
        assert_eq!(elite_bits(p(false, false, true, false)), shm::P4);
        assert_eq!(elite_bits(p(true, true, true, true)), 0x0f);
    }

    #[test]
    fn modes_round_trip() {
        for m in [Mode::Auto, Mode::Keys, Mode::Steam, Mode::Both] {
            assert_eq!(Mode::from_u8(m as u8), m);
        }
        assert_eq!(Mode::from_u8(99), Mode::Auto);
    }

    #[test]
    fn status_round_trip() {
        for v in 0..=8u8 {
            assert_eq!(Status::from_u8(v) as u8, v);
        }
        assert_eq!(Status::from_u8(200), Status::Off);
    }

    #[test]
    fn steam_setting_parse() {
        assert!(setting_on("\t\t\"SteamController_XBoxSupport\"\t\t\"1\"\n"));
        assert!(!setting_on("\t\t\"SteamController_XBoxSupport\"\t\t\"0\"\n"));
        assert!(!setting_on("\"Other\" \"1\""));
    }
}
