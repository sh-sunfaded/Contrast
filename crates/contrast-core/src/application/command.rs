//! core への入力: `Command`、`Action`、`Direction`。

use crate::domain::column::{ColumnWidth, WindowHeight};
use crate::domain::geometry::Rect;
use crate::domain::id::{ExternalKey, WorkspaceIndex};
use crate::domain::insets::Insets;
use crate::domain::output::OutputName;
use crate::domain::window::WindowMeta;

/// シェルが core に渡す入力。
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// 出力が増えた。
    OutputAdded {
        /// シェル側の識別子。
        key: ExternalKey,
        /// 名前。
        name: OutputName,
        /// 論理矩形。
        geometry: Rect,
    },
    /// 出力が消えた。
    OutputRemoved(ExternalKey),
    /// 出力の論理矩形が変わった。
    OutputChanged {
        /// シェル側の識別子。
        key: ExternalKey,
        /// 新しい論理矩形。
        geometry: Rect,
    },
    /// 出力の作業領域の余白が変わった（D31）。
    OutputInsetsChanged {
        /// シェル側の識別子。
        key: ExternalKey,
        /// 新しい余白。
        insets: Insets,
    },
    /// トップレベル窓が現れた。
    WindowMapped {
        /// シェル側の識別子。
        key: ExternalKey,
        /// 窓の性質。
        meta: WindowMeta,
    },
    /// トップレベル窓が消えた。
    WindowUnmapped(ExternalKey),
    /// 窓の性質が変わった（再配置はしない）。
    WindowMetaChanged {
        /// シェル側の識別子。
        key: ExternalKey,
        /// 新しい性質。
        meta: WindowMeta,
    },
    /// 窓へのフォーカスの要求（クリックなど）。仕様 v0.1 への追加（5 節）。
    ///
    /// 窓が表示中のワークスペースにあるときだけフォーカスを移す。それ以外は状態不変で `Ok`。
    WindowFocusRequested(ExternalKey),
    /// キーバインド解決後の意味操作。
    Action(Action),
}

/// 意味操作。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// その方向の最も近い表示中の窓へフォーカスを移す（D10）。
    FocusDirection(Direction),
    /// フォーカス中の Tiled 窓を順序列で隣と入れ替える（D11）。
    MoveWindow(Direction),
    /// フォーカス出力の表示ワークスペースを切り替える（D12）。
    SwitchWorkspace(WorkspaceIndex),
    /// フォーカス窓を別のワークスペースへ移す（D13）。
    MoveToWorkspace(WorkspaceIndex),
    /// フォーカス窓の Tiled / Floating を切り替える。
    ToggleFloating,
    /// フォーカス窓の全画面を切り替える（D9）。
    ToggleFullscreen,
    /// フォーカス窓に閉じる要求を出す（D14）。
    CloseFocused,
    /// 最初の列へフォーカスを移す（`Scrolling` のみ、D42）。
    FocusColumnFirst,
    /// 最後の列へフォーカスを移す（`Scrolling` のみ、D42）。
    FocusColumnLast,
    /// フォーカス列を最初の位置へ移す（`Scrolling` のみ、D42）。
    MoveColumnToFirst,
    /// フォーカス列を最後の位置へ移す（`Scrolling` のみ、D42）。
    MoveColumnToLast,
    /// 右隣の列の先頭の窓を、フォーカス列の最下段へ移す（`Scrolling` のみ、D43）。
    ConsumeWindowIntoColumn,
    /// フォーカス窓を、右隣の新しい列へ出す（`Scrolling` のみ、D43）。
    ExpelWindowFromColumn,
    /// フォーカス列の幅を、次のプリセットへ変える（`Scrolling` のみ、D44）。
    SwitchPresetColumnWidth,
    /// フォーカス列を最大化する。最大化中なら元の幅に戻す（`Scrolling` のみ、D44）。
    MaximizeColumn,
    /// フォーカス列の幅を指定する（`Scrolling` のみ、D44）。
    SetColumnWidth(ColumnWidth),
    /// フォーカス窓の高さを指定する。`Auto` は `ResetWindowHeight` と同じ（`Scrolling` のみ、D51）。
    SetWindowHeight(WindowHeight),
    /// フォーカス窓の高さを、現在の px の高さより高い最初のプリセットへ変える。なければ先頭へ（`Scrolling` のみ、D51）。
    SwitchPresetWindowHeight,
    /// フォーカス窓の高さを `Auto` に戻す（`Scrolling` のみ、D51）。
    ResetWindowHeight,
    /// フォーカス列の中心を作業領域の中心に合わせる（`Scrolling` のみ、D51・D53）。
    CenterColumn,
    /// 列に窓が 1 枚なら左隣の列の最下段へ、2 枚以上なら左隣に新しい列として出す（`Scrolling` のみ、D57・D58）。
    ConsumeOrExpelWindowLeft,
    /// 列に窓が 1 枚なら右隣の列の最下段へ、2 枚以上なら右隣に新しい列として出す（`Scrolling` のみ、D57・D59）。
    ConsumeOrExpelWindowRight,
}

/// 方向。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// 左。
    Left,
    /// 右。
    Right,
    /// 上。
    Up,
    /// 下。
    Down,
}
