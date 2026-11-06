//! 設定の値（`Settings`）。CDL を知らない。読み込み器（`ConfigLoader`）が作り、`Config` に入る。
//!
//! 組み込みの既定値は `Settings::builtin`。設定ファイルが無い・壊れているときはこれで動く。

use std::collections::BTreeMap;

use crate::port::{Action, Mods};

/// ワークスペース数の上限。
pub const MAX_WORKSPACES: u8 = 32;

/// 枠の太さの上限（px）。
pub const MAX_BORDER_WIDTH: i32 = 256;

/// キーバインドの `mod` が指す修飾キー。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModKey {
    /// Alt（既定。nested で動かすとき向け）。
    #[default]
    Alt,
    /// Super / Logo。
    Super,
}

impl ModKey {
    /// `"alt"` / `"super"`（大文字小文字は区別しない）を `ModKey` にする。それ以外は `None`。
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "alt" => Some(Self::Alt),
            "super" => Some(Self::Super),
            _ => None,
        }
    }

    /// `mod` を `Mods` にしたもの。
    pub fn mods(self) -> Mods {
        match self {
            Self::Alt => Mods {
                alt: true,
                ..Mods::default()
            },
            Self::Super => Mods {
                logo: true,
                ..Mods::default()
            },
        }
    }
}

/// キーの指定: 修飾キーの集合とキー名（xkb のキー名を小文字にしたもの）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySpec {
    /// 修飾キー。押されている修飾キーとちょうど一致したときだけ当たる。
    pub mods: Mods,
    /// キー名（小文字。比較は大文字小文字を区別しない）。
    pub keysym: String,
}

impl KeySpec {
    /// 押されたキーに当たるか。キー名は大文字小文字を区別しない。
    pub fn matches(&self, mods: Mods, keysym: &str) -> bool {
        self.mods == mods && self.keysym.eq_ignore_ascii_case(keysym)
    }
}

/// 1 つのキーバインド。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    /// 当たるキー。
    pub key: KeySpec,
    /// 実行する操作。
    pub action: Action,
}

/// 見た目の設定。色は RGBA（0.0〜1.0）。
#[derive(Debug, Clone, PartialEq)]
pub struct Appearance {
    /// 枠の太さ（px、0 で枠なし）。
    pub border_width: i32,
    /// フォーカス窓の枠の色。
    pub focus_color: [f32; 4],
    /// フォーカス外の窓の枠の色。`None` なら描かない。
    pub border_color: Option<[f32; 4]>,
    /// 背景色。
    pub background: [f32; 4],
    /// 窓と枠の角の丸め半径（px、0 で角丸なし）。
    pub radius: i32,
    /// 窓の影。`blur` が 0 なら描かない。
    pub shadow: Shadow,
    /// フォーカス外の窓の枠の太さ（px）。`None` なら `border_width` と同じ。窓の大きさは `border_width` で決まる。
    pub inactive_border_width: Option<i32>,
}

/// 窓の影。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    /// ぼかしの幅（px）。0 で影なし。
    pub blur: i32,
    /// 影の横のずれ（px）。
    pub offset_x: i32,
    /// 影の縦のずれ（px）。
    pub offset_y: i32,
    /// 影の色（RGBA）。
    pub color: [f32; 4],
}

impl Default for Shadow {
    fn default() -> Self {
        Self {
            blur: 0,
            offset_x: 0,
            offset_y: 0,
            color: [0.0, 0.0, 0.0, 0.5],
        }
    }
}

/// 窓の並べ方の設定。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LayoutSettings {
    /// 窓と窓の間（px）。
    pub gap: i32,
    /// 出力の端と窓の間（px）。
    pub outer_gap: i32,
    /// 窓の並べ方（`Equal`: 等分、`Scrolling`: スクロール列）。起動時に決まり、リロードでは変わらない。
    pub mode: contrast_core::LayoutMode,
    /// 新しい列の幅。`None` なら core の既定（1/2）。
    pub default_column_width: Option<contrast_core::Ratio>,
    /// プリセットの列幅。空なら core の既定（1/3・1/2・2/3）。
    pub preset_column_widths: Vec<contrast_core::Ratio>,
    /// プリセットの窓の高さ。空なら core の既定（1/3・1/2・2/3）。
    pub window_height_presets: Vec<contrast_core::Ratio>,
    /// 窓の移動・大きさの変化・ワークスペース切替の補間にかける時間（ミリ秒）。0 で補間しない。
    pub animation_ms: u32,
}

/// `set-window-height` の `heightPx` の上限（px）。
pub const MAX_WINDOW_HEIGHT_PX: u32 = 16_384;

/// `animation_ms` の上限（ミリ秒）。
pub const MAX_ANIMATION_MS: u32 = 2000;

/// カーソルの設定。`None` は「設定なし」（環境変数 `XCURSOR_THEME` / `XCURSOR_SIZE`、無ければ既定）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CursorSettings {
    /// xcursor のテーマ名。
    pub theme: Option<String>,
    /// カーソルの大きさ（px）。
    pub size: Option<u32>,
    /// 入力がないまま、この時間（ミリ秒）が過ぎたらカーソルを隠す。`None` なら隠さない。動かすと出る。
    pub hide_after_inactive_ms: Option<u32>,
}

/// `hide_after_inactive_ms` の上限（ミリ秒。1 時間）。
pub const MAX_HIDE_AFTER_INACTIVE_MS: u32 = 3_600_000;

/// 隙間・角丸などの上限（px）。
pub const MAX_DECORATION_PX: i32 = 512;

/// タッチパッドなどの入力設定。libinput のデバイスに適用する（デバイスが対応しない項目は無視される）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputSettings {
    /// タップでクリックする。
    pub tap_to_click: bool,
    /// 自然なスクロール（指と同じ向きに動く）。
    pub natural_scroll: bool,
    /// ポインタの加速（-1.0〜1.0、0 が既定）。
    pub accel_speed: f64,
    /// タイピング中はタッチパッドを無効にする。
    pub disable_while_typing: bool,
    /// ポインタが窓に入ったら、その窓にフォーカスを移す。
    pub focus_follows_mouse: bool,
}

impl Default for InputSettings {
    fn default() -> Self {
        Self {
            tap_to_click: true,
            natural_scroll: false,
            accel_speed: 0.0,
            disable_while_typing: true,
            focus_follows_mouse: false,
        }
    }
}

/// 設定の値。
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// `mod` が指す修飾キー。
    pub mod_key: ModKey,
    /// 見た目。
    pub appearance: Appearance,
    /// ワークスペースの数（1〜`MAX_WORKSPACES`）。
    pub workspaces: u8,
    /// 入力デバイスの設定。
    pub input: InputSettings,
    /// 窓の隙間。
    pub layout: LayoutSettings,
    /// カーソル。
    pub cursor: CursorSettings,
    /// 起動時に一度だけ実行するコマンド（並びの順。空白で区切った先頭がプログラム、残りが引数）。リロードでは再実行しない。
    pub startup: Vec<String>,
    /// spawn する子プロセスに足す環境変数（名前順）。コンポジタ自身の環境は変えない。
    pub environment: BTreeMap<String, String>,
    /// wlr-data-control（フォーカスなしでクリップボードを読み書きするプロトコル）を使ってよいクライアントの
    /// プログラム名（実行ファイルの名前）。ここに無いクライアントには、そのグローバルを見せない。
    pub data_control_clients: Vec<String>,
    /// キーバインド（並びの順）。
    pub keybinds: Vec<KeyBinding>,
}

/// `data_control_clients` の既定。
pub const DEFAULT_DATA_CONTROL_CLIENTS: [&str; 5] = [
    "ironbar",
    "wl-copy",
    "wl-paste",
    "cliphist",
    "wl-clip-persist",
];

/// 8 ビットの RGBA を 0.0〜1.0 にする。
pub fn rgba8(c: [u8; 4]) -> [f32; 4] {
    c.map(|v| f32::from(v) / 255.0)
}

impl Settings {
    /// 組み込みの既定値。端末は `foot`。
    pub fn builtin() -> Self {
        Self::builtin_with_terminal("foot")
    }

    /// 組み込みの既定値。`terminal` は Alt+Return で起動するコマンド。
    pub fn builtin_with_terminal(terminal: impl Into<String>) -> Self {
        let alt = ModKey::Alt.mods();
        let alt_shift = Mods { shift: true, ..alt };
        let bind = |mods: Mods, key: &str, action: Action| KeyBinding {
            key: KeySpec {
                mods,
                keysym: key.to_owned(),
            },
            action,
        };
        let mut keybinds = vec![
            bind(alt, "return", Action::Spawn(terminal.into())),
            bind(alt, "q", Action::CloseFocused),
            bind(alt, "left", Action::FocusPrev),
            bind(alt, "right", Action::FocusNext),
        ];
        for (key, index) in [("1", 0u8), ("2", 1), ("3", 2), ("4", 3)] {
            keybinds.push(bind(alt, key, Action::SwitchWorkspace(index)));
        }
        keybinds.push(bind(alt_shift, "q", Action::Quit));
        Self {
            mod_key: ModKey::Alt,
            appearance: Appearance {
                border_width: 3,
                focus_color: rgba8([0x66, 0xa6, 0xff, 0xff]),
                border_color: None,
                background: rgba8([0x1f, 0x24, 0x33, 0xff]),
                radius: 0,
                shadow: Shadow::default(),
                inactive_border_width: None,
            },
            workspaces: 4,
            input: InputSettings::default(),
            layout: LayoutSettings::default(),
            cursor: CursorSettings::default(),
            startup: Vec::new(),
            environment: BTreeMap::new(),
            data_control_clients: DEFAULT_DATA_CONTROL_CLIENTS
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
            keybinds,
        }
    }

    /// 押されたキーに当たる操作。最初に当たったもの。
    pub fn resolve_key(&self, mods: Mods, keysym: &str) -> Option<Action> {
        self.keybinds
            .iter()
            .find(|b| b.key.matches(mods, keysym))
            .map(|b| b.action.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_keys_resolve() {
        let s = Settings::builtin();
        let alt = ModKey::Alt.mods();
        assert_eq!(
            s.resolve_key(alt, "Return"),
            Some(Action::Spawn("foot".into()))
        );
        assert_eq!(s.resolve_key(alt, "Q"), Some(Action::CloseFocused));
        let alt_shift = Mods { shift: true, ..alt };
        assert_eq!(s.resolve_key(alt_shift, "q"), Some(Action::Quit));
        assert_eq!(s.resolve_key(alt, "4"), Some(Action::SwitchWorkspace(3)));
        assert_eq!(s.resolve_key(alt, "5"), None);
        assert_eq!(s.resolve_key(Mods::default(), "q"), None);
        assert_eq!(s.keybinds.len(), 9);
    }

    #[test]
    fn mod_key_parses_names() {
        assert_eq!(ModKey::parse("alt"), Some(ModKey::Alt));
        assert_eq!(ModKey::parse(" Super "), Some(ModKey::Super));
        assert_eq!(ModKey::parse("hyper"), None);
        assert_eq!(ModKey::parse(""), None);
    }

    #[test]
    fn super_mod_key_uses_logo() {
        assert!(ModKey::Super.mods().logo);
        assert!(!ModKey::Super.mods().alt);
    }
}
