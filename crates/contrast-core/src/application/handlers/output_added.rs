//! OutputAdded: 出力を登録し、未表示のワークスペースのうち最小インデックスを表示する（D2）。

use alloc::vec;
use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::domain::geometry::Rect;
use crate::domain::id::{EntityId, ExternalKey, WorkspaceIndex};
use crate::domain::insets::Insets;
use crate::domain::output::{Output, OutputName};
use crate::domain::state::State;
use crate::error::CoreError;

/// 出力を追加する。
///
/// # Errors
/// key が登録済みなら `DuplicateKey`、未表示のワークスペースが無ければ `NoFreeWorkspace`、
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
    let mut free = (0..state.workspace_count())
        .map(WorkspaceIndex::new)
        .find(|i| state.output_showing(*i).is_none());
    // `Dynamic` で、空いているワークスペースが無ければ（出力が増えたとき）、新しい空を足して表示する。
    if free.is_none() && state.is_dynamic() {
        state.append_empty_workspace();
        free = (0..state.workspace_count())
            .map(WorkspaceIndex::new)
            .find(|i| state.output_showing(*i).is_none());
    }
    let shown = free.ok_or(CoreError::NoFreeWorkspace)?;
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
    Ok(vec![Effect::Registered {
        key,
        id: EntityId::Output(id),
    }])
}
