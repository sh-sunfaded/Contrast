//! ToggleFloating: フォーカス窓の Tiled と Floating を切り替える。
//!
//! Fullscreen の窓は状態不変で `Ok`（docs/questions.md Q11）。矩形は `arrange` が D8 で決める。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::handlers::focused;
use crate::domain::state::State;
use crate::domain::window::WindowState;
use crate::error::CoreError;

/// Tiled / Floating を切り替える。
///
/// # Errors
/// フォーカスが None なら `NoFocusedWindow`。
pub fn handle(state: &mut State) -> Result<Vec<Effect>, CoreError> {
    let id = focused(state)?;
    if let Some(w) = state.windows.get_mut(&id) {
        w.state = match w.state {
            WindowState::Tiled => WindowState::Floating,
            WindowState::Floating => WindowState::Tiled,
            WindowState::Fullscreen => WindowState::Fullscreen,
        };
    }
    Ok(Vec::new())
}
