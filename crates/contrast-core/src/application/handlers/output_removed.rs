//! OutputRemoved: 出力を除く。
//!
//! `Fixed`: 表示していたワークスペースは未表示になり、窓は残る（D3）。フォーカスがそのワークスペースにあれば、
//! 残る出力のうち OutputId 最小の表示ワークスペースの順序列先頭へ移し、なければ None にする（I4）。
//! `Dynamic`: 窓のあるワークスペースを、元の出力か主の出力の列へ移す（D77）。I13 を保つ。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::handlers::fix_focus_on_active_output;
use crate::domain::id::{ExternalKey, OutputId, WorkspaceId};
use crate::domain::state::State;
use crate::error::CoreError;

/// 出力を削除する。
///
/// # Errors
/// key が未登録なら `UnknownKey`。
pub fn handle(state: &mut State, key: ExternalKey) -> Result<Vec<Effect>, CoreError> {
    let order: Vec<OutputId> = state.outputs.keys().copied().collect();
    let id = state.output_keys.remove_by_key(key)?;
    let removed = state.outputs.remove(&id);
    if state.is_dynamic() {
        if let Some(output) = removed {
            remove_dynamic(state, &order, id, output.name, output.shown);
        }
        return Ok(Vec::new());
    }
    let focus_lost = state.focus.is_some_and(|f| !state.is_window_visible(f));
    if focus_lost {
        let next = state
            .outputs
            .values()
            .next()
            .and_then(|o| state.workspace(o.shown))
            .and_then(|ws| ws.windows.first().copied());
        state.set_focus(next);
    }
    Ok(Vec::new())
}

/// D77: `Dynamic` の出力の削除。`order` は削除前の出力の並び（OutputId 昇順）。
fn remove_dynamic(
    state: &mut State,
    order: &[OutputId],
    id: OutputId,
    name: crate::domain::output::OutputName,
    shown: WorkspaceId,
) {
    state.last_shown.insert(name, shown);
    let list = state.list_of(Some(id));
    let (keep, empty): (Vec<WorkspaceId>, Vec<WorkspaceId>) = list
        .into_iter()
        .partition(|ws| state.workspace(*ws).is_some_and(|w| !w.windows.is_empty()));
    state.workspaces.retain(|ws| !empty.contains(&ws.id));
    if state.outputs.is_empty() {
        for ws in &keep {
            if let Some(w) = state.workspace_mut(*ws) {
                w.output = None;
            }
        }
        state.normalize_order();
        state.active_output = None;
        state.primary_output = None;
        state.set_focus(None);
        return;
    }
    // 主の出力とアクティブな出力を、niri の位置の規則で指し直す。
    let removed_at = order.iter().position(|o| *o == id).unwrap_or(0);
    let remaining: Vec<OutputId> = state.outputs.keys().copied().collect();
    let repoint = |current: Option<OutputId>| -> Option<OutputId> {
        let at = current.and_then(|c| order.iter().position(|o| *o == c))?;
        let at = if at >= removed_at {
            at.saturating_sub(1)
        } else {
            at
        };
        remaining
            .get(at)
            .copied()
            .or_else(|| remaining.first().copied())
    };
    state.primary_output = repoint(state.primary_output);
    state.active_output = repoint(state.active_output);
    let mut targets: Vec<OutputId> = Vec::new();
    for ws in keep {
        let original = state.workspace(ws).and_then(|w| w.original_output.clone());
        let target = original
            .and_then(|n| state.outputs.values().find(|o| o.name == n).map(|o| o.id))
            .or(state.primary_output);
        let Some(target) = target else { continue };
        insert_before_trailing(state, ws, target);
        if !targets.contains(&target) {
            targets.push(target);
        }
    }
    for target in targets {
        state.clean_up(Some(target));
    }
    fix_focus_on_active_output(state);
}

/// ワークスペース `ws` を、出力 `target` の列の末尾の空の直前へ入れる（niri の `append_workspaces`）。
fn insert_before_trailing(state: &mut State, ws: WorkspaceId, target: OutputId) {
    let Some(at) = state.workspaces.iter().position(|w| w.id == ws) else {
        return;
    };
    let mut moving = state.workspaces.remove(at);
    moving.output = Some(target);
    let trailing = state
        .workspaces
        .iter()
        .rposition(|w| w.output == Some(target))
        .unwrap_or(state.workspaces.len());
    state.workspaces.insert(trailing, moving);
    state.normalize_order();
}
