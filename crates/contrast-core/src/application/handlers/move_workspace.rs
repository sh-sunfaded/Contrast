//! MoveWorkspaceDown / MoveWorkspaceUp: 表示中のワークスペースを 1 つ下／上と入れ替える（D68）。
//!
//! `Dynamic` のみ（`Fixed` では状態不変で `Ok`）。端では状態不変で `Ok`。表示は入れ替えたワークスペースについていく。
//! 入れ替えのあとの末尾の空の補充（D64。`Core::apply`）と整理（D65）は、niri の順序と同じ結果になる。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::domain::state::State;
use crate::error::CoreError;

/// フォーカス出力が表示するワークスペースを、`down` なら 1 つ下、そうでなければ 1 つ上と入れ替える。
///
/// # Errors
/// なし（常に `Ok`）。
pub fn handle(state: &mut State, down: bool) -> Result<Vec<Effect>, CoreError> {
    if !state.is_dynamic() {
        return Ok(Vec::new());
    }
    let Some(output) = state.focused_output() else {
        return Ok(Vec::new());
    };
    let Some(shown) = state.outputs.get(&output).map(|o| o.shown) else {
        return Ok(Vec::new());
    };
    // D81: アクティブな出力の列の中で入れ替える。
    let list = state.list_of(Some(output));
    let Some(at) = list.iter().position(|ws| *ws == shown) else {
        return Ok(Vec::new());
    };
    let last = list.len().saturating_sub(1);
    let next = if down {
        at.saturating_add(1).min(last)
    } else {
        at.saturating_sub(1)
    };
    let Some(other) = list.get(next).copied().filter(|_| next != at) else {
        return Ok(Vec::new());
    };
    state.swap_workspaces(shown, other);
    state.clean_up(Some(output));
    Ok(Vec::new())
}
