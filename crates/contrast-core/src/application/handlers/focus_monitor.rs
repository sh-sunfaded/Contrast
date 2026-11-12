//! FocusMonitor: その方向の出力をアクティブにする（D78）。
//!
//! `Dynamic` のみ。フォーカスは移した出力の表示中のワークスペースの、D12 の規則のフォーカスにする（I13）。

use alloc::vec::Vec;

use crate::application::command::Direction;
use crate::application::effect::Effect;
use crate::application::handlers::neighbor_output;
use crate::domain::state::State;
use crate::error::CoreError;

/// アクティブな出力を、`direction` の出力へ移す。
///
/// # Errors
/// なし（常に `Ok`）。
pub fn handle(state: &mut State, direction: Direction) -> Result<Vec<Effect>, CoreError> {
    if !state.is_dynamic() {
        return Ok(Vec::new());
    }
    let Some(target) = neighbor_output(state, direction) else {
        return Ok(Vec::new());
    };
    state.active_output = Some(target);
    let next = state.preferred_focus_on(target);
    state.set_focus(next);
    Ok(Vec::new())
}
