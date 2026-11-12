//! MoveColumnToMonitor: フォーカス窓の列ごとを、その方向の出力の表示中のワークスペースへ移す（D79）。
//!
//! `Scrolling` かつ `Dynamic` のみ。Floating・Fullscreen は窓だけを移し、元の出力を整理する（niri の `move_to_output`）。
//! フォーカスは窓についていき、アクティブな出力は相手になる（D75・I13）。

use alloc::vec::Vec;

use crate::application::command::Direction;
use crate::application::effect::Effect;
use crate::application::handlers::{demote_other_fullscreen, focused, neighbor_output};
use crate::domain::column::{Column, LayoutMode};
use crate::domain::state::State;
use crate::domain::window::WindowState;
use crate::error::CoreError;

/// フォーカス窓の列を、`direction` の出力へ移す。
///
/// # Errors
/// 相手の出力があり、フォーカスが None なら `NoFocusedWindow`。
pub fn handle(state: &mut State, direction: Direction) -> Result<Vec<Effect>, CoreError> {
    if state.layout.mode != LayoutMode::Scrolling || !state.is_dynamic() {
        return Ok(Vec::new());
    }
    let Some(target) = neighbor_output(state, direction) else {
        return Ok(Vec::new());
    };
    let id = focused(state)?;
    let source = state.focused_output();
    let Some((from, window_state)) = state.windows.get(&id).map(|w| (w.workspace, w.state)) else {
        return Ok(Vec::new());
    };
    let Some(to) = state.outputs.get(&target).map(|o| o.shown) else {
        return Ok(Vec::new());
    };
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
    for window in &moving {
        state.move_window_to(*window, to);
    }
    if window_state == WindowState::Fullscreen {
        demote_other_fullscreen(state, to, id);
    }
    let whole_column = column.is_some();
    if let Some(column) = column
        && let Some(ws) = state.workspace_mut(to)
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
    state.set_focus(Some(id));
    if !whole_column {
        state.clean_up(source);
    }
    Ok(Vec::new())
}
