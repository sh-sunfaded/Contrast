//! ResetWindowHeight: フォーカス窓の高さを `Auto` に戻す（D51）。
//!
//! フォーカス窓が列になければ状態不変で `Ok`。`Equal` でも状態不変で `Ok`（D54）。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::scrolling::{self, FocusPos};
use crate::domain::column::LayoutMode;
use crate::domain::state::State;
use crate::error::CoreError;

/// フォーカス窓の高さを `Auto` に戻す。
///
/// # Errors
/// `Scrolling` でフォーカスが None なら `NoFocusedWindow`。
pub fn handle(state: &mut State) -> Result<Vec<Effect>, CoreError> {
    if state.layout.mode != LayoutMode::Scrolling {
        return Ok(Vec::new());
    }
    let Some(FocusPos { ws, col, id, .. }) = scrolling::focus_pos(state)? else {
        return Ok(Vec::new());
    };
    if let Some(c) = state.workspace_mut(ws).and_then(|w| w.columns.get_mut(col)) {
        c.heights.remove(&id);
    }
    Ok(Vec::new())
}
