//! MoveColumnToFirst / MoveColumnToLast: フォーカス列を最初・最後の位置へ移す（D42）。
//!
//! `Equal` では状態不変で `Ok`（D45）。すでにその位置にあるときも状態不変で `Ok`。フォーカスは列についていく。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::handlers::focus_column::End;
use crate::application::scrolling::{self, FocusPos};
use crate::domain::column::LayoutMode;
use crate::domain::state::State;
use crate::error::CoreError;

/// フォーカス列を端へ移す。
///
/// # Errors
/// `Scrolling` でフォーカスが None なら `NoFocusedWindow`。
pub fn handle(state: &mut State, end: End) -> Result<Vec<Effect>, CoreError> {
    if state.layout.mode != LayoutMode::Scrolling {
        return Ok(Vec::new());
    }
    let Some(FocusPos { ws, col, .. }) = scrolling::focus_pos(state)? else {
        return Ok(Vec::new());
    };
    if let Some(w) = state.workspace_mut(ws) {
        let target = match end {
            End::First => 0,
            End::Last => w.columns.len().saturating_sub(1),
        };
        w.move_column(col, target);
    }
    Ok(Vec::new())
}
