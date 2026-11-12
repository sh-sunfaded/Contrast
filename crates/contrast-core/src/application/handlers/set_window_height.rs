//! SetWindowHeight: フォーカス窓の高さを指定の高さにする（D51）。`Auto` は `ResetWindowHeight` と同じ。
//!
//! 1 未満の `Fixed` は 1 に直す。フォーカス窓が列になければ（Floating・Fullscreen）状態不変で `Ok`。
//! `Equal` では状態不変で `Ok`（D54）。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::scrolling::{self, FocusPos};
use crate::domain::column::{LayoutMode, WindowHeight};
use crate::domain::state::State;
use crate::error::CoreError;

/// フォーカス窓の高さを `height` にする。
///
/// # Errors
/// `Scrolling` でフォーカスが None なら `NoFocusedWindow`。
pub fn handle(state: &mut State, height: WindowHeight) -> Result<Vec<Effect>, CoreError> {
    if state.layout.mode != LayoutMode::Scrolling {
        return Ok(Vec::new());
    }
    let Some(FocusPos { ws, col, id, .. }) = scrolling::focus_pos(state)? else {
        return Ok(Vec::new());
    };
    if let Some(c) = state.workspace_mut(ws).and_then(|w| w.columns.get_mut(col)) {
        match height.sanitized() {
            WindowHeight::Auto => {
                c.heights.remove(&id);
            }
            other => {
                c.heights.insert(id, other);
            }
        }
    }
    Ok(Vec::new())
}
