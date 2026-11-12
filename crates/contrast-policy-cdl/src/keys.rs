//! キー書式 `mod+shift+Return` のパーサ。パニックしない純関数。
//!
//! 最後の `+` より後ろがキー名（xkb のキー名。英数字と `_` だけ）、それ以前が修飾キー。
//! 修飾キーは `mod` / `alt` / `shift` / `ctrl` / `super`、キー名とともに大文字小文字を区別しない。

use std::fmt;

use contrast_policy::{KeySpec, ModKey, Mods};

/// キー書式の誤り。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    /// 空（空白だけを含む）。
    Empty,
    /// キー名がない（`mod+` など）。
    MissingKey,
    /// 未知の修飾キー。
    UnknownModifier(String),
    /// キー名に使えない文字がある。
    BadKeyName(String),
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "キーの指定が空"),
            Self::MissingKey => write!(
                f,
                "キー名がない（最後は `mod+Return` のようにキー名にする）"
            ),
            Self::UnknownModifier(m) => write!(
                f,
                "未知の修飾キー `{m}`（mod / alt / shift / ctrl / super のどれか）"
            ),
            Self::BadKeyName(k) => {
                write!(f, "キー名 `{k}` に使えない文字がある（英数字と `_` だけ）")
            }
        }
    }
}

/// キー名として使える文字だけか（xkb のキー名: `Return`、`KP_Enter`、`XF86AudioMute`、`1`）。
fn is_key_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `text` を `KeySpec` にする。`mod` は `mod_key` が指す修飾キーになる。
pub fn parse_key_spec(text: &str, mod_key: ModKey) -> Result<KeySpec, KeyError> {
    if text.trim().is_empty() {
        return Err(KeyError::Empty);
    }
    let mut parts = text.split('+').map(str::trim);
    let key = parts.next_back().unwrap_or_default();
    if key.is_empty() {
        return Err(KeyError::MissingKey);
    }
    if !is_key_name(key) {
        return Err(KeyError::BadKeyName(key.to_owned()));
    }
    let mut mods = Mods::default();
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "mod" => {
                let m = mod_key.mods();
                mods.alt |= m.alt;
                mods.logo |= m.logo;
            }
            "alt" => mods.alt = true,
            "shift" => mods.shift = true,
            "ctrl" => mods.ctrl = true,
            "super" => mods.logo = true,
            _ => return Err(KeyError::UnknownModifier(part.to_owned())),
        }
    }
    Ok(KeySpec {
        mods,
        keysym: key.to_ascii_lowercase(),
    })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn parses_mod_and_key() {
        let spec = parse_key_spec("mod+shift+Return", ModKey::Alt);
        assert_eq!(
            spec,
            Ok(KeySpec {
                mods: Mods {
                    alt: true,
                    shift: true,
                    ..Mods::default()
                },
                keysym: "return".to_owned(),
            })
        );
    }

    #[test]
    fn mod_follows_mod_key_and_is_case_insensitive() {
        let spec = parse_key_spec("MOD+Ctrl+Q", ModKey::Super);
        assert_eq!(
            spec,
            Ok(KeySpec {
                mods: Mods {
                    logo: true,
                    ctrl: true,
                    ..Mods::default()
                },
                keysym: "q".to_owned(),
            })
        );
    }

    #[test]
    fn mod_and_alt_are_the_same_when_mod_is_alt() {
        assert_eq!(
            parse_key_spec("alt+mod+x", ModKey::Alt),
            parse_key_spec("mod+x", ModKey::Alt)
        );
    }

    #[test]
    fn bare_key_has_no_modifiers() {
        assert_eq!(
            parse_key_spec("F5", ModKey::Alt).map(|k| (k.mods, k.keysym)),
            Ok((Mods::default(), "f5".to_owned()))
        );
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(parse_key_spec("", ModKey::Alt), Err(KeyError::Empty));
        assert_eq!(parse_key_spec("  ", ModKey::Alt), Err(KeyError::Empty));
        assert_eq!(
            parse_key_spec("mod+", ModKey::Alt),
            Err(KeyError::MissingKey)
        );
        assert_eq!(parse_key_spec("+", ModKey::Alt), Err(KeyError::MissingKey));
        assert_eq!(
            parse_key_spec("hyper+q", ModKey::Alt),
            Err(KeyError::UnknownModifier("hyper".to_owned()))
        );
        assert_eq!(
            parse_key_spec("mod++q", ModKey::Alt),
            Err(KeyError::UnknownModifier(String::new()))
        );
        assert_eq!(
            parse_key_spec("mod+a b", ModKey::Alt),
            Err(KeyError::BadKeyName("a b".to_owned()))
        );
    }

    proptest! {
        /// 任意の文字列でパニックしない。成功したら、キー名は小文字の英数字と `_` だけ。
        #[test]
        fn never_panics(text in any::<String>(), super_mod in any::<bool>()) {
            let mod_key = if super_mod { ModKey::Super } else { ModKey::Alt };
            if let Ok(spec) = parse_key_spec(&text, mod_key) {
                prop_assert!(is_key_name(&spec.keysym));
                prop_assert_eq!(spec.keysym.to_ascii_lowercase(), spec.keysym);
            }
        }

        /// `+` と空白を多く含む入力でもパニックしない。
        #[test]
        fn never_panics_on_plus_heavy_input(text in "[ +a-zA-Z_0-9\\u{3042}]{0,24}") {
            let _ = parse_key_spec(&text, ModKey::Alt);
        }
    }
}
