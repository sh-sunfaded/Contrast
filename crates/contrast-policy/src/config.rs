//! 設定の入口: 設定ファイルのパスの解決と、読み込み器 `ConfigLoader`、設定の診断 `ConfigDiagnostic`。
//!
//! 読み込み器の実装は別のクレート（`contrast-policy-cdl`）にある。このモジュールは CDL の型を一切持たない。

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::store::Config;

/// 設定ファイルのパスを上書きする環境変数。
pub const CONFIG_ENV: &str = "CONTRAST_CONFIG";

/// `$HOME` からの既定の設定ファイルの相対パス。
pub const DEFAULT_RELATIVE_PATH: &str = ".config/contrast/config.cdl";

/// `$XDG_CONFIG_HOME` からの設定ファイルの相対パス。
pub const XDG_RELATIVE_PATH: &str = "contrast/config.cdl";

/// 設定ファイルのパスを決める。
///
/// 優先順は `CONTRAST_CONFIG` → `$XDG_CONFIG_HOME/contrast/config.cdl` → `$HOME/.config/contrast/config.cdl`。
/// 空の環境変数は無いものとして扱う。どれも使えなければ `None`。引数は環境変数の値（テストしやすいよう外から渡す）。
pub fn resolve_config_path(
    contrast_config: Option<OsString>,
    xdg_config_home: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    if let Some(path) = contrast_config.filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(path));
    }
    if let Some(xdg) = xdg_config_home.filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(xdg).join(XDG_RELATIVE_PATH));
    }
    home.filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(DEFAULT_RELATIVE_PATH))
}

/// 診断の重大度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    /// 誤り。
    Error,
    /// 警告。
    Warning,
    /// 補足。
    Hint,
}

impl DiagnosticSeverity {
    /// ログ用の名前。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Hint => "hint",
        }
    }
}

/// 設定の読み込み・変換で出た診断（読み込み器の実装には依存しない）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigDiagnostic {
    /// 重大度。
    pub severity: DiagnosticSeverity,
    /// 診断コード（例: `E4004`）。読み込み器が決める。
    pub code: String,
    /// 位置のあるファイル名。位置がなければ空。
    pub file: String,
    /// 1 始まりの行。位置を求められなければ `None`。
    pub line: Option<u32>,
    /// 1 始まりの列（文字単位）。位置を求められなければ `None`。
    pub column: Option<u32>,
    /// メッセージ。
    pub message: String,
}

impl fmt::Display for ConfigDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}[{}] ", self.severity.as_str(), self.code)?;
        if !self.file.is_empty() {
            write!(f, "{}", self.file)?;
            if let (Some(line), Some(column)) = (self.line, self.column) {
                write!(f, ":{line}:{column}")?;
            }
            write!(f, ": ")?;
        }
        write!(f, "{}", self.message)
    }
}

/// 読み込みの結果。
pub enum LoadOutcome {
    /// 読み込めた。この設定で動く。
    Loaded(Box<Config>),
    /// 設定ファイルが無い。エラーではない（既定値で動く）。
    Absent,
    /// 読み込み・評価・変換のどれかに失敗した。診断は 1 件以上。半端な設定は返さない。
    Failed(Vec<ConfigDiagnostic>),
}

/// 設定ファイルから `Config` を作る読み込み器。
pub trait ConfigLoader {
    /// ログ用の名前。
    fn name(&self) -> &'static str;

    /// `path` の設定を読み込む。`path` が無ければ `Absent`。
    fn load(&self, path: &Path) -> LoadOutcome;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(s: &str) -> Option<OsString> {
        Some(OsString::from(s))
    }

    #[test]
    fn env_overrides_everything() {
        assert_eq!(
            resolve_config_path(os("/tmp/x.cdl"), os("/xdg"), os("/home/u")),
            Some(PathBuf::from("/tmp/x.cdl"))
        );
    }

    #[test]
    fn xdg_then_home() {
        assert_eq!(
            resolve_config_path(None, os("/xdg"), os("/home/u")),
            Some(PathBuf::from("/xdg/contrast/config.cdl"))
        );
        assert_eq!(
            resolve_config_path(os(""), os(""), os("/home/u")),
            Some(PathBuf::from("/home/u/.config/contrast/config.cdl"))
        );
        assert_eq!(resolve_config_path(None, None, None), None);
    }

    #[test]
    fn diagnostic_display_with_and_without_location() {
        let mut d = ConfigDiagnostic {
            severity: DiagnosticSeverity::Error,
            code: "E4004".to_string(),
            file: "/c/config.cdl".to_string(),
            line: Some(5),
            column: Some(3),
            message: "型が合わない".to_string(),
        };
        assert_eq!(
            d.to_string(),
            "error[E4004] /c/config.cdl:5:3: 型が合わない"
        );
        d.line = None;
        assert_eq!(d.to_string(), "error[E4004] /c/config.cdl: 型が合わない");
        d.file.clear();
        assert_eq!(d.to_string(), "error[E4004] 型が合わない");
    }
}
