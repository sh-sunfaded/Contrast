//! WindowFocusRequested: クリックなどによるフォーカス要求（仕様 v0.1 への追加）。
//!
//! 窓が表示中のワークスペースにあるときだけフォーカスを移す。それ以外は状態不変で `Ok`（I4）。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::domain::id::ExternalKey;
use crate::domain::state::State;
use crate::error::CoreError;

/// 窓へのフォーカスを要求する。
///
/// # Errors
/// key が未登録なら `UnknownKey`。
pub fn handle(state: &mut State, key: ExternalKey) -> Result<Vec<Effect>, CoreError> {
    let id = state.window_keys.id_of(key).ok_or(CoreError::UnknownKey)?;
    if state.is_window_visible(id) {
        state.set_focus(Some(id));
    }
    Ok(Vec::new())
}
