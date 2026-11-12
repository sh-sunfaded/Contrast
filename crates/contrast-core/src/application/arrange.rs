//! 再配置: 表示中ワークスペースの全窓の矩形を計算し直す。
//!
//! 守る不変条件:
//! - I5 Tiled 窓は `Layout` に任せる（作業領域内・重なりなし）。`Scrolling` は列から決める（縦だけが作業領域内、D47）。
//! - D8 Floating 窓は要求サイズ（なければ作業領域の半分）を中央に置き、作業領域に収まるよう切り詰める。
//! - D9・D33 Fullscreen 窓は insets を無視した出力の論理矩形全体。
//!
//! 非表示のワークスペースの窓は最後の矩形を保つ。

use alloc::vec::Vec;

use crate::application::scrolling;
use crate::domain::column::LayoutMode;
use crate::domain::geometry::Rect;
use crate::domain::id::WindowId;
use crate::domain::state::State;
use crate::domain::window::{Window, WindowState};
use crate::error::CoreError;
use crate::layout::Layout;

/// 全出力の表示ワークスペースについて窓の矩形を計算し直す。
///
/// # Errors
/// 作業領域が作れない・Layout の失敗は `CoreError::Layout`、浮動矩形が i32 に収まらなければ `GeometryOverflow`。
pub fn arrange<L: Layout>(state: &mut State, layout: &L) -> Result<(), CoreError> {
    let mut shown = Vec::with_capacity(state.outputs.len());
    for o in state.outputs.values() {
        shown.push((o.work_area()?, o.geometry, o.shown));
    }
    for (area, geometry, index) in shown {
        let ids: Vec<WindowId> = state
            .workspace(index)
            .map(|ws| ws.windows.clone())
            .unwrap_or_default();
        let tiled: Vec<WindowId> = ids
            .iter()
            .copied()
            .filter(|id| {
                state
                    .windows
                    .get(id)
                    .is_some_and(|w| w.state == WindowState::Tiled)
            })
            .collect();
        if state.layout.mode == LayoutMode::Scrolling {
            // D38: Scrolling は `Layout` 型を使わず、列から矩形を決める（Floating・Fullscreen は下で既存どおり）。
            scrolling::arrange_workspace(state, index, area)?;
        } else {
            let mut placed = Vec::with_capacity(tiled.len());
            layout.arrange(area, &tiled, &mut placed)?;
            for (id, rect) in placed {
                if let Some(w) = state.windows.get_mut(&id) {
                    w.rect = Some(rect);
                }
            }
        }
        for id in ids {
            let Some(w) = state.windows.get_mut(&id) else {
                continue;
            };
            match w.state {
                WindowState::Tiled => {}
                WindowState::Floating => {
                    let rect = floating_rect(area, w)?;
                    w.floating_rect = Some(rect);
                    w.rect = Some(rect);
                }
                WindowState::Fullscreen => w.rect = Some(geometry),
            }
        }
    }
    Ok(())
}

/// D8 の浮動矩形。前回の浮動矩形が作業領域と交差していればそれを保つ。
fn floating_rect(area: Rect, window: &Window) -> Result<Rect, CoreError> {
    let kept = window
        .floating_rect
        .filter(|prev| prev.intersect(&area).is_some());
    if let Some(prev) = kept {
        return Ok(prev);
    }
    let (want_w, want_h) = match window.meta.requested_size {
        Some(size) => (size.width(), size.height()),
        None => (
            half_at_least_one(area.width()),
            half_at_least_one(area.height()),
        ),
    };
    let width = want_w.min(area.width());
    let height = want_h.min(area.height());
    let x = centered(area.x(), area.width(), width)?;
    let y = centered(area.y(), area.height(), height)?;
    Rect::new(x, y, width, height).ok_or(CoreError::GeometryOverflow)
}

/// `v / 2`（1 未満なら 1）。
fn half_at_least_one(v: i32) -> i32 {
    v.checked_div(2).unwrap_or(0).max(1)
}

/// 長さ `len` の区間を、`start` から長さ `outer` の区間の中央に置いたときの開始位置。
fn centered(start: i32, outer: i32, len: i32) -> Result<i32, CoreError> {
    let slack = outer.checked_sub(len).ok_or(CoreError::GeometryOverflow)?;
    let offset = slack.checked_div(2).ok_or(CoreError::GeometryOverflow)?;
    start.checked_add(offset).ok_or(CoreError::GeometryOverflow)
}
