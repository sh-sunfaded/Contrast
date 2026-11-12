//! ConsumeWindowIntoColumn: 右隣の列の先頭の窓を、フォーカス列の最下段へ移す（D43）。
//!
//! 右隣が空になれば列を削除する。右隣がなければ状態不変で `Ok`。`Equal` でも状態不変で `Ok`（D45）。
//! フォーカスは動かない。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::scrolling::{self, FocusPos};
use crate::domain::column::LayoutMode;
use crate::domain::state::State;
use crate::error::CoreError;

/// 右隣の列の先頭の窓を取り込む。
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
    let Some(w) = state.workspace_mut(ws) else {
        return Ok(Vec::new());
    };
    let taken = col
        .checked_add(1)
        .and_then(|next| w.columns.get_mut(next))
        .and_then(|c| {
            if c.windows.is_empty() {
                None
            } else {
                Some(c.windows.remove(0))
            }
        });
    // 空になった右隣は、整合（`sync`）が削除する。
    if let Some(id) = taken
        && let Some(c) = w.columns.get_mut(col)
    {
        c.windows.push(id);
    }
    Ok(Vec::new())
}
