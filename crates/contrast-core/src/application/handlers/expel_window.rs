//! ExpelWindowFromColumn: フォーカス窓を、元の列の右隣の新しい列（既定の幅）へ出す（D43）。
//!
//! 列に窓が 1 枚だけなら状態不変で `Ok`。`Equal` でも状態不変で `Ok`（D45）。フォーカスは窓についていく。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::scrolling::{self, FocusPos};
use crate::domain::column::{Column, ColumnWidth, LayoutMode};
use crate::domain::state::State;
use crate::error::CoreError;

/// フォーカス窓を新しい列へ出す。
///
/// # Errors
/// `Scrolling` でフォーカスが None なら `NoFocusedWindow`。
pub fn handle(state: &mut State) -> Result<Vec<Effect>, CoreError> {
    if state.layout.mode != LayoutMode::Scrolling {
        return Ok(Vec::new());
    }
    let Some(FocusPos { ws, col, row, id }) = scrolling::focus_pos(state)? else {
        return Ok(Vec::new());
    };
    let width = ColumnWidth::Proportion(state.layout.default_width);
    let Some(w) = state.workspace_mut(ws) else {
        return Ok(Vec::new());
    };
    let stacked = w.columns.get(col).is_some_and(|c| c.windows.len() >= 2);
    if !stacked {
        return Ok(Vec::new());
    }
    if let Some(c) = w.columns.get_mut(col) {
        c.windows.remove(row.min(c.windows.len().saturating_sub(1)));
    }
    let at = col.saturating_add(1).min(w.columns.len());
    w.columns.insert(at, Column::single(id, width));
    Ok(Vec::new())
}
