//! CenterColumn: フォーカス列の中心を作業領域の中心に合わせる（D53）。
//!
//! `view_x` を `列の x + 列の幅 / 2 − 作業領域の幅 / 2` にし、D39 のクランプを適用する。結果が今の `view_x` と同じなら状態不変。
//! フォーカス窓が列になければ、作業領域がなければ、状態不変で `Ok`。`Equal` でも状態不変で `Ok`（D54）。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::scrolling::{self, FocusPos};
use crate::domain::column::LayoutMode;
use crate::domain::state::State;
use crate::error::CoreError;

/// フォーカス列をビューの中央に置く。
///
/// # Errors
/// `Scrolling` でフォーカスが None なら `NoFocusedWindow`。足し算が i32 に収まらなければ `GeometryOverflow`。
pub fn handle(state: &mut State) -> Result<Vec<Effect>, CoreError> {
    if state.layout.mode != LayoutMode::Scrolling {
        return Ok(Vec::new());
    }
    let Some(FocusPos { ws, col, .. }) = scrolling::focus_pos(state)? else {
        return Ok(Vec::new());
    };
    let Some(area_width) = scrolling::shown_area_width(state, ws) else {
        return Ok(Vec::new());
    };
    let Some(workspace) = state.workspace(ws) else {
        return Ok(Vec::new());
    };
    let (widths, lefts, total) =
        scrolling::column_layout(workspace, area_width).ok_or(CoreError::GeometryOverflow)?;
    let Some((left, width)) = lefts.get(col).zip(widths.get(col)) else {
        return Ok(Vec::new());
    };
    let view = scrolling::center_view(*left, *width, area_width, total);
    if let Some(w) = state.workspace_mut(ws) {
        w.view_x = view;
    }
    Ok(Vec::new())
}
