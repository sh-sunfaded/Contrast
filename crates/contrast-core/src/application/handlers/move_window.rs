//! MoveWindow: フォーカス中の Tiled 窓を、順序列で隣の Tiled 窓と入れ替える（D11）。
//!
//! Left / Up は 1 つ前、Right / Down は 1 つ後。端や Floating / Fullscreen は状態不変で `Ok`。
//! 入れ替えは順序列の 2 要素の交換なので I2（重複なし）を保つ。

use alloc::vec::Vec;

use crate::application::command::Direction;
use crate::application::effect::Effect;
use crate::application::handlers::focused;
use crate::application::scrolling::{self, FocusPos};
use crate::domain::column::LayoutMode;
use crate::domain::id::WindowId;
use crate::domain::state::State;
use crate::domain::window::WindowState;
use crate::error::CoreError;

/// フォーカス窓を隣と入れ替える。
///
/// # Errors
/// フォーカスが None なら `NoFocusedWindow`。
pub fn handle(state: &mut State, direction: Direction) -> Result<Vec<Effect>, CoreError> {
    let id = focused(state)?;
    let Some(window) = state.windows.get(&id) else {
        return Ok(Vec::new());
    };
    if window.state != WindowState::Tiled {
        return Ok(Vec::new());
    }
    let index = window.workspace;
    if state.layout.mode == LayoutMode::Scrolling
        && let Some(pos) = scrolling::focus_pos(state)?
    {
        move_in_columns(state, pos, direction);
        return Ok(Vec::new());
    }
    let Some(ws) = state.workspace(index) else {
        return Ok(Vec::new());
    };
    // 順序列のうち Tiled 窓の位置だけを並べる。
    let tiled: Vec<(usize, WindowId)> = ws
        .windows
        .iter()
        .enumerate()
        .filter(|(_, w)| {
            state
                .windows
                .get(w)
                .is_some_and(|w| w.state == WindowState::Tiled)
        })
        .map(|(p, w)| (p, *w))
        .collect();
    let Some(me) = tiled.iter().position(|(_, w)| *w == id) else {
        return Ok(Vec::new());
    };
    let other = match direction {
        Direction::Left | Direction::Up => me.checked_sub(1),
        Direction::Right | Direction::Down => me.checked_add(1),
    };
    let (Some((a, _)), Some((b, _))) = (tiled.get(me), other.and_then(|o| tiled.get(o))) else {
        return Ok(Vec::new());
    };
    let (a, b) = (*a, *b);
    if let Some(ws) = state.workspace_mut(index) {
        ws.windows.swap(a, b);
    }
    Ok(Vec::new())
}

/// D42: `Scrolling` では、左右はフォーカス列を隣の列と、上下は列の中の上下の窓と入れ替える。端では何もしない。
/// フォーカスは動かした窓・列についていく（フォーカス窓は変わらない）。
fn move_in_columns(state: &mut State, pos: FocusPos, direction: Direction) {
    let Some(ws) = state.workspace_mut(pos.ws) else {
        return;
    };
    match direction {
        Direction::Left | Direction::Right => {
            let other = if direction == Direction::Left {
                pos.col.checked_sub(1)
            } else {
                pos.col.checked_add(1)
            };
            if let Some(o) = other.filter(|o| *o < ws.columns.len()) {
                ws.columns.swap(pos.col, o);
            }
        }
        Direction::Up | Direction::Down => {
            let Some(column) = ws.columns.get_mut(pos.col) else {
                return;
            };
            let other = if direction == Direction::Up {
                pos.row.checked_sub(1)
            } else {
                pos.row.checked_add(1)
            };
            // `pos.row` は `focus_pos` が列の中にあると確かめている。
            if let Some(o) = other.filter(|o| *o < column.windows.len()) {
                column.windows.swap(pos.row, o);
            }
        }
    }
}
