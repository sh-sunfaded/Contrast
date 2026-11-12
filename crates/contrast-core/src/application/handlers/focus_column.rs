//! FocusColumnFirst / FocusColumnLast: 最初・最後の列の「最後にフォーカスした窓」へフォーカスを移す（D42）。
//!
//! `Equal` では状態不変で `Ok`（D45）。すでにその列にいるときも状態不変で `Ok`。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::scrolling::{self, FocusPos};
use crate::domain::column::LayoutMode;
use crate::domain::state::State;
use crate::error::CoreError;

/// どちらの端の列か。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    /// 最初の列。
    First,
    /// 最後の列。
    Last,
}

/// 端の列へフォーカスを移す。
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
    let Some(columns) = state.workspace(ws).map(|w| &w.columns) else {
        return Ok(Vec::new());
    };
    let target = match end {
        End::First => 0,
        End::Last => columns.len().saturating_sub(1),
    };
    if target == col {
        return Ok(Vec::new());
    }
    let next = columns.get(target).and_then(|c| c.focus_target());
    if next.is_some() {
        state.set_focus(next);
    }
    Ok(Vec::new())
}
