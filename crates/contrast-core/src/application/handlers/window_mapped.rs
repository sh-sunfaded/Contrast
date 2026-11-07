//! WindowMapped: 窓を登録し、Policy の判断に従って配置する（D6・D7・D9）。
//!
//! 守る不変条件: I1（所属は 1 つ）、I2（順序列の末尾に 1 回だけ足す）、I4（表示中のときだけフォーカス）。

use alloc::vec;
use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::handlers::{check_index, demote_other_fullscreen};
use crate::domain::id::{EntityId, ExternalKey, WorkspaceIndex};
use crate::domain::state::State;
use crate::domain::window::{Window, WindowMeta, WindowState};
use crate::error::CoreError;
use crate::policy::{PlacementContext, Policy};

/// 窓を追加する。
///
/// # Errors
/// key が登録済みなら `DuplicateKey`、配置先が範囲外なら `WorkspaceIndexOutOfRange`、
/// ID 枯渇なら `IdExhausted`。
pub fn handle<P: Policy>(
    state: &mut State,
    policy: &P,
    key: ExternalKey,
    meta: WindowMeta,
) -> Result<Vec<Effect>, CoreError> {
    if state.window_keys.id_of(key).is_some() {
        return Err(CoreError::DuplicateKey);
    }
    let focused_workspace = state
        .focused_output()
        .and_then(|o| state.outputs.get(&o))
        .map(|o| o.shown);
    let ctx = PlacementContext::new(
        focused_workspace,
        state.workspace_count(),
        state.outputs.len(),
    );
    let placement = policy.place(&meta, &ctx);
    let mut index = placement
        .workspace
        .or(focused_workspace)
        .unwrap_or(WorkspaceIndex::new(0));
    // `Dynamic` で、存在しないワークスペースを指定されたときは、指定がなかったことにする（niri の窓ルールと同じ）。
    if state.is_dynamic() && state.workspace(index).is_none() {
        index = focused_workspace.unwrap_or(WorkspaceIndex::new(0));
    }
    check_index(state, index)?;

    let id = state.window_ids.next()?;
    state.window_keys.insert(key, id)?;
    state
        .windows
        .insert(id, Window::new(id, meta, placement.state, index));
    if let Some(ws) = state.workspace_mut(index) {
        ws.push_window(id);
    }
    if placement.state == WindowState::Fullscreen {
        demote_other_fullscreen(state, index, id);
    }
    if placement.focus && state.output_showing(index).is_some() {
        state.set_focus(Some(id));
    }
    Ok(vec![Effect::Registered {
        key,
        id: EntityId::Window(id),
    }])
}
