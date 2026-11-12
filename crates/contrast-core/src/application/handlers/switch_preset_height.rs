//! SwitchPresetWindowHeight: フォーカス窓の高さを、現在の px の高さより高い最初のプリセットへ変える。なければ先頭のプリセットへ（D51）。
//!
//! 現在の高さは、配分（D50）の結果の px。作業領域がなければ、フォーカス窓が列になければ、状態不変で `Ok`。
//! `Equal` でも状態不変で `Ok`（D54）。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::scrolling::{self, FocusPos};
use crate::domain::column::{LayoutMode, Ratio, WindowHeight};
use crate::domain::state::State;
use crate::error::CoreError;

/// 次のプリセットの高さにする。
///
/// # Errors
/// `Scrolling` でフォーカスが None なら `NoFocusedWindow`。
pub fn handle(state: &mut State) -> Result<Vec<Effect>, CoreError> {
    if state.layout.mode != LayoutMode::Scrolling {
        return Ok(Vec::new());
    }
    let Some(FocusPos { ws, col, row, id }) = scrolling::focus_pos(state)? else {
        return Ok(Vec::new());
    };
    let Some(area_height) = scrolling::shown_area(state, ws).map(|a| a.height()) else {
        return Ok(Vec::new());
    };
    let current = state
        .workspace(ws)
        .and_then(|w| w.columns.get(col))
        .and_then(|c| scrolling::column_heights(c, area_height))
        .and_then(|heights| heights.get(row).copied());
    let Some(current) = current else {
        return Ok(Vec::new());
    };
    let presets: Vec<Ratio> = state.layout.window_preset_iter().collect();
    let next = presets
        .iter()
        .find(|r| WindowHeight::Proportion(**r).requested_px(area_height) > Some(current))
        .or_else(|| presets.first())
        .copied();
    if let (Some(ratio), Some(c)) = (
        next,
        state.workspace_mut(ws).and_then(|w| w.columns.get_mut(col)),
    ) {
        c.heights.insert(id, WindowHeight::Proportion(ratio));
    }
    Ok(Vec::new())
}
