//! トップレベル窓: `Window`、`WindowMeta`、`WindowState`。
//!
//! 守る不変条件:
//! - I1 窓はちょうど 1 つのワークスペースに属する。`Window::workspace` はその所属先で、
//!   ワークスペースの順序列と常に一致させる（一致の検査は `invariant.rs`）。

use alloc::string::String;

use crate::domain::geometry::{Rect, Size};
use crate::domain::id::{WindowId, WorkspaceId};

/// 窓の性質。シェルがクライアントから読み取って渡す。
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowMeta {
    /// アプリケーション ID。
    pub app_id: Option<String>,
    /// タイトル。
    pub title: Option<String>,
    /// ダイアログ（親を持つ窓など）なら true。
    pub is_dialog: bool,
    /// クライアントが希望する大きさ。
    pub requested_size: Option<Size>,
}

/// 窓の配置状態。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowState {
    /// タイル配置（Layout が矩形を決める）。
    Tiled,
    /// 浮動（D8 の矩形）。
    Floating,
    /// 全画面（出力の論理矩形全体、D9）。
    Fullscreen,
}

/// core が管理する窓 1 枚。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    /// ID。
    pub id: WindowId,
    /// 性質。
    pub meta: WindowMeta,
    /// 現在の配置状態。
    pub state: WindowState,
    /// 全画面にする前の状態。全画面を解くときにここへ戻す（D9）。
    pub restore_state: WindowState,
    /// 属するワークスペースの id（I1。位置ではなく id で参照する。D73）。
    pub workspace: WorkspaceId,
    /// 最後に計算した矩形。一度も表示されていなければ `None`。
    pub rect: Option<Rect>,
    /// 浮動時の矩形（D8）。一度も浮動していなければ `None`。
    pub floating_rect: Option<Rect>,
}

impl Window {
    /// 配置先と状態を決めた新しい窓を作る。矩形は未計算。
    #[must_use]
    pub const fn new(
        id: WindowId,
        meta: WindowMeta,
        state: WindowState,
        workspace: WorkspaceId,
    ) -> Self {
        Self {
            id,
            meta,
            state,
            restore_state: WindowState::Tiled,
            workspace,
            rect: None,
            floating_rect: None,
        }
    }
}
