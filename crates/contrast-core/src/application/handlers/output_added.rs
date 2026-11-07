//! OutputAdded: 出力を登録する。
//!
//! `Fixed`: 未表示のワークスペースのうち最小インデックスを表示する（D2）。フォーカスは与えない（D21）。
//! `Dynamic`: 出力ごとの列を作る（D76）。元の出力がこの出力のワークスペースを既存の出力から移し、末尾に空を足す。
//! 守る不変条件: I3（表示は自分の列のワークスペース）、I13（持ち主・アクティブな出力・フォーカス）。

use alloc::vec;
use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::handlers::fix_focus_on_active_output;
use crate::domain::geometry::Rect;
use crate::domain::id::{EntityId, ExternalKey, IdKind, OutputId, WorkspaceId};
use crate::domain::insets::Insets;
use crate::domain::output::{Output, OutputName};
use crate::domain::state::State;
use crate::error::CoreError;

/// 出力を追加する。
///
/// # Errors
/// key が登録済みなら `DuplicateKey`、未表示のワークスペースが無ければ `NoFreeWorkspace`（`Fixed`）、
/// ID 枯渇なら `IdExhausted`。
pub fn handle(
    state: &mut State,
    key: ExternalKey,
    name: OutputName,
    geometry: Rect,
) -> Result<Vec<Effect>, CoreError> {
    if state.output_keys.id_of(key).is_some() {
        return Err(CoreError::DuplicateKey);
    }
    if state.is_dynamic() {
        return add_dynamic(state, key, name, geometry);
    }
    let shown = state
        .workspaces
        .iter()
        .map(|ws| ws.id)
        .find(|id| state.output_showing(*id).is_none())
        .ok_or(CoreError::NoFreeWorkspace)?;
    let id = register(state, key, name, geometry, shown)?;
    Ok(registered(key, id))
}

/// ID を払い出し、出力を登録する。
///
/// # Errors
/// ID 枯渇なら `IdExhausted`、key の重複なら `DuplicateKey`。
fn register(
    state: &mut State,
    key: ExternalKey,
    name: OutputName,
    geometry: Rect,
    shown: WorkspaceId,
) -> Result<OutputId, CoreError> {
    let id = state.output_ids.next()?;
    state.output_keys.insert(key, id)?;
    state.outputs.insert(
        id,
        Output {
            id,
            name,
            geometry,
            shown,
            insets: Insets::default(),
        },
    );
    Ok(id)
}

/// `Registered` の Effect。
fn registered(key: ExternalKey, id: OutputId) -> Vec<Effect> {
    vec![Effect::Registered {
        key,
        id: EntityId::Output(id),
    }]
}

/// D76: `Dynamic` の出力の追加。
///
/// # Errors
/// ID 枯渇なら `IdExhausted`。
fn add_dynamic(
    state: &mut State,
    key: ExternalKey,
    name: OutputName,
    geometry: Rect,
) -> Result<Vec<Effect>, CoreError> {
    let was_empty = state.outputs.is_empty();
    // 表示は後で決める。仮の id（0 はどのワークスペースにも使わない）で登録する。
    let id = register(state, key, name.clone(), geometry, WorkspaceId::from_raw(0))?;
    let mine: Vec<WorkspaceId> = if was_empty {
        adopt_unowned(state)
    } else {
        take_from_others(state, id, &name)
    };
    for ws in &mine {
        if let Some(w) = state.workspace_mut(*ws) {
            w.output = Some(id);
        }
    }
    state.normalize_order();
    // 末尾の空（持ち主の無い列から引き継いだものが無ければ、新しく作る）。
    let ends_empty = mine
        .last()
        .and_then(|ws| state.workspace(*ws))
        .is_some_and(|ws| ws.windows.is_empty());
    if !ends_empty {
        state.append_empty_workspace(Some(id));
    }
    let list = state.list_of(Some(id));
    let remembered = state
        .last_shown
        .remove(&name)
        .filter(|ws| list.contains(ws));
    let shown = remembered.or_else(|| list.first().copied());
    if let (Some(shown), Some(o)) = (shown, state.outputs.get_mut(&id)) {
        o.shown = shown;
    }
    if was_empty {
        state.active_output = Some(id);
        state.primary_output = Some(id);
    }
    fix_focus_on_active_output(state);
    Ok(registered(key, id))
}

/// 出力が 0 台だったとき: 持ち主の無い列のうち、窓のあるものと末尾の空を残し（ほかの空は消す）、その id を返す。
fn adopt_unowned(state: &mut State) -> Vec<WorkspaceId> {
    let list = state.list_of(None);
    let last = list.last().copied();
    let keep: Vec<WorkspaceId> = list
        .into_iter()
        .filter(|ws| {
            Some(*ws) == last || state.workspace(*ws).is_some_and(|w| !w.windows.is_empty())
        })
        .collect();
    state
        .workspaces
        .retain(|ws| ws.output.is_some() || keep.contains(&ws.id));
    keep
}

/// 1 台以上あったとき: 既存の各出力の列から、元の出力が `name` のワークスペースを外す（表示中の位置の扱いは niri と同じ）。
/// 窓のあるものの id を、元の並びで返す（空のものは消す）。
fn take_from_others(state: &mut State, id: OutputId, name: &OutputName) -> Vec<WorkspaceId> {
    let mut moved: Vec<WorkspaceId> = Vec::new();
    let others: Vec<OutputId> = state.outputs.keys().copied().filter(|o| *o != id).collect();
    for other in others {
        let mut list = state.list_of(Some(other));
        let Some(shown) = state.outputs.get(&other).map(|o| o.shown) else {
            continue;
        };
        let mut shown_at = list.iter().position(|ws| *ws == shown).unwrap_or(0);
        let mut taken: Vec<WorkspaceId> = Vec::new();
        for i in (0..list.len()).rev() {
            let Some(ws) = list.get(i).copied() else {
                continue;
            };
            let preferred = state
                .workspace(ws)
                .is_some_and(|w| w.original_output.as_ref() == Some(name));
            if !preferred {
                continue;
            }
            list.remove(i);
            taken.push(ws);
            if i <= shown_at {
                shown_at = shown_at.saturating_sub(1);
            }
        }
        for ws in &taken {
            let has_windows = state.workspace(*ws).is_some_and(|w| !w.windows.is_empty());
            if has_windows {
                // 窓のあるものは新しい出力へ（持ち主は呼び出し側が直す）。
                if let Some(w) = state.workspace_mut(*ws) {
                    w.output = None;
                }
            } else {
                state.workspaces.retain(|w| w.id != *ws);
            }
        }
        // 末尾から外したので、逆順にすると元の並び。
        moved.extend(
            taken
                .iter()
                .rev()
                .filter(|ws| state.workspace(**ws).is_some()),
        );
        // 表示は、位置を直した先のワークスペース。列が空になったら新しい空を作る。
        let new_shown = match list.get(shown_at) {
            Some(ws) => Some(*ws),
            None => state.append_empty_workspace(Some(other)),
        };
        if let (Some(ws), Some(o)) = (new_shown, state.outputs.get_mut(&other)) {
            o.shown = ws;
        }
    }
    moved
}
