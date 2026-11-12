//! ViewGestureBegin: 出力の表示中のワークスペースで、ビューの連続スクロールを始める（D85・D86）。
//!
//! 守る不変条件: I14（ジェスチャーは高々 1 つで、表示中の、列のあるワークスペースにある）。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::domain::column::LayoutMode;
use crate::domain::id::ExternalKey;
use crate::domain::state::State;
use crate::domain::workspace::ViewGesture;
use crate::error::CoreError;

/// ジェスチャーを始める。
///
/// # Errors
/// `output` が未登録なら `UnknownKey`。
pub fn handle(state: &mut State, output: ExternalKey, dnd: bool) -> Result<Vec<Effect>, CoreError> {
    let id = state
        .output_keys
        .id_of(output)
        .ok_or(CoreError::UnknownKey)?;
    if state.layout.mode != LayoutMode::Scrolling {
        return Ok(Vec::new());
    }
    let Some(shown) = state.outputs.get(&id).map(|o| o.shown) else {
        return Ok(Vec::new());
    };
    let has_columns = state
        .workspace(shown)
        .is_some_and(|ws| !ws.columns.is_empty());
    if !has_columns {
        return Ok(Vec::new());
    }
    // ほかのジェスチャーは終える（ビューは `arrange` が D39 で直す）。
    for ws in &mut state.workspaces {
        ws.gesture = None;
    }
    if let Some(ws) = state.workspace_mut(shown) {
        ws.gesture = Some(ViewGesture {
            start_view: ws.view_x,
            offset: 0,
            dnd,
        });
    }
    Ok(Vec::new())
}
