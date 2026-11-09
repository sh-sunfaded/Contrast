//! ポリシーのフック `windowOpen` の口。CDL を知らない。実装は読み込み器の側にある。
//!
//! ワークスペース番号はここでは 0 始まり（CDL の 1 始まりとの変換は実装側で行う）。

use crate::config::ConfigDiagnostic;

/// フックに渡す新しい窓の情報。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    /// アプリケーション ID。無いときは空文字列。
    pub app_id: String,
    /// タイトル。
    pub title: Option<String>,
    /// ダイアログか。
    pub is_dialog: bool,
    /// フォーカス中のワークスペース（0 始まり）。
    pub workspace: u8,
}

/// フックが返した配置（形の検査は済んでいる。ワークスペースの範囲は `Config` が検査する）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookPlacement {
    /// 配置先のワークスペース（0 始まり）。`None` ならフォーカス中のワークスペース。
    pub workspace: Option<u8>,
    /// Floating で置くか。
    pub floating: bool,
    /// 配置先が表示中ならフォーカスを与えるか。
    pub focus: bool,
    /// 開いた直後に全画面にするか。
    pub fullscreen: bool,
    /// 最初の列の幅（`columnWidth`）。
    pub column_width: Option<crate::PlacementWidth>,
}

/// 新しい窓が開いたときに呼ばれるフック。
pub trait PlacementHook {
    /// 配置を決める。フックが定義されていなければ `Ok(None)`。
    /// 評価の失敗や戻り値の不正は診断で返す（呼び出し側が既定の配置に戻す）。
    fn window_open(&self, window: &WindowInfo) -> Result<Option<HookPlacement>, ConfigDiagnostic>;
}
