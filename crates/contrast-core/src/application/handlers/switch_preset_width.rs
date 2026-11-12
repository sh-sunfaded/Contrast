//! SwitchPresetColumnWidth: フォーカス列の幅を、現在の px 幅より広い最初のプリセットへ変える。なければ先頭のプリセットへ（D44）。
//!
//! 最大化の記憶は捨てる。`Equal` では状態不変で `Ok`（D45）。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::scrolling::{self, FocusPos};
use crate::domain::column::{ColumnWidth, LayoutMode, Ratio};
use crate::domain::state::State;
use crate::error::CoreError;

/// 次のプリセットの幅にする。
///
/// # Errors
/// `Scrolling` でフォーカスが None なら `NoFocusedWindow`。
pub fn handle(state: &mut State) -> Result<Vec<Effect>, CoreError> {
    if state.layout.mode != LayoutMode::Scrolling {
        return Ok(Vec::new());
    }
    let Some(FocusPos { ws, col, .. }) = scrolling::focus_pos(state)? else {
        return Ok(Vec::new());
    };
    let Some(area_width) = scrolling::shown_area_width(state, ws) else {
        return Ok(Vec::new());
    };
    let presets: Vec<Ratio> = state.layout.preset_iter().collect();
    let Some(current) = state
        .workspace(ws)
        .and_then(|w| w.columns.get(col))
        .map(|c| c.width.px(area_width))
    else {
        return Ok(Vec::new());
    };
    let next = presets
        .iter()
        .find(|r| ColumnWidth::Proportion(**r).px(area_width) > current)
        .or_else(|| presets.first())
        .copied();
    if let (Some(ratio), Some(c)) = (
        next,
        state.workspace_mut(ws).and_then(|w| w.columns.get_mut(col)),
    ) {
        c.width = ColumnWidth::Proportion(ratio);
        c.restore_width = None;
    }
    Ok(Vec::new())
}
