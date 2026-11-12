//! MaximizeColumn: フォーカス列の幅を `Proportion(1/1)` にする。最大化中なら、直前の幅に戻す（D44）。
//!
//! すでに 1/1 で、戻す幅の記憶がない列は状態不変で `Ok`。`Equal` でも状態不変で `Ok`（D45）。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::scrolling::{self, FocusPos};
use crate::domain::column::{ColumnWidth, LayoutMode, Ratio};
use crate::domain::state::State;
use crate::error::CoreError;

/// 最大化と元の幅への復帰を切り替える。
///
/// # Errors
/// `Scrolling` でフォーカスが None なら `NoFocusedWindow`。
pub fn handle(state: &mut State) -> Result<Vec<Effect>, CoreError> {
    if state.layout.mode != LayoutMode::Scrolling {
        return Ok(Vec::new());
    }
    let Some(FocusPos { ws, col, .. }) = scrolling::focus_pos(state)? else {
        return Ok(Vec::new());
    };
    let full = ColumnWidth::Proportion(Ratio::FULL);
    if let Some(c) = state.workspace_mut(ws).and_then(|w| w.columns.get_mut(col)) {
        if let Some(previous) = c.restore_width.take() {
            c.width = previous;
        } else if c.width != full {
            c.restore_width = Some(c.width);
            c.width = full;
        }
    }
    Ok(Vec::new())
}
