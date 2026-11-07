//! OutputFocusRequested: 窓の無い所のクリックなどで、出力をアクティブにする（D80）。
//!
//! `Dynamic` のみ。フォーカスはその出力の表示中のワークスペースの、D12 の規則のフォーカスにする（I13）。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::domain::id::ExternalKey;
use crate::domain::state::State;
use crate::error::CoreError;

/// 出力をアクティブにする。
///
/// # Errors
/// key が未登録なら `UnknownKey`。
pub fn handle(state: &mut State, key: ExternalKey) -> Result<Vec<Effect>, CoreError> {
    let id = state.output_keys.id_of(key).ok_or(CoreError::UnknownKey)?;
    if !state.is_dynamic() || state.focused_output() == Some(id) {
        return Ok(Vec::new());
    }
    state.active_output = Some(id);
    let next = state.preferred_focus_on(id);
    state.set_focus(next);
    Ok(Vec::new())
}
