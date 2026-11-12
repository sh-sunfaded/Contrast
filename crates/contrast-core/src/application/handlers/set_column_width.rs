//! SetColumnWidth: フォーカス列の幅を指定の幅にする（D44）。
//!
//! 最大化の記憶は捨てる（明示した幅が最新になる）。1 未満の `Fixed` は 1 に直す（I10）。
//! `Equal` では状態不変で `Ok`（D45）。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::scrolling::{self, FocusPos};
use crate::domain::column::{ColumnWidth, LayoutMode};
use crate::domain::state::State;
use crate::error::CoreError;

/// フォーカス列の幅を `width` にする。
///
/// # Errors
/// `Scrolling` でフォーカスが None なら `NoFocusedWindow`。
pub fn handle(state: &mut State, width: ColumnWidth) -> Result<Vec<Effect>, CoreError> {
    if state.layout.mode != LayoutMode::Scrolling {
        return Ok(Vec::new());
    }
    let Some(FocusPos { ws, col, .. }) = scrolling::focus_pos(state)? else {
        return Ok(Vec::new());
    };
    if let Some(c) = state.workspace_mut(ws).and_then(|w| w.columns.get_mut(col)) {
        c.width = width.sanitized();
        c.restore_width = None;
    }
    Ok(Vec::new())
}
