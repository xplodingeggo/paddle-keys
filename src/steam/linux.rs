//! Linux: Steam loads the hook as an rtld-audit module (LD_AUDIT), the way SLSsteam is loaded.
//! paddle-keys adds the hook to steam.sh once; Steam picks it up on its next start, before it
//! enumerates controllers, so no power-cycle is needed.

use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::{State, Status, setting_on, shm};

/// Marks every line of steam.sh that paddle-keys wrote or changed.
const MARKER: &str = "# paddle-keys";
/// Part of every staged hook path, so removal finds exactly ours.
const HOOK_NAME: &str = "libpaddle_keys_steam";
/// (staged name, built names to look for in each search dir; the second is the cargo target layout)
const HOOKS: [(&str, &[&str]); 2] = [
    ("libpaddle_keys_steam32.so", &["libpaddle_keys_steam32.so", "../i686-unknown-linux-gnu/release/libpaddle_keys_steam.so"]),
    ("libpaddle_keys_steam64.so", &["libpaddle_keys_steam64.so", "libpaddle_keys_steam.so"]),
];

#[derive(Serialize, Deserialize, Default)]
struct Settings {
    #[serde(default)]
    steam_elite: bool,
    #[serde(default)]
    paddle_mode: u8,
}

fn settings_path() -> Option<PathBuf> {
    crate::config_dir().map(|d| d.join("state.toml"))
}

/// (Steam Elite mode on, paddle mode)
pub fn load_settings() -> (bool, u8) {
    let s: Settings = settings_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default();
    (s.steam_elite, s.paddle_mode)
}

pub fn save_settings(enabled: bool, mode: u8) {
    let Some(path) = settings_path() else { return };
    if let (Some(dir), Ok(text)) = (path.parent(), toml::to_string(&Settings { steam_elite: enabled, paddle_mode: mode })) {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(path, text);
    }
}

pub struct Link {
    pub s: &'static shm::Shared,
}

impl Link {
    pub fn open() -> Option<Link> {
        let f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode_0600()
            .open(shm::path())
            .ok()?;
        f.set_len(4096).ok()?;
        unsafe {
            let p = libc::mmap(std::ptr::null_mut(), 4096, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, f.as_raw_fd(), 0);
            if p == libc::MAP_FAILED {
                return None;
            }
            // The mapping lives for the whole process.
            let s = (p as *const shm::Shared).as_ref()?;
            // A hook in a running Steam may still hold the file; keep its counters.
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
        self.s.heartbeat.store(shm::now_ms(), Ordering::Relaxed);
    }

    /// The hook took over the controller's current connection, in a Steam that is still running.
    pub fn live(&self) -> bool {
        let g = self.s.generation.load(Ordering::Relaxed);
        g != 0 && self.s.hook_generation.load(Ordering::Relaxed) == g && alive(self.s.hook_pid.load(Ordering::Relaxed))
    }
}

trait Mode0600 {
    fn mode_0600(&mut self) -> &mut Self;
}

impl Mode0600 for std::fs::OpenOptions {
    fn mode_0600(&mut self) -> &mut Self {
        use std::os::unix::fs::OpenOptionsExt;
        self.mode(0o600)
    }
}

fn alive(pid: u32) -> bool {
    pid != 0 && Path::new(&format!("/proc/{pid}")).exists()
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Steam's install folder: ~/.steam/steam is the launcher's symlink to it.
fn steam_dir() -> Option<PathBuf> {
    let h = home()?;
    [h.join(".steam/steam"), h.join(".local/share/Steam")]
        .into_iter()
        .find(|d| d.join("steam.sh").is_file())
        .map(|d| d.canonicalize().unwrap_or(d))
}

fn hook_dir() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".local/share")))?;
    Some(data.join("paddle-keys"))
}

/// Copies the built hooks to fixed paths. Each copy goes through a temp file and a rename, so a
/// running Steam keeps the file it loaded. Returns the staged paths that exist afterwards.
fn stage() -> Vec<PathBuf> {
    let Some(dir) = hook_dir() else { return vec![] };
    // next to the binary (tarball, cargo target dir), then where distro packages put them
    let search: Vec<PathBuf> = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
        .into_iter()
        .chain(["/usr/lib/paddle-keys", "/usr/local/lib/paddle-keys"].map(PathBuf::from))
        .collect();
    let _ = std::fs::create_dir_all(&dir);
    let mut out = Vec::new();
    for (staged, built) in HOOKS {
        let dst = dir.join(staged);
        let src = search.iter().flat_map(|d| built.iter().map(move |b| d.join(b))).find(|p| p.is_file());
        if let Some(src) = src
            && let Ok(bytes) = std::fs::read(&src)
            && std::fs::read(&dst).ok().as_deref() != Some(&bytes[..])
        {
            let tmp = dir.join(format!(".{staged}.tmp"));
            if std::fs::write(&tmp, &bytes).is_ok() {
                let _ = std::fs::rename(&tmp, &dst);
            }
        }
        if dst.is_file() {
            out.push(dst);
        }
    }
    out
}

/// `/home/me/x` becomes `$HOME/x`, the way headcrab writes its paths.
fn shell_path(p: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| p.strip_prefix(h).ok()) {
        Some(rel) => format!("$HOME/{}", rel.display()),
        None => p.display().to_string(),
    }
}

/// Adds `paths` to steam.sh's LD_AUDIT: into an existing `LD_AUDIT=` assignment (headcrab's
/// INJECT_SLS line), or as an export after the shebang. None if it is already there.
pub fn patch(text: &str, paths: &[String]) -> Option<String> {
    if text.lines().any(|l| l.contains(MARKER)) {
        return None;
    }
    let joined = paths.join(":");
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let target = lines.iter().position(|l| {
        let code = l.split('#').next().unwrap_or("");
        code.contains("LD_AUDIT=") && !code.contains("LD_AUDIT=\"${LD_AUDIT")
    });
    match target {
        Some(i) => {
            let l = &lines[i];
            let start = l.find("LD_AUDIT=").expect("found above") + "LD_AUDIT=".len();
            let rest = &l[start..];
            // `X="LD_AUDIT="`: inside a string already, so the quote after = closes it
            let inside = l[..start].matches(['"', '\'']).count() % 2 == 1;
            let quoted = !inside && (rest.starts_with('"') || rest.starts_with('\''));
            let body = if quoted { start + 1 } else { start };
            let end = l[body..]
                .find(|c: char| c == '"' || c == '\'' || c.is_whitespace())
                .map_or(l.len(), |e| body + e);
            let sep = if end > body { ":" } else { "" };
            lines[i] = format!("{}{sep}{joined}{} {MARKER}", &l[..end], &l[end..]);
        }
        None => {
            let at = usize::from(lines.first().is_some_and(|l| l.starts_with("#!")));
            lines.insert(at, format!("export LD_AUDIT=\"${{LD_AUDIT:+$LD_AUDIT:}}{joined}\" {MARKER}"));
        }
    }
    let mut out = lines.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    Some(out)
}

/// Undoes `patch`. None if steam.sh has nothing of ours.
pub fn unpatch(text: &str) -> Option<String> {
    if !text.lines().any(|l| l.contains(MARKER)) {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    for l in text.lines() {
        let Some(code) = l.strip_suffix(&format!(" {MARKER}")) else {
            out.push(l.to_string());
            continue;
        };
        if code.trim_start().starts_with("export LD_AUDIT=\"${LD_AUDIT:+$LD_AUDIT:}") {
            continue; // a line we inserted
        }
        let mut line = code.to_string();
        // remove ":<our path>" (or "<our path>:" when it came first) inside the assignment
        while let Some(i) = line.find(HOOK_NAME) {
            let begin = line[..i].rfind([':', '=', '"', '\'']).map_or(0, |b| b + 1);
            let end = line[i..].find([':', '"', '\'', ' ']).map_or(line.len(), |e| i + e);
            let (begin, end) = if line[..begin].ends_with(':') {
                (begin - 1, end)
            } else if line[end..].starts_with(':') {
                (begin, end + 1)
            } else {
                (begin, end)
            };
            line.replace_range(begin..end, "");
        }
        out.push(line);
    }
    let mut s = out.join("\n");
    if text.ends_with('\n') {
        s.push('\n');
    }
    Some(s)
}

fn steam_sh() -> Result<PathBuf, String> {
    steam_dir().map(|d| d.join("steam.sh")).ok_or_else(|| "Steam not found (no ~/.steam/steam/steam.sh)".to_string())
}

fn hook_installed() -> bool {
    steam_sh().ok().and_then(|p| std::fs::read_to_string(p).ok()).is_some_and(|t| t.contains(MARKER))
}

/// Writes steam.sh in place (keeping its mode), after a one-time backup.
fn rewrite(sh: &Path, text: &str) -> Result<(), String> {
    let bak = sh.with_file_name("steam.sh.paddle-keys.bak");
    if !bak.exists() {
        std::fs::copy(sh, &bak).map_err(|e| format!("backup {}: {e}", bak.display()))?;
    }
    // headcrab makes steam.sh read-only so Steam's updater can't replace it; keep it that way
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(sh).map_err(|e| format!("stat {}: {e}", sh.display()))?.permissions().mode();
    let set = |m: u32| std::fs::set_permissions(sh, std::fs::Permissions::from_mode(m));
    if mode & 0o200 == 0 {
        set(mode | 0o200).map_err(|e| format!("chmod {}: {e}", sh.display()))?;
    }
    let r = std::fs::write(sh, text).map_err(|e| format!("write {}: {e}", sh.display()));
    let _ = set(mode);
    r
}

pub fn install_hook() -> Result<String, String> {
    let hooks = stage();
    if !hooks.iter().any(|p| p.ends_with(HOOKS[0].0)) {
        return Err("32-bit hook not found: build it with `cargo build -p paddle-keys-steam --release --target i686-unknown-linux-gnu` or keep libpaddle_keys_steam32.so next to paddle-keys".into());
    }
    let sh = steam_sh()?;
    let text = std::fs::read_to_string(&sh).map_err(|e| format!("read {}: {e}", sh.display()))?;
    let h = home();
    let paths: Vec<String> = hooks.iter().map(|p| shell_path(p, h.as_deref())).collect();
    match patch(&text, &paths) {
        None => Ok(format!("The Steam hook is already in {}. Restart Steam if it is running.", sh.display())),
        Some(new) => {
            rewrite(&sh, &new)?;
            Ok(format!("Added the Steam hook to {}. Restart Steam to load it.", sh.display()))
        }
    }
}

pub fn remove_hook() -> Result<String, String> {
    let sh = steam_sh()?;
    let text = std::fs::read_to_string(&sh).map_err(|e| format!("read {}: {e}", sh.display()))?;
    match unpatch(&text) {
        None => Ok(format!("The Steam hook is not in {}.", sh.display())),
        Some(new) => {
            rewrite(&sh, &new)?;
            Ok(format!("Removed the Steam hook from {}. It stays loaded until Steam restarts.", sh.display()))
        }
    }
}

/// PIDs of the Steam client (ubuntu12_32/steam, or ubuntu12_64/steam on a 64-bit client).
fn steam_pids() -> Vec<u32> {
    let Ok(dir) = std::fs::read_dir("/proc") else { return vec![] };
    dir.flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            std::fs::read_link(format!("/proc/{pid}/exe")).is_ok_and(|exe| {
                exe.file_name().is_some_and(|n| n == "steam")
                    && exe.parent().and_then(Path::file_name).is_some_and(|d| d == "ubuntu12_32" || d == "ubuntu12_64")
            })
        })
        .collect()
}

fn has_hook(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/maps")).is_ok_and(|m| m.contains(HOOK_NAME))
}

fn xbox_setting_on() -> bool {
    let Some(dir) = steam_dir() else { return false };
    let users = std::fs::read_dir(dir.join("userdata"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path().join("config").join("localconfig.vdf"));
    std::iter::once(dir.join("config").join("config.vdf"))
        .chain(users)
        .any(|p| std::fs::read_to_string(p).is_ok_and(|t| setting_on(&t)))
}

/// Keeps the hook staged and the status current. Loading the hook is Steam's job (steam.sh).
pub fn run(state: Arc<State>, stop: Arc<AtomicBool>) {
    stage();
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
                setup = (hook_installed(), xbox_setting_on());
                checked = Instant::now();
            }
            let pids = steam_pids();
            let loaded = pids.iter().any(|&p| has_hook(p));
            // The setup checks only explain why it is not live; they never block a live takeover.
            if state.link.as_ref().is_some_and(Link::live) {
                Status::Active
            } else if !setup.0 && !loaded {
                Status::NeedHook
            } else if !setup.1 {
                Status::NeedSetting
            } else if pids.is_empty() {
                Status::NoSteam
            } else if !loaded {
                Status::NeedRestart
            } else {
                Status::Reconnect
            }
        };
        state.live.store(status == Status::Active, Ordering::Relaxed);
        state.status.store(status as u8, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADCRAB: &str = "#!/usr/bin/env bash\n\n\n    STEAM_CLIENT=\"$HOME/.steam/steam/client.sh\"\n    INJECT_SLS=\"LD_AUDIT=$HOME/.local/share/SLSsteam/library-inject.so:$HOME/.local/share/SLSsteam/SLSsteam.so\"\n    NOTIF=\"x\"\n        echo \"Loaded SLSsteam\" & export $INJECT_SLS &> /dev/null\n";
    const STOCK: &str = "#!/usr/bin/env bash\n\n# verbose\n#set -x\nexport TEXTDOMAIN=steam\n";

    fn ours() -> Vec<String> {
        vec![
            "$HOME/.local/share/paddle-keys/libpaddle_keys_steam32.so".into(),
            "$HOME/.local/share/paddle-keys/libpaddle_keys_steam64.so".into(),
        ]
    }

    #[test]
    fn headcrab_line_gets_our_paths() {
        let out = patch(HEADCRAB, &ours()).unwrap();
        let line = out.lines().find(|l| l.contains("INJECT_SLS=")).unwrap();
        assert_eq!(
            line,
            "    INJECT_SLS=\"LD_AUDIT=$HOME/.local/share/SLSsteam/library-inject.so:$HOME/.local/share/SLSsteam/SLSsteam.so:$HOME/.local/share/paddle-keys/libpaddle_keys_steam32.so:$HOME/.local/share/paddle-keys/libpaddle_keys_steam64.so\" # paddle-keys"
        );
        // the `export $INJECT_SLS` line has no LD_AUDIT= and is left alone
        assert_eq!(out.lines().count(), HEADCRAB.lines().count());
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn stock_script_gets_an_export_after_the_shebang() {
        let out = patch(STOCK, &ours()).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "#!/usr/bin/env bash");
        assert_eq!(
            lines[1],
            "export LD_AUDIT=\"${LD_AUDIT:+$LD_AUDIT:}$HOME/.local/share/paddle-keys/libpaddle_keys_steam32.so:$HOME/.local/share/paddle-keys/libpaddle_keys_steam64.so\" # paddle-keys"
        );
        assert_eq!(lines.len(), STOCK.lines().count() + 1);
    }

    #[test]
    fn patch_is_idempotent_and_unpatch_restores() {
        for text in [HEADCRAB, STOCK] {
            let once = patch(text, &ours()).unwrap();
            assert_eq!(patch(&once, &ours()), None);
            assert_eq!(unpatch(&once).as_deref(), Some(text));
            assert_eq!(unpatch(text), None);
        }
    }

    #[test]
    fn unquoted_and_empty_assignments() {
        let t = "export LD_AUDIT=/a.so\n";
        let out = patch(t, &ours()[..1]).unwrap();
        assert_eq!(out, "export LD_AUDIT=/a.so:$HOME/.local/share/paddle-keys/libpaddle_keys_steam32.so # paddle-keys\n");
        assert_eq!(unpatch(&out).as_deref(), Some(t));

        let t = "X=\"LD_AUDIT=\"\n";
        let out = patch(t, &ours()[..1]).unwrap();
        assert_eq!(out, "X=\"LD_AUDIT=$HOME/.local/share/paddle-keys/libpaddle_keys_steam32.so\" # paddle-keys\n");
        assert_eq!(unpatch(&out).as_deref(), Some(t));
    }

    #[test]
    fn commented_ld_audit_is_ignored() {
        let t = "#!/bin/sh\n# LD_AUDIT=/old.so\n";
        let out = patch(t, &ours()[..1]).unwrap();
        assert!(out.lines().nth(1).unwrap().starts_with("export LD_AUDIT="));
        assert_eq!(unpatch(&out).as_deref(), Some(t));
    }

    #[test]
    fn home_paths_become_dollar_home() {
        let h = Path::new("/home/me");
        assert_eq!(shell_path(Path::new("/home/me/.local/share/x.so"), Some(h)), "$HOME/.local/share/x.so");
        assert_eq!(shell_path(Path::new("/opt/x.so"), Some(h)), "/opt/x.so");
    }
}
