use std::io;

#[cfg(windows)]
use winreg::RegKey;
#[cfg(windows)]
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

#[cfg(windows)]
const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

#[cfg(windows)]
pub fn is_enabled_named(name: &str) -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN, KEY_READ)
        .and_then(|k| k.get_value::<String, _>(name))
        .is_ok()
}

#[cfg(windows)]
pub fn set_named(name: &str, enabled: bool) -> io::Result<()> {
    let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN, KEY_WRITE)?;
    if enabled {
        let exe = std::env::current_exe()?;
        key.set_value(name, &format!("\"{}\"", exe.display()))
    } else {
        match key.delete_value(name) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            r => r,
        }
    }
}

/// XDG autostart entry, read by most desktops; Hyprland needs `exec-once = paddle-keys` instead.
#[cfg(target_os = "linux")]
fn entry(name: &str) -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    Some(base.join("autostart").join(format!("{name}.desktop")))
}

#[cfg(target_os = "linux")]
pub fn is_enabled_named(name: &str) -> bool {
    entry(name).is_some_and(|p| p.exists())
}

#[cfg(target_os = "linux")]
pub fn set_named(name: &str, enabled: bool) -> io::Result<()> {
    let path = entry(name).ok_or_else(|| io::Error::other("no HOME"))?;
    if enabled {
        let exe = std::env::current_exe()?;
        std::fs::create_dir_all(path.parent().expect("has parent"))?;
        std::fs::write(
            &path,
            format!("[Desktop Entry]\nType=Application\nName=paddle-keys\nExec=\"{}\"\nX-GNOME-Autostart-enabled=true\n", exe.display()),
        )
    } else {
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            r => r,
        }
    }
}

pub fn is_enabled() -> bool {
    is_enabled_named("paddle-keys")
}

pub fn set(enabled: bool) -> io::Result<()> {
    set_named("paddle-keys", enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_named() {
        let n = "paddle-keys-test";
        let _ = set_named(n, false);
        assert!(!is_enabled_named(n));
        set_named(n, true).unwrap();
        assert!(is_enabled_named(n));
        set_named(n, false).unwrap();
        assert!(!is_enabled_named(n));
    }
}
