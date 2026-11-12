//! ViewGestureEnd: ジェスチャーを終え、列の端に吸着させ、その列にフォーカスを移す（D88・D89）。
//!
//! 守る不変条件: I4（フォーカスは表示中の窓）、I13（`Dynamic` ではアクティブな出力のときだけ core のフォーカスを移す）。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::scrolling;
use crate::domain::state::State;
use crate::domain::window::WindowState;
use crate::error::CoreError;

/// ジェスチャーを終える。
///
/// # Errors
/// なし（常に `Ok`）。
pub fn handle(state: &mut State, projected_offset: i32) -> Result<Vec<Effect>, CoreError> {
    let Some(at) = state.workspaces.iter().position(|ws| ws.gesture.is_some()) else {
        return Ok(Vec::new());
    };
    let Some((id, gesture, current)) = state
        .workspaces
        .get_mut(at)
        .and_then(|ws| Some((ws.id, ws.gesture.take()?, ws.view_x)))
    else {
        return Ok(Vec::new());
    };
    if gesture.dnd && gesture.offset == 0 {
        // 動かなかった DnD: ビューを開始時のまま保つ（`arrange` が D39 を当てる）。
        if let Some(ws) = state.workspaces.get_mut(at) {
            ws.view_x = gesture.start_view;
        }
        return Ok(Vec::new());
    }
    let Some(width) = scrolling::shown_area_width(state, id) else {
        return Ok(Vec::new());
    };
    let Some((widths, lefts, _)) = state
        .workspaces
        .get(at)
        .and_then(|ws| scrolling::column_layout(ws, width))
    else {
        return Ok(Vec::new());
    };
    let target = gesture.start_view.saturating_add(projected_offset);
    let Some((view, column)) = scrolling::snap_view(&widths, &lefts, width, current, target) else {
        return Ok(Vec::new());
    };
    let Some(window) = state.workspaces.get_mut(at).and_then(|ws| {
        ws.view_x = view;
        ws.columns.get(column).and_then(|c| c.focus_target())
    }) else {
        return Ok(Vec::new());
    };
    // D89: アクティブな出力に表示中で、フォーカスがこのワークスペースの Tiled の窓か None なら、core のフォーカスを移す。
    let on_active =
        state.focused_output().is_some() && state.focused_output() == state.output_showing(id);
    let focus_here = match state.focus.and_then(|f| state.windows.get(&f)) {
        None => true,
        Some(w) => w.workspace == id && w.state == WindowState::Tiled,
    };
    if on_active && focus_here {
        state.set_focus(Some(window));
    } else if let Some(ws) = state.workspaces.get_mut(at) {
        ws.record_focus(window);
        if let Some(col) = ws.columns.get_mut(column) {
            col.last_focused = Some(window);
        }
    }
    Ok(Vec::new())
}
