//! MoveColumnToWorkspace: フォーカス窓の列ごと、別のワークスペースへ移し、そこへ切り替える（D67）。
//!
//! `Scrolling` のフォーカス窓（Tiled）なら列の全部の窓を、幅・窓ごとの高さ・最大化の状態を保ったまま、移し先の
//! 「最後にフォーカスした列」の右隣に入れる。Floating・Fullscreen は窓だけ移す。`Equal` では状態不変で `Ok`。
//! 移し先が今のワークスペースなら状態不変で `Ok`。`Dynamic` では、元のワークスペースが空になれば整理で消える（D65）。
//! `focus` が偽（D93・D94。niri の `focus=false`）なら、表示を切り替えず、フォーカスは元のワークスペースに残る
//! （列ごとなら右の列、無ければ左の列。窓だけなら順序列の先頭）。整理はしない。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::handlers::{
    demote_other_fullscreen, focused, resolve_index, show_workspace,
};
use crate::domain::column::{Column, LayoutMode};
use crate::domain::id::WorkspaceIndex;
use crate::domain::state::State;
use crate::domain::window::WindowState;
use crate::error::CoreError;

/// フォーカス窓の列ごとを、ワークスペース `index`（`Dynamic` では末尾に丸める）へ移す。`focus` なら、そこへ切り替える。
///
/// # Errors
/// `Fixed` で範囲外なら `WorkspaceIndexOutOfRange`。フォーカスが None なら `NoFocusedWindow`。
pub fn handle(
    state: &mut State,
    index: WorkspaceIndex,
    focus: bool,
) -> Result<Vec<Effect>, CoreError> {
    if state.layout.mode != LayoutMode::Scrolling {
        return Ok(Vec::new());
    }
    let target = resolve_index(state, index)?;
    let id = focused(state)?;
    let Some((from, window_state)) = state.windows.get(&id).map(|w| (w.workspace, w.state)) else {
        return Ok(Vec::new());
    };
    if from == target {
        return Ok(Vec::new());
    }
    // 動かす列（Tiled で列に属するとき）。窓の並びは列の並び。
    let column: Option<Column> = (window_state == WindowState::Tiled)
        .then(|| {
            state.workspace(from).and_then(|ws| {
                ws.column_of(id)
                    .and_then(|(c, _)| ws.columns.get(c).cloned())
            })
        })
        .flatten();
    let moving: Vec<_> = column
        .as_ref()
        .map_or_else(|| alloc::vec![id], |c| c.windows.clone());
    // D94: 元のワークスペースに残るフォーカス（移す前の列の並びで決める）。
    let stay = if focus {
        None
    } else {
        state.workspace(from).and_then(|ws| {
            let at = ws.column_of(id).map(|(c, _)| c);
            match (column.is_some(), at) {
                (true, Some(c)) => ws
                    .columns
                    .get(c.saturating_add(1))
                    .or_else(|| c.checked_sub(1).and_then(|l| ws.columns.get(l)))
                    .and_then(crate::domain::column::Column::focus_target),
                _ => ws.windows.iter().copied().find(|w| *w != id),
            }
        })
    };
    for window in &moving {
        state.move_window_to(*window, target);
    }
    if window_state == WindowState::Fullscreen {
        demote_other_fullscreen(state, target, id);
    }
    // 移し先の「最後にフォーカスした列」の右隣（なければ末尾）に、列をそのまま入れる。元の列は `sync` が片づける。
    if let Some(column) = column
        && let Some(ws) = state.workspace_mut(target)
    {
        let anchor = ws
            .focus_history
            .iter()
            .rev()
            .find_map(|h| ws.column_of(*h).map(|(c, _)| c));
        let at = anchor
            .map_or(ws.columns.len(), |c| c.saturating_add(1))
            .min(ws.columns.len());
        ws.columns.insert(at, column);
    }
    if !focus {
        state.set_focus(stay);
        return Ok(Vec::new());
    }
    if let Some(output) = state.focused_output() {
        show_workspace(state, output, target);
    }
    state.set_focus(Some(id));
    state.clean_up_active();
    Ok(Vec::new())
}
