//! core からの出力: `Effect` と `Applied`。
//!
//! Effect 列の順序は仕様 5 節で固定: Registered → ShowWorkspace（OutputId 昇順）→
//! Configure（WindowId 昇順）→ SetFocus（最大 1 個）→ RequestClose → Damage（OutputId 昇順、重複なし）。

use alloc::vec::Vec;

use crate::domain::geometry::Rect;
use crate::domain::id::{EntityId, ExternalKey, OutputId, WindowId, WorkspaceId};
use crate::domain::window::WindowState;

/// シェルが順に実行する指示。
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// `key` を `id` として登録した。
    Registered {
        /// シェル側の識別子。
        key: ExternalKey,
        /// 払い出した ID。
        id: EntityId,
    },
    /// 出力の表示ワークスペースが変わった。
    ShowWorkspace {
        /// 出力。
        output: OutputId,
        /// 表示するワークスペース。
        workspace: WorkspaceId,
    },
    /// 表示中の窓の矩形か状態が変わった。
    Configure {
        /// 窓。
        window: WindowId,
        /// 新しい矩形。
        rect: Rect,
        /// 新しい状態。
        state: WindowState,
    },
    /// フォーカスが変わった。
    SetFocus(Option<WindowId>),
    /// 窓に閉じる要求を送る（削除は `WindowUnmapped` で行う、D14）。
    RequestClose(WindowId),
    /// 出力の表示内容が変わった（D15）。
    Damage(OutputId),
}

/// `Core::apply` の成功結果。
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    /// 実行すべき Effect 列（仕様の順序）。
    pub effects: Vec<Effect>,
}
