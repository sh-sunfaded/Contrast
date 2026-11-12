//! FocusDirection: フォーカス窓の矩形中心から見てその方向にある、最も近い表示中の窓へ移る（D10）。
//!
//! 距離は中心間のマンハッタン距離、同距離は WindowId 最小。候補なしは状態不変で `Ok`。
//! `Scrolling` で列にある窓からは、列の構造に従う（D42）。

use alloc::vec::Vec;

use crate::application::command::Direction;
use crate::application::effect::Effect;
use crate::application::handlers::focused;
use crate::application::scrolling::{self, FocusPos};
use crate::domain::column::LayoutMode;
use crate::domain::geometry::Point;
use crate::domain::state::State;
use crate::error::CoreError;

/// 方向にフォーカスを移す。
///
/// # Errors
/// フォーカスが None なら `NoFocusedWindow`。
pub fn handle(state: &mut State, direction: Direction) -> Result<Vec<Effect>, CoreError> {
    let id = focused(state)?;
    if state.layout.mode == LayoutMode::Scrolling
        && let Some(pos) = scrolling::focus_pos(state)?
    {
        return Ok(focus_in_columns(state, pos, direction));
    }
    let Some(from) = state
        .windows
        .get(&id)
        .and_then(|w| w.rect)
        .map(|r| r.center())
    else {
        return Ok(Vec::new());
    };
    let target = state
        .windows
        .values()
        .filter(|w| w.id != id && state.is_window_visible(w.id))
        .filter_map(|w| w.rect.map(|r| (w.id, r.center())))
        .filter(|(_, c)| is_toward(from, *c, direction))
        .min_by_key(|(wid, c)| (manhattan(from, *c), *wid))
        .map(|(wid, _)| wid);
    if target.is_some() {
        state.set_focus(target);
    }
    Ok(Vec::new())
}

/// D42: `Scrolling` では、左右は隣の列の「最後にフォーカスした窓」、上下は列の中の上下の窓へ移す。端では何もしない。
fn focus_in_columns(state: &mut State, pos: FocusPos, direction: Direction) -> Vec<Effect> {
    let target = state.workspace(pos.ws).and_then(|ws| match direction {
        Direction::Left => pos
            .col
            .checked_sub(1)
            .and_then(|c| ws.columns.get(c))
            .and_then(|c| c.focus_target()),
        Direction::Right => pos
            .col
            .checked_add(1)
            .and_then(|c| ws.columns.get(c))
            .and_then(|c| c.focus_target()),
        Direction::Up => pos
            .row
            .checked_sub(1)
            .and_then(|r| ws.columns.get(pos.col)?.windows.get(r))
            .copied(),
        Direction::Down => pos
            .row
            .checked_add(1)
            .and_then(|r| ws.columns.get(pos.col)?.windows.get(r))
            .copied(),
    });
    if target.is_some() {
        state.set_focus(target);
    }
    Vec::new()
}

/// `to` が `from` から見て `direction` の側にあるか。
const fn is_toward(from: Point, to: Point, direction: Direction) -> bool {
    match direction {
        Direction::Left => to.x < from.x,
        Direction::Right => to.x > from.x,
        Direction::Up => to.y < from.y,
        Direction::Down => to.y > from.y,
    }
}

/// 2 点のマンハッタン距離。
fn manhattan(a: Point, b: Point) -> u64 {
    u64::from(a.x.abs_diff(b.x)).saturating_add(u64::from(a.y.abs_diff(b.y)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_toward_is_strict_in_each_direction() {
        let from = Point::new(10, 10);
        let cases = [
            (
                Direction::Left,
                Point::new(9, 10),
                Point::new(10, 10),
                Point::new(11, 10),
            ),
            (
                Direction::Right,
                Point::new(11, 10),
                Point::new(10, 10),
                Point::new(9, 10),
            ),
            (
                Direction::Up,
                Point::new(10, 9),
                Point::new(10, 10),
                Point::new(10, 11),
            ),
            (
                Direction::Down,
                Point::new(10, 11),
                Point::new(10, 10),
                Point::new(10, 9),
            ),
        ];
        for (direction, toward, same, away) in cases {
            assert!(is_toward(from, toward, direction), "{direction:?}");
            assert!(!is_toward(from, same, direction), "{direction:?}");
            assert!(!is_toward(from, away, direction), "{direction:?}");
        }
    }

    #[test]
    fn other_axis_does_not_count() {
        let from = Point::new(10, 10);
        assert!(!is_toward(from, Point::new(0, 11), Direction::Right));
        assert!(!is_toward(from, Point::new(11, 0), Direction::Down));
    }
}
