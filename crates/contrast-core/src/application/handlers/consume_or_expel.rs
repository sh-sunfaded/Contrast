//! ConsumeOrExpelWindowLeft / ConsumeOrExpelWindowRight: 列に窓が 1 枚なら隣の列の最下段へ、2 枚以上なら隣に新しい列へ出す（D58・D59）。
//!
//! `Scrolling` のフォーカス窓（Tiled）だけが対象。端の列の 1 枚の窓、`Equal`、Floating・Fullscreen のフォーカスは、状態不変で `Ok`
//! （D60）。フォーカスは窓についていく（フォーカス窓は変わらない）。窓の高さは、別の列へ移るので `sync` が捨てて `Auto` に戻る（D52）。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::scrolling::{self, FocusPos};
use crate::domain::column::{Column, LayoutMode};
use crate::domain::state::State;
use crate::error::CoreError;

/// フォーカス窓を、左（`to_left`）か右の隣へ、取り込ませるか出す。
///
/// # Errors
/// `Scrolling` でフォーカスが None なら `NoFocusedWindow`。
pub fn handle(state: &mut State, to_left: bool) -> Result<Vec<Effect>, CoreError> {
    if state.layout.mode != LayoutMode::Scrolling {
        return Ok(Vec::new());
    }
    let Some(FocusPos { ws, col, row, id }) = scrolling::focus_pos(state)? else {
        return Ok(Vec::new());
    };
    let Some(w) = state.workspace_mut(ws) else {
        return Ok(Vec::new());
    };
    let Some(source) = w.columns.get(col) else {
        return Ok(Vec::new());
    };
    if source.windows.len() <= 1 {
        // 隣の列の最下段へ。隣がなければ（端の列）何もしない。
        let target = if to_left {
            col.checked_sub(1)
        } else {
            col.checked_add(1).filter(|t| *t < w.columns.len())
        };
        if target.is_none() {
            return Ok(Vec::new());
        }
        w.columns.remove(col);
        // 右へのとき、元の列を消すと、右隣の列の位置が 1 つ前（`col`）に詰まる。左のときは `col - 1` のまま。
        let at = if to_left { col.saturating_sub(1) } else { col };
        if let Some(c) = w.columns.get_mut(at) {
            c.windows.push(id);
        }
    } else {
        // 元の列の幅（と最大化の状態）を保って、隣に新しい列として出す。
        let (width, restore_width) = (source.width, source.restore_width);
        if let Some(c) = w.columns.get_mut(col) {
            c.windows.remove(row.min(c.windows.len().saturating_sub(1)));
        }
        let at = if to_left { col } else { col.saturating_add(1) }.min(w.columns.len());
        let mut column = Column::single(id, width);
        column.restore_width = restore_width;
        w.columns.insert(at, column);
    }
    Ok(Vec::new())
}
