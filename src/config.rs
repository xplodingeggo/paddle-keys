use std::path::Path;

use serde::Deserialize;

use crate::decode::Paddle;

pub const DEFAULT_TOML: &str = r#"# Key each paddle presses. Modifiers are optional, e.g. "Ctrl+Shift+F13".
# Keys: F1-F24, A-Z, 0-9, Space, Enter, Esc, Tab.
[keys]
L4 = "F13"
R4 = "F14"
PL = "F15"
PR = "F16"
"#;

#[derive(Deserialize, Default)]
struct File {
    #[serde(default)]
    keys: Keys,
}

#[derive(Deserialize, Default)]
struct Keys {
    #[serde(rename = "L4")]
    l4: Option<String>,
    #[serde(rename = "R4")]
    r4: Option<String>,
    #[serde(rename = "PL")]
    pl: Option<String>,
    #[serde(rename = "PR")]
    pr: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeySpec {
    pub mods: Vec<u16>,
    pub key: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bindings {
    l4: KeySpec,
    r4: KeySpec,
    pl: KeySpec,
    pr: KeySpec,
}

impl Default for Bindings {
    fn default() -> Self {
        Self {
            l4: spec(&[], codes::fkey(13)),
            r4: spec(&[], codes::fkey(14)),
            pl: spec(&[], codes::fkey(15)),
            pr: spec(&[], codes::fkey(16)),
        }
    }
}

impl Bindings {
    pub fn get(&self, p: Paddle) -> &KeySpec {
        match p {
            Paddle::L4 => &self.l4,
            Paddle::R4 => &self.r4,
            Paddle::PL => &self.pl,
            Paddle::PR => &self.pr,
        }
    }
}

pub fn parse_keyspec(s: &str) -> Result<KeySpec, String> {
    let tokens: Vec<&str> = s.split('+').map(str::trim).collect();
    if tokens.iter().any(|t| t.is_empty()) {
        return Err(format!("empty token in {s:?}"));
    }
    let (key, mods) = tokens.split_last().ok_or("empty key spec")?;
    let mods = mods.iter().map(|m| modifier(m)).collect::<Result<Vec<_>, _>>()?;
    Ok(KeySpec {
        mods,
        key: key_code(key)?,
    })
}

fn modifier(t: &str) -> Result<u16, String> {
    match t.to_ascii_lowercase().as_str() {
        "ctrl" => Ok(codes::CTRL),
        "shift" => Ok(codes::SHIFT),
        "alt" => Ok(codes::ALT),
        "win" => Ok(codes::WIN),
        _ => Err(format!("unknown modifier {t:?}")),
    }
}

fn key_code(t: &str) -> Result<u16, String> {
    let lower = t.to_ascii_lowercase();
    let named = match lower.as_str() {
        "space" => Some(codes::SPACE),
        "enter" => Some(codes::ENTER),
        "esc" => Some(codes::ESC),
        "tab" => Some(codes::TAB),
        _ => None,
    };
    if let Some(k) = named {
        return Ok(k);
    }
    // Single char first, so "F" is the letter F and not a malformed F-key.
    if t.len() == 1 && t.as_bytes()[0].is_ascii_alphanumeric() {
        return Ok(codes::char_key(t.as_bytes()[0].to_ascii_uppercase()));
    }
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u16>().ok())
        && (1..=24).contains(&n)
    {
        return Ok(codes::fkey(n));
    }
    Err(format!("unknown key {t:?}"))
}

/// Windows virtual-key codes, as SendInput takes them.
#[cfg(windows)]
pub mod codes {
    pub const CTRL: u16 = 0x11;
    pub const SHIFT: u16 = 0x10;
    pub const ALT: u16 = 0x12;
    pub const WIN: u16 = 0x5B;
    pub const SPACE: u16 = 0x20;
    pub const ENTER: u16 = 0x0D;
    pub const ESC: u16 = 0x1B;
    pub const TAB: u16 = 0x09;

    /// `c` is an uppercase ASCII letter or a digit.
    pub fn char_key(c: u8) -> u16 {
        c as u16
    }

    /// `n` is 1 to 24.
    pub fn fkey(n: u16) -> u16 {
        0x6F + n
    }
}

/// Linux input event codes (`KEY_*` in linux/input-event-codes.h), as uinput takes them.
#[cfg(target_os = "linux")]
pub mod codes {
    pub const CTRL: u16 = 29; // KEY_LEFTCTRL
    pub const SHIFT: u16 = 42; // KEY_LEFTSHIFT
    pub const ALT: u16 = 56; // KEY_LEFTALT
    pub const WIN: u16 = 125; // KEY_LEFTMETA
    pub const SPACE: u16 = 57;
    pub const ENTER: u16 = 28;
    pub const ESC: u16 = 1;
    pub const TAB: u16 = 15;

    /// `c` is an uppercase ASCII letter or a digit.
    pub fn char_key(c: u8) -> u16 {
        const LETTERS: [u16; 26] = [
            30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17, 45, 21, 44,
        ];
        match c {
            b'A'..=b'Z' => LETTERS[(c - b'A') as usize],
            b'0' => 11,
            _ => (c - b'1') as u16 + 2,
        }
    }

    /// `n` is 1 to 24.
    pub fn fkey(n: u16) -> u16 {
        match n {
            1..=10 => 58 + n,
            11 => 87,
            12 => 88,
            _ => 170 + n, // KEY_F13 is 183
        }
    }

    /// Every code a binding can produce, for the uinput keyboard's capabilities.
    pub fn all() -> Vec<u16> {
        let mut v = vec![CTRL, SHIFT, ALT, WIN, SPACE, ENTER, ESC, TAB];
        v.extend((b'A'..=b'Z').chain(b'0'..=b'9').map(char_key));
        v.extend((1..=24).map(fkey));
        v
    }
}

/// Missing file: write the defaults so the user can edit them. Any failure falls back to defaults.
pub fn load(path: &Path) -> Bindings {
    match std::fs::read_to_string(path) {
        Ok(text) => from_toml(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, DEFAULT_TOML);
            Bindings::default()
        }
        Err(_) => Bindings::default(),
    }
}

fn from_toml(text: &str) -> Bindings {
    let Ok(file) = toml::from_str::<File>(text) else {
        return Bindings::default();
    };
    let mut b = Bindings::default();
    let slots = [
        (file.keys.l4, &mut b.l4),
        (file.keys.r4, &mut b.r4),
        (file.keys.pl, &mut b.pl),
        (file.keys.pr, &mut b.pr),
    ];
    for (value, slot) in slots {
        if let Some(Ok(spec)) = value.as_deref().map(parse_keyspec) {
            *slot = spec;
        }
    }
    b
}

#[cfg(test)]
fn test_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("paddle-keys-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn spec(mods: &[u16], key: u16) -> KeySpec {
    KeySpec {
        mods: mods.to_vec(),
        key,
    }
}

#[cfg(test)]
mod tests {
    use super::codes::*;
    use super::*;

    #[test]
    fn function_keys() {
        assert_eq!(parse_keyspec("F13"), Ok(spec(&[], fkey(13))));
        assert_eq!(parse_keyspec("F24"), Ok(spec(&[], fkey(24))));
        assert_eq!(parse_keyspec("f16"), Ok(spec(&[], fkey(16))));
    }

    #[test]
    fn modifiers() {
        assert_eq!(
            parse_keyspec("Ctrl+Shift+F13"),
            Ok(spec(&[CTRL, SHIFT], fkey(13)))
        );
        assert_eq!(parse_keyspec("alt+F1"), Ok(spec(&[ALT], fkey(1))));
        assert_eq!(parse_keyspec("Win+F1"), Ok(spec(&[WIN], fkey(1))));
        assert_eq!(parse_keyspec(" Ctrl + F1 "), Ok(spec(&[CTRL], fkey(1))));
    }

    #[test]
    fn letters_digits_and_named_keys() {
        assert_eq!(parse_keyspec("A"), Ok(spec(&[], char_key(b'A'))));
        assert_eq!(parse_keyspec("F"), Ok(spec(&[], char_key(b'F'))));
        assert_eq!(parse_keyspec("5"), Ok(spec(&[], char_key(b'5'))));
        assert_eq!(parse_keyspec("Space"), Ok(spec(&[], SPACE)));
        assert_eq!(parse_keyspec("Enter"), Ok(spec(&[], ENTER)));
        assert_eq!(parse_keyspec("Esc"), Ok(spec(&[], ESC)));
        assert_eq!(parse_keyspec("Tab"), Ok(spec(&[], TAB)));
    }

    #[test]
    fn invalid_specs_are_errors() {
        assert!(parse_keyspec("").is_err());
        assert!(parse_keyspec("F25").is_err());
        assert!(parse_keyspec("Ctrl+").is_err());
        assert!(parse_keyspec("Banana").is_err());
    }

    #[test]
    fn default_bindings() {
        let b = Bindings::default();
        assert_eq!(b.get(Paddle::L4), &spec(&[], fkey(13)));
        assert_eq!(b.get(Paddle::R4), &spec(&[], fkey(14)));
        assert_eq!(b.get(Paddle::PL), &spec(&[], fkey(15)));
        assert_eq!(b.get(Paddle::PR), &spec(&[], fkey(16)));
    }

    #[test]
    fn default_toml_has_the_defaults() {
        let keys = toml::from_str::<File>(DEFAULT_TOML).unwrap().keys;
        assert_eq!(keys.l4.as_deref(), Some("F13"));
        assert_eq!(keys.r4.as_deref(), Some("F14"));
        assert_eq!(keys.pl.as_deref(), Some("F15"));
        assert_eq!(keys.pr.as_deref(), Some("F16"));
    }

    #[test]
    fn missing_file_writes_defaults_and_returns_them() {
        let dir = test_dir("missing");
        let path = dir.join("sub").join("keys.toml");
        assert_eq!(load(&path), Bindings::default());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), DEFAULT_TOML);
    }

    #[test]
    fn garbage_toml_returns_defaults() {
        let dir = test_dir("garbage");
        let path = dir.join("keys.toml");
        std::fs::write(&path, "this is [[[ not toml").unwrap();
        assert_eq!(load(&path), Bindings::default());
    }

    #[test]
    fn partial_override_keeps_other_defaults() {
        let dir = test_dir("partial");
        let path = dir.join("keys.toml");
        std::fs::write(&path, "[keys]\nL4 = \"Ctrl+F1\"\n").unwrap();
        let b = load(&path);
        assert_eq!(b.get(Paddle::L4), &spec(&[CTRL], fkey(1)));
        assert_eq!(b.get(Paddle::R4), &spec(&[], fkey(14)));
        assert_eq!(b.get(Paddle::PL), &spec(&[], fkey(15)));
        assert_eq!(b.get(Paddle::PR), &spec(&[], fkey(16)));
    }

    #[test]
    fn invalid_value_keeps_only_that_key_default() {
        let dir = test_dir("invalid");
        let path = dir.join("keys.toml");
        std::fs::write(
            &path,
            "[keys]\nL4 = \"Banana\"\nR4 = \"Ctrl+F2\"\n",
        )
        .unwrap();
        let b = load(&path);
        assert_eq!(b.get(Paddle::L4), &spec(&[], fkey(13)));
        assert_eq!(b.get(Paddle::R4), &spec(&[CTRL], fkey(2)));
    }

    #[cfg(windows)]
    #[test]
    fn windows_virtual_keys() {
        assert_eq!(parse_keyspec("Ctrl+Shift+F13"), Ok(spec(&[0x11, 0x10], 0x7C)));
        assert_eq!(parse_keyspec("Win+F24"), Ok(spec(&[0x5B], 0x87)));
        assert_eq!(parse_keyspec("A"), Ok(spec(&[], 0x41)));
        assert_eq!(parse_keyspec("5"), Ok(spec(&[], 0x35)));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_key_codes() {
        // values from linux/input-event-codes.h
        assert_eq!(parse_keyspec("Ctrl+Shift+F13"), Ok(spec(&[29, 42], 183)));
        assert_eq!(parse_keyspec("Win+F24"), Ok(spec(&[125], 194)));
        assert_eq!(parse_keyspec("F1"), Ok(spec(&[], 59)));
        assert_eq!(parse_keyspec("F10"), Ok(spec(&[], 68)));
        assert_eq!(parse_keyspec("F11"), Ok(spec(&[], 87)));
        assert_eq!(parse_keyspec("F12"), Ok(spec(&[], 88)));
        assert_eq!(parse_keyspec("A"), Ok(spec(&[], 30)));
        assert_eq!(parse_keyspec("Q"), Ok(spec(&[], 16)));
        assert_eq!(parse_keyspec("M"), Ok(spec(&[], 50)));
        assert_eq!(parse_keyspec("Z"), Ok(spec(&[], 44)));
        assert_eq!(parse_keyspec("1"), Ok(spec(&[], 2)));
        assert_eq!(parse_keyspec("9"), Ok(spec(&[], 10)));
        assert_eq!(parse_keyspec("0"), Ok(spec(&[], 11)));
        assert_eq!(parse_keyspec("Esc"), Ok(spec(&[], 1)));
        let all = all();
        let unique: std::collections::HashSet<_> = all.iter().collect();
        assert_eq!(unique.len(), all.len(), "no two keys share a code");
    }
}
