//! `Env`（読み取り専用・純粋・決定的な入力。`docs/ui-spec.md` §4.3）の Rust 側の型と、`Env` ごとに設定を作る口。
//!
//! CDL を知らない。設定ファイルの中身を `Env` から作る実装（`render` + 変換）は `contrast-policy-cdl` にある。
//!
//! 守ること:
//! - `PolicyEnv` は `title` を持たない（変化が頻繁で、再 `render` が増えるため）。
//! - `Env` が前回と等しければ再 `render` しない（`PartialEq`）。
//! - `Env` で変わってよいのは `Appearance`（`Tiers` を含む）・`Motion`・`Keybind`（と層）だけ。`Layout`・`Workspaces` など
//!   起動時に決まるものは変わらない（`Settings::apply_env_result`）。

use crate::config::ConfigDiagnostic;
use crate::settings::Settings;

/// フォーカス窓の配置状態（`Env.focus.state`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusState {
    /// タイル配置。
    Tiled,
    /// 浮動。
    Floating,
    /// 全画面。
    Fullscreen,
}

impl FocusState {
    /// `Env` に渡す文字列（`"tiled"` / `"floating"` / `"fullscreen"`）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tiled => "tiled",
            Self::Floating => "floating",
            Self::Fullscreen => "fullscreen",
        }
    }
}

/// フォーカス窓（`Env.focus`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusEnv {
    /// アプリケーション ID。無いときは空文字列。
    pub app_id: String,
    /// ダイアログか。
    pub is_dialog: bool,
    /// 配置状態。
    pub state: FocusState,
}

/// アクティブ出力の表示ワークスペース（`Env.workspace`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceEnv {
    /// 位置（1 始まり）。
    pub index: u32,
    /// 名前（名前表にあるときだけ）。
    pub name: Option<String>,
    /// 窓の数。
    pub window_count: u32,
}

/// `Env` の値。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyEnv {
    /// プロファイル名（ホストが決める。既定 `"default"`）。
    pub profile: String,
    /// キー層の名前（既定 `"normal"`）。
    pub mode: String,
    /// アクティブ出力の名前。
    pub output_name: String,
    /// アクティブ出力の表示ワークスペース。
    pub workspace: WorkspaceEnv,
    /// フォーカス窓（無ければ `None`）。
    pub focus: Option<FocusEnv>,
}

/// プロファイルの既定名。
pub const DEFAULT_PROFILE: &str = "default";

/// プロファイルの置き場所（`$XDG_CONFIG_HOME` または `$HOME/.config` からの相対。1 行のテキスト）。
pub const PROFILE_RELATIVE_PATH: &str = "contrast/profile";

/// プロファイル名を決める。環境変数 `CONTRAST_PROFILE`（`env_value`）が優先で、無ければ `profile` ファイルの最初の行
/// （`file_text`）、どちらも空なら `DEFAULT_PROFILE`。前後の空白は取る。引数は外から渡す（テストしやすいように）。
pub fn resolve_profile(env_value: Option<&str>, file_text: Option<&str>) -> String {
    let clean = |s: &str| s.trim().to_owned();
    env_value
        .map(clean)
        .filter(|p| !p.is_empty())
        .or_else(|| {
            file_text
                .and_then(|t| t.lines().next())
                .map(clean)
                .filter(|p| !p.is_empty())
        })
        .unwrap_or_else(|| DEFAULT_PROFILE.to_owned())
}

/// `profile` ファイルのパス（`$XDG_CONFIG_HOME/contrast/profile`、無ければ `$HOME/.config/contrast/profile`）。
pub fn profile_file_path(
    xdg_config_home: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<std::path::PathBuf> {
    xdg_config_home
        .filter(|p| !p.is_empty())
        .map(|x| std::path::PathBuf::from(x).join(PROFILE_RELATIVE_PATH))
        .or_else(|| {
            home.filter(|h| !h.is_empty()).map(|h| {
                std::path::PathBuf::from(h)
                    .join(".config")
                    .join(PROFILE_RELATIVE_PATH)
            })
        })
}

impl PolicyEnv {
    /// 読み込み時の検査に使う、フォーカスなし・ワークスペース 1・窓 0 の `Env`。
    pub fn baseline(profile: &str) -> Self {
        Self {
            profile: profile.to_owned(),
            mode: crate::settings::NORMAL_MODE.to_owned(),
            output_name: String::new(),
            workspace: WorkspaceEnv {
                index: 1,
                name: None,
                window_count: 0,
            },
            focus: None,
        }
    }

    /// 読み込み時の検査に使う代表の `Env`: フォーカスなし、と、フォーカスあり（`state` 3 値。`appId` は空文字列）。
    /// 最初の要素が基準（`Config.settings` の元）。
    pub fn representatives(profile: &str) -> Vec<Self> {
        let mut all = vec![Self::baseline(profile)];
        for state in [
            FocusState::Tiled,
            FocusState::Floating,
            FocusState::Fullscreen,
        ] {
            let mut env = Self::baseline(profile);
            env.workspace.window_count = 1;
            env.focus = Some(FocusEnv {
                app_id: String::new(),
                is_dialog: false,
                state,
            });
            all.push(env);
        }
        all
    }
}

/// `Env` から設定（`Settings`）を作る口。設定ファイルが関数形ルートのときだけある。
pub trait EnvRenderer {
    /// `env` で設定を作る。失敗は診断（CT081 など）。
    fn render(&self, env: &PolicyEnv) -> Result<Settings, Vec<ConfigDiagnostic>>;
}

/// `Policy::update_env` の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvUpdate {
    /// 変化なし（`Env` が前回と等しい、または設定が関数形ルートでない）。再 `render` しなかった。
    Unchanged,
    /// 新しい設定に変わった（`Appearance`・`Motion`・`Keybind` のいずれかが違う）。`Env` で変わってはいけない項目が
    /// 違っていたときは、無視した旨の警告（CT050）が入る。
    Applied(Vec<ConfigDiagnostic>),
    /// `render`・変換に失敗した。直前の設定を保持した（CT080 の警告。同じ `Env` では 1 回だけ返す）。
    Failed(ConfigDiagnostic),
}

impl Settings {
    /// `Env` で作った設定 `rendered` のうち、`Env` で変わってよい部分（`Appearance`・`Motion`・`Keybind`・層）だけを
    /// `self`（基準）に反映した設定と、**変わってはいけないのに違っていた**項目の名前を返す（呼び出し側が警告にする）。
    #[must_use]
    pub fn apply_env_result(&self, rendered: &Settings) -> (Settings, Vec<&'static str>) {
        let mut merged = self.clone();
        merged.appearance = rendered.appearance.clone();
        merged.motion = rendered.motion;
        merged.keybinds = rendered.keybinds.clone();
        merged.modes = rendered.modes.clone();
        let mut ignored = Vec::new();
        let fixed: [(&'static str, bool); 9] = [
            ("layout", self.layout != rendered.layout),
            ("workspaces", self.workspaces != rendered.workspaces),
            (
                "workspace_mode",
                self.workspace_mode != rendered.workspace_mode,
            ),
            (
                "named_workspaces",
                self.named_workspaces != rendered.named_workspaces,
            ),
            ("startup", self.startup != rendered.startup),
            ("environment", self.environment != rendered.environment),
            ("cursor", self.cursor != rendered.cursor),
            ("mod_key", self.mod_key != rendered.mod_key),
            ("zoom", self.zoom != rendered.zoom),
        ];
        for (name, differs) in fixed {
            if differs {
                ignored.push(name);
            }
        }
        (merged, ignored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_representatives_start_with_the_baseline_and_cover_every_focus_state() {
        let all = PolicyEnv::representatives("p");
        assert_eq!(all.first(), Some(&PolicyEnv::baseline("p")));
        let states: Vec<_> = all
            .iter()
            .filter_map(|e| e.focus.as_ref().map(|f| f.state))
            .collect();
        assert_eq!(
            states,
            [
                FocusState::Tiled,
                FocusState::Floating,
                FocusState::Fullscreen
            ]
        );
        assert!(all.iter().all(|e| e.profile == "p" && e.mode == "normal"));
    }

    #[test]
    fn the_profile_comes_from_the_env_var_then_the_file_then_the_default() {
        assert_eq!(resolve_profile(Some(" dim "), Some("other\n")), "dim");
        assert_eq!(resolve_profile(None, Some("other\nsecond\n")), "other");
        assert_eq!(resolve_profile(Some("  "), Some(" b ")), "b");
        assert_eq!(resolve_profile(Some(""), Some("")), "default");
        assert_eq!(resolve_profile(None, None), "default");
    }

    #[test]
    fn the_profile_file_is_under_the_xdg_config_home_or_dot_config() {
        use std::ffi::OsString;
        use std::path::PathBuf;
        let os = |s: &str| Some(OsString::from(s));
        assert_eq!(
            profile_file_path(os("/x"), os("/h")),
            Some(PathBuf::from("/x/contrast/profile"))
        );
        assert_eq!(
            profile_file_path(os(""), os("/h")),
            Some(PathBuf::from("/h/.config/contrast/profile"))
        );
        assert_eq!(profile_file_path(None, None), None);
    }

    #[test]
    fn env_equality_is_structural() {
        let a = PolicyEnv::baseline("x");
        let mut b = a.clone();
        assert_eq!(a, b);
        b.workspace.window_count = 1;
        assert_ne!(a, b);
    }

    #[test]
    fn only_appearance_motion_and_keybinds_follow_the_env() {
        let base = Settings::builtin();
        let mut rendered = base.clone();
        rendered.appearance.radius = 7;
        rendered.motion = Some(crate::motion::MotionSettings::default());
        rendered.keybinds.clear();
        rendered.workspaces = 9;
        rendered.layout.gap = 5;
        let (merged, ignored) = base.apply_env_result(&rendered);
        assert_eq!(merged.appearance.radius, 7);
        assert!(merged.motion.is_some());
        assert!(merged.keybinds.is_empty());
        // 起動時に決まるものは変わらず、違っていたことが分かる。
        assert_eq!(merged.workspaces, base.workspaces);
        assert_eq!(merged.layout, base.layout);
        assert_eq!(ignored, ["layout", "workspaces"]);
        // 同じなら何も無視しない。
        assert!(base.apply_env_result(&base).1.is_empty());
    }

    #[test]
    fn focus_state_names_match_the_env_literals() {
        assert_eq!(FocusState::Tiled.as_str(), "tiled");
        assert_eq!(FocusState::Floating.as_str(), "floating");
        assert_eq!(FocusState::Fullscreen.as_str(), "fullscreen");
    }
}
