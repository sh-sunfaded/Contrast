//! ToggleFullscreen: フォーカス窓の全画面を切り替える（D9）。
//!
//! 全画面にするときは元の状態を覚え、同じワークスペースの他の全画面窓を元の状態に戻す
//! （1 ワークスペースに全画面は高々 1 枚）。解くときは覚えた状態に戻す。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::handlers::{demote_other_fullscreen, focused};
use crate::domain::state::State;
use crate::domain::window::WindowState;
use crate::error::CoreError;

/// 全画面を切り替える。
///
/// # Errors
/// フォーカスが None なら `NoFocusedWindow`。
pub fn handle(state: &mut State) -> Result<Vec<Effect>, CoreError> {
    let id = focused(state)?;
    let Some(w) = state.windows.get_mut(&id) else {
        return Ok(Vec::new());
    };
    if w.state == WindowState::Fullscreen {
        w.state = w.restore_state;
        return Ok(Vec::new());
    }
    w.restore_state = w.state;
    w.state = WindowState::Fullscreen;
    let index = w.workspace;
    demote_other_fullscreen(state, index, id);
    Ok(Vec::new())
}
