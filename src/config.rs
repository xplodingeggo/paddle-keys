#![allow(dead_code)] // config::load unused until Task 5

use std::path::{Path, PathBuf};

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
            l4: spec(&[], 0x7C),
            r4: spec(&[], 0x7D),
            pl: spec(&[], 0x7E),
            pr: spec(&[], 0x7F),
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
        "ctrl" => Ok(0x11),
        "shift" => Ok(0x10),
        "alt" => Ok(0x12),
        "win" => Ok(0x5B),
        _ => Err(format!("unknown modifier {t:?}")),
    }
}

fn key_code(t: &str) -> Result<u16, String> {
    let lower = t.to_ascii_lowercase();
    let named = match lower.as_str() {
        "space" => Some(0x20),
        "enter" => Some(0x0D),
        "esc" => Some(0x1B),
        "tab" => Some(0x09),
        _ => None,
    };
    if let Some(k) = named {
        return Ok(k);
    }
    // Single char first, so "F" is the letter F and not a malformed F-key.
    if t.len() == 1 && t.as_bytes()[0].is_ascii_alphanumeric() {
        return Ok(t.as_bytes()[0].to_ascii_uppercase() as u16);
    }
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u16>().ok()) {
        if (1..=24).contains(&n) {
            return Ok(0x6F + n);
        }
    }
    Err(format!("unknown key {t:?}"))
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

fn test_dir(name: &str) -> PathBuf {
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
    use super::*;

    #[test]
    fn function_keys() {
        assert_eq!(parse_keyspec("F13"), Ok(spec(&[], 0x7C)));
        assert_eq!(parse_keyspec("F24"), Ok(spec(&[], 0x87)));
        assert_eq!(parse_keyspec("f16"), Ok(spec(&[], 0x7F)));
    }

    #[test]
    fn modifiers() {
        assert_eq!(
            parse_keyspec("Ctrl+Shift+F13"),
            Ok(spec(&[0x11, 0x10], 0x7C))
        );
        assert_eq!(parse_keyspec("alt+F1"), Ok(spec(&[0x12], 0x70)));
        assert_eq!(parse_keyspec("Win+F1"), Ok(spec(&[0x5B], 0x70)));
        assert_eq!(parse_keyspec(" Ctrl + F1 "), Ok(spec(&[0x11], 0x70)));
    }

    #[test]
    fn letters_digits_and_named_keys() {
        assert_eq!(parse_keyspec("A"), Ok(spec(&[], 0x41)));
        assert_eq!(parse_keyspec("F"), Ok(spec(&[], 0x46)));
        assert_eq!(parse_keyspec("5"), Ok(spec(&[], 0x35)));
        assert_eq!(parse_keyspec("Space"), Ok(spec(&[], 0x20)));
        assert_eq!(parse_keyspec("Enter"), Ok(spec(&[], 0x0D)));
        assert_eq!(parse_keyspec("Esc"), Ok(spec(&[], 0x1B)));
        assert_eq!(parse_keyspec("Tab"), Ok(spec(&[], 0x09)));
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
        assert_eq!(b.get(Paddle::L4), &spec(&[], 0x7C));
        assert_eq!(b.get(Paddle::R4), &spec(&[], 0x7D));
        assert_eq!(b.get(Paddle::PL), &spec(&[], 0x7E));
        assert_eq!(b.get(Paddle::PR), &spec(&[], 0x7F));
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
        assert_eq!(b.get(Paddle::L4), &spec(&[0x11], 0x70));
        assert_eq!(b.get(Paddle::R4), &spec(&[], 0x7D));
        assert_eq!(b.get(Paddle::PL), &spec(&[], 0x7E));
        assert_eq!(b.get(Paddle::PR), &spec(&[], 0x7F));
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
        assert_eq!(b.get(Paddle::L4), &spec(&[], 0x7C));
        assert_eq!(b.get(Paddle::R4), &spec(&[0x11], 0x71));
    }
}
