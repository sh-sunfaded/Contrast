//! ViewGestureUpdate: ジェスチャー中のビューを、開始時から `offset` だけ動かす（D87）。
//!
//! DnD のときだけ範囲を制限する。ジェスチャー中のワークスペースには D39 を当てない（I11 の例外、D91）。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::scrolling;
use crate::domain::state::State;
use crate::error::CoreError;

/// ジェスチャー中のビューを動かす。
///
/// # Errors
/// なし（常に `Ok`）。
pub fn handle(state: &mut State, offset: i32) -> Result<Vec<Effect>, CoreError> {
    let Some(at) = state.workspaces.iter().position(|ws| ws.gesture.is_some()) else {
        return Ok(Vec::new());
    };
    let Some(id) = state.workspaces.get(at).map(|ws| ws.id) else {
        return Ok(Vec::new());
    };
    let width = scrolling::shown_area_width(state, id);
    let Some(ws) = state.workspaces.get_mut(at) else {
        return Ok(Vec::new());
    };
    let Some(gesture) = ws.gesture.as_mut() else {
        return Ok(Vec::new());
    };
    gesture.offset = offset;
    let (start, dnd) = (gesture.start_view, gesture.dnd);
    let mut view = start.saturating_add(offset);
    if dnd && let Some(w) = width {
        let total = scrolling::column_layout(ws, w).map_or(0, |(_, _, total)| total);
        let (a, b) = (w.saturating_neg(), total);
        view = view.clamp(a.min(b), a.max(b));
    }
    ws.view_x = view;
    Ok(Vec::new())
}
