//! MoveWorkspaceDown / MoveWorkspaceUp: 表示中のワークスペースを 1 つ下／上と入れ替える（D68）。
//!
//! `Dynamic` のみ（`Fixed` では状態不変で `Ok`）。端では状態不変で `Ok`。表示は入れ替えたワークスペースについていく。
//! 入れ替えのあとの末尾の空の補充（D64。`Core::apply`）と整理（D65）は、niri の順序と同じ結果になる。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::domain::id::WorkspaceIndex;
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
    let Some(shown) = state
        .focused_output()
        .and_then(|o| state.outputs.get(&o))
        .map(|o| o.shown)
    else {
        return Ok(Vec::new());
    };
    let last = state.workspace_count().saturating_sub(1);
    let next = if down {
        shown.get().saturating_add(1).min(last)
    } else {
        shown.get().saturating_sub(1)
    };
    if next == shown.get() {
        return Ok(Vec::new());
    }
    state.swap_workspaces(shown, WorkspaceIndex::new(next));
    state.clean_up_workspaces();
    Ok(Vec::new())
}
