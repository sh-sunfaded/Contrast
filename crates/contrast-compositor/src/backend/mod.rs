//! バックエンド。ホストの Wayland 上の winit 窓と、実機の DRM（フェーズ1: 出力の列挙のみ）を持つ。

pub mod drm;
mod vt_guard;
pub mod winit;

use std::fmt;

use contrast_policy::ModKey;

/// 使うバックエンド。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// ホストの Wayland 上の winit 窓（開発用）。
    Winit,
    /// 実機の DRM。
    Drm,
}

/// バックエンドを決められなかった理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectError {
    /// `CONTRAST_BACKEND` が `winit` / `drm` のどちらでもない。
    Unknown(String),
    /// 未指定で、ホストの Wayland / X11 も無い。
    NoHost,
}

impl fmt::Display for SelectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SelectError::Unknown(v) => write!(
                f,
                "CONTRAST_BACKEND={v} は不明です。winit か drm を指定してください"
            ),
            SelectError::NoHost => write!(
                f,
                "WAYLAND_DISPLAY も DISPLAY も無いため winit は使えません。\
                 DRM（実機）を使うなら CONTRAST_BACKEND=drm を指定してください"
            ),
        }
    }
}

impl std::error::Error for SelectError {}

/// 環境変数の値からバックエンドを選ぶ。DRM は `drm` を明示したときだけ有効。
///
/// `choice` は `CONTRAST_BACKEND`、`has_host_display` は `WAYLAND_DISPLAY` か `DISPLAY` があるか。
pub fn select(choice: Option<&str>, has_host_display: bool) -> Result<Kind, SelectError> {
    match choice {
        Some("winit") => Ok(Kind::Winit),
        Some("drm") => Ok(Kind::Drm),
        Some(other) => Err(SelectError::Unknown(other.to_string())),
        None if has_host_display => Ok(Kind::Winit),
        None => Err(SelectError::NoHost),
    }
}

/// `mod` の既定: DRM（実機の TTY）は Super、winit（nested）は Alt（ホストが Super を奪うため）。
pub fn default_mod_key(kind: Kind) -> ModKey {
    match kind {
        Kind::Drm => ModKey::Super,
        Kind::Winit => ModKey::Alt,
    }
}

/// `mod` の既定を決める。`CONTRAST_MOD_KEY`（`alt` / `super`）> バックエンドの既定。
/// 環境変数が不正なら無視する（`Err` に値を入れて返し、呼び出し側が警告する）。
/// CDL の `modKey` は、この結果より優先される（読み込み器が処理する）。
pub fn resolve_mod_key(env: Option<&str>, kind: Kind) -> (ModKey, Option<String>) {
    match env {
        None => (default_mod_key(kind), None),
        Some(v) => match ModKey::parse(v) {
            Some(m) => (m, None),
            None => (default_mod_key(kind), Some(v.to_string())),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_key_priority_is_env_then_backend() {
        assert_eq!(resolve_mod_key(None, Kind::Drm), (ModKey::Super, None));
        assert_eq!(resolve_mod_key(None, Kind::Winit), (ModKey::Alt, None));
        assert_eq!(resolve_mod_key(Some("alt"), Kind::Drm), (ModKey::Alt, None));
        assert_eq!(
            resolve_mod_key(Some("super"), Kind::Winit),
            (ModKey::Super, None)
        );
        assert_eq!(
            resolve_mod_key(Some("hyper"), Kind::Drm),
            (ModKey::Super, Some("hyper".to_string()))
        );
    }

    #[test]
    fn explicit_choice_wins() {
        assert_eq!(select(Some("drm"), true), Ok(Kind::Drm));
        assert_eq!(select(Some("winit"), false), Ok(Kind::Winit));
    }

    #[test]
    fn unset_needs_a_host_display() {
        assert_eq!(select(None, true), Ok(Kind::Winit));
        assert_eq!(select(None, false), Err(SelectError::NoHost));
    }

    #[test]
    fn unknown_value_is_an_error() {
        assert_eq!(
            select(Some("x11"), true),
            Err(SelectError::Unknown("x11".to_string()))
        );
    }
}
