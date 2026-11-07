//! MoveToWorkspace: フォーカス窓をワークスペース i の順序列末尾へ移す（D13）。
//!
//! フォーカスは窓に付いていかず、元のワークスペースの順序列先頭へ移る（D3 と同じ規則）。
//! 元のワークスペースが表示されていなければ None（I4）。同じワークスペースへの移動は状態不変で `Ok`。
//! 全画面のまま移した先に全画面窓があれば、先にいた方を元の状態に戻す（D9）。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::handlers::{demote_other_fullscreen, focused, resolve_index};
use crate::domain::id::WorkspaceIndex;
use crate::domain::state::State;
use crate::domain::window::WindowState;
use crate::error::CoreError;

/// フォーカス窓を別のワークスペースへ移す。
///
/// # Errors
/// i が範囲外なら `WorkspaceIndexOutOfRange`、フォーカスが None なら `NoFocusedWindow`。
pub fn handle(state: &mut State, index: WorkspaceIndex) -> Result<Vec<Effect>, CoreError> {
    let index = resolve_index(state, index)?;
    let id = focused(state)?;
    let Some((from, window_state)) = state.windows.get(&id).map(|w| (w.workspace, w.state)) else {
        return Ok(Vec::new());
    };
    if from == index {
        return Ok(Vec::new());
    }
    state.move_window_to(id, index);
    if window_state == WindowState::Fullscreen {
        demote_other_fullscreen(state, index, id);
    }
    let next = if state.output_showing(from).is_some() {
        state
            .workspace(from)
            .and_then(|ws| ws.windows.first().copied())
    } else {
        None
    };
    state.set_focus(next);
    // D65（`Dynamic`）。
    state.clean_up_workspaces();
    Ok(Vec::new())
}
