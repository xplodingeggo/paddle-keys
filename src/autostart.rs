use std::io;

use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

pub fn is_enabled_named(name: &str) -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN, KEY_READ)
        .and_then(|k| k.get_value::<String, _>(name))
        .is_ok()
}

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
