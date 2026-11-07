//! スクロール列レイアウト（core-spec 12 節）の計算と、列の状態の整合。
//!
//! 純粋な関数（`split_heights`・`distribute_heights`・`column_lefts`・`adjust_view`・`clamp_view`）と、`State` / `Workspace` を読み書きする
//! 関数（`sync`・`arrange_workspace`・`focus_pos`・`focus_after_removal`）を置く。
//!
//! 守る不変条件:
//! - I5（Scrolling 版）`arrange_workspace` は列を x = 0 から隙間なく並べ、窓の高さを D50 の規則で配る。縦は作業領域に収まり、重ならない。
//! - I10 `sync` は、Tiled の窓をちょうど 1 つの列に入れ、空の列を作らない。
//! - I11 `arrange_workspace` は、フォーカス列が見えるようにビューを調整する（D39）。
//!
//! 算術はすべて検査つき。オーバーフローは `CoreError::GeometryOverflow` にする。

use alloc::vec::Vec;

use crate::domain::column::{Column, ColumnWidth, WindowHeight};
use crate::domain::geometry::Rect;
use crate::domain::id::{WindowId, WorkspaceId};
use crate::domain::state::State;
use crate::domain::window::WindowState;
use crate::domain::workspace::Workspace;
use crate::error::{CoreError, LayoutError};

/// 高さ `total` を `n` 個に等分した高さ。余りは上から 1 px ずつ（D38）。`n == 0` か `total < n` なら `None`。
#[must_use]
pub fn split_heights(total: i32, n: usize) -> Option<Vec<i32>> {
    let count = i32::try_from(n).ok().filter(|c| *c >= 1 && *c <= total)?;
    let base = total.checked_div(count)?;
    let extra = total.checked_rem(count)?;
    let mut heights = Vec::with_capacity(n);
    for i in 0..count {
        heights.push(if i < extra {
            base.checked_add(1)?
        } else {
            base
        });
    }
    Some(heights)
}

/// D50: 高さ `total` を、上から並んだ窓の要求 `requests` に配る。
///
/// - `Auto` が 1 つもなければ、一番下の窓を `Auto` とみなす（その窓自身の要求は使わない）。
/// - `Auto` でない窓は、上から順に、要求の高さ（`Proportion` は `total × r` の切り捨て、1 以上。`Fixed` は px）を取る。
///   ただし上限は、残りの高さ（これまでに配った `Auto` でない窓の分を引いた高さ）から、
///   まだ配っていない他の窓の数 × 1 px を引いた値。上限を超える要求は上限に切り詰める。
/// - `Auto` の窓は、残りを等分する（余りは上から 1 px ずつ）。
///
/// どの窓も 1 以上で、合計は `total` になる。窓がなければ、または窓の数が `total` より多ければ `None`。
/// 足し引きが i32 に収まらなくても `None`。
///
/// 上限の数え方は、D50 の「後ろに続く窓の数」を、`Auto` が `Auto` でない窓より上にあるときも
/// 1 px を残せるように、「まだ配っていない他の窓の数」に広げたもの（上にある `Auto` の窓が 0 になるのを防ぐ）。
/// `Auto` が `Auto` でない窓より下にしかないときは、D50 の文面と同じ結果になる。
#[must_use]
pub fn distribute_heights(total: i32, requests: &[WindowHeight]) -> Option<Vec<i32>> {
    let count = i32::try_from(requests.len())
        .ok()
        .filter(|c| *c >= 1 && *c <= total)?;
    let has_auto = requests.contains(&WindowHeight::Auto);
    let last = requests.len().checked_sub(1)?;
    let mut fixed: Vec<Option<i32>> = Vec::with_capacity(requests.len());
    let mut fixed_sum: i32 = 0;
    let mut assigned: i32 = 0;
    for (i, request) in requests.iter().enumerate() {
        let auto = *request == WindowHeight::Auto || (!has_auto && i == last);
        if auto {
            fixed.push(None);
            continue;
        }
        let want = request.requested_px(total).unwrap_or(1);
        let unassigned_others = count.checked_sub(assigned)?.checked_sub(1)?;
        let bound = total
            .checked_sub(fixed_sum)?
            .checked_sub(unassigned_others)?;
        let height = want.clamp(1, bound.max(1));
        fixed_sum = fixed_sum.checked_add(height)?;
        assigned = assigned.checked_add(1)?;
        fixed.push(Some(height));
    }
    let autos = usize::try_from(count.checked_sub(assigned)?).ok()?;
    let mut shares = split_heights(total.checked_sub(fixed_sum)?, autos)?.into_iter();
    fixed
        .into_iter()
        .map(|height| height.or_else(|| shares.next()))
        .collect()
}

/// 列 `column` の窓の高さを、上から順に、作業領域の高さ `total` に対して求める（D50）。`distribute_heights` が `None` なら `None`。
#[must_use]
pub fn column_heights(column: &Column, total: i32) -> Option<Vec<i32>> {
    distribute_heights(total, &column.height_requests())
}

/// 幅の並び（左から右）から、各列の左端（x = 0 から隙間なし）と総幅を求める。足し算が i32 に収まらなければ `None`。
#[must_use]
pub fn column_lefts(widths: &[i32]) -> Option<(Vec<i32>, i32)> {
    let mut lefts = Vec::with_capacity(widths.len());
    let mut x: i32 = 0;
    for w in widths {
        lefts.push(x);
        x = x.checked_add(*w)?;
    }
    Some((lefts, x))
}

/// ビューの左端を、`0` 以上 `max(0, 総幅 − 作業領域の幅)` 以下に収める（D39 の最後）。
#[must_use]
pub fn clamp_view(view_x: i32, total: i32, area_width: i32) -> i32 {
    let max = total.saturating_sub(area_width).max(0);
    view_x.clamp(0, max)
}

/// フォーカス列（左端 `left`、幅 `width`）が見えるようにしたビューの左端（D39）。
///
/// 列が作業領域以上に広ければ左端を合わせる。そうでなければ、はみ出している側の端へ最小限だけ動かす。
/// 最後に `clamp_view` で収める。
#[must_use]
pub fn adjust_view(view_x: i32, left: i32, width: i32, area_width: i32, total: i32) -> i32 {
    // 右端が見えるまで動かし（`right − 作業領域の幅` 以上）、それでも左端を越えないようにする（`left` 以下）。
    // 列が作業領域以上に広ければ `right − 作業領域の幅 >= left` なので、結果は自然に左端になる。
    let right = left.saturating_add(width);
    let moved = view_x.max(right.saturating_sub(area_width)).min(left);
    clamp_view(moved, total, area_width)
}

/// 作業領域の幅 `area_width` のときの、列ごとの px 幅。
#[must_use]
pub fn column_widths(columns: &[Column], area_width: i32) -> Vec<i32> {
    columns.iter().map(|c| c.width.px(area_width)).collect()
}

/// ワークスペース `ws` の列の配置: 各列の幅・左端・総幅。足し算が i32 に収まらなければ `None`。
#[must_use]
pub fn column_layout(ws: &Workspace, area_width: i32) -> Option<(Vec<i32>, Vec<i32>, i32)> {
    let widths = column_widths(&ws.columns, area_width);
    let (lefts, total) = column_lefts(&widths)?;
    Some((widths, lefts, total))
}

/// ワークスペース `index` を表示している出力の作業領域の幅。出力がなければ `None`。
#[must_use]
pub fn shown_area_width(state: &State, index: WorkspaceId) -> Option<i32> {
    let output = state
        .output_showing(index)
        .and_then(|o| state.outputs.get(&o))?;
    output.work_area().ok().map(|a| a.width())
}

/// ワークスペース `index` を表示している出力の作業領域。出力がなければ `None`。
#[must_use]
pub fn shown_area(state: &State, index: WorkspaceId) -> Option<Rect> {
    let output = state
        .output_showing(index)
        .and_then(|o| state.outputs.get(&o))?;
    output.work_area().ok()
}

/// D53: 列（左端 `left`、幅 `width`）の中心を作業領域の中心に合わせるビューの左端。
/// 列の x + 列の幅 / 2 − 作業領域の幅 / 2 を、D39 のクランプ（0 以上、`max(0, 総幅 − 作業領域の幅)` 以下）に収める。
#[must_use]
pub fn center_view(left: i32, width: i32, area_width: i32, total: i32) -> i32 {
    let center = left.saturating_add(width.checked_div(2).unwrap_or(0));
    let half = area_width.checked_div(2).unwrap_or(0);
    clamp_view(center.saturating_sub(half), total, area_width)
}

/// 列の状態を、窓の状態に合わせる（D36・D40・D46・D52、I10）。`Scrolling` のとき、各 handler のあと・`arrange` の前に呼ぶ。
///
/// - Tiled でなくなった窓・消えた窓を列から除き、空の列を除く。
/// - どの列にもない Tiled の窓は、最後にフォーカスされた列の右隣に、新しい列（既定の幅）として入れる。
///   列がなければ先頭、フォーカス履歴に列のある窓がなければ末尾。
/// - フォーカス窓がある列の「最後にフォーカスした窓」をその窓にする。
pub fn sync(state: &mut State) {
    let default_width = ColumnWidth::Proportion(state.layout.default_width);
    let focus = state.focus;
    let tiled: Vec<(WorkspaceId, Vec<WindowId>)> = state
        .workspaces
        .iter()
        .map(|ws| {
            let ids = ws
                .windows
                .iter()
                .copied()
                .filter(|id| {
                    state
                        .windows
                        .get(id)
                        .is_some_and(|w| w.state == WindowState::Tiled)
                })
                .collect();
            (ws.id, ids)
        })
        .collect();
    for (index, ids) in tiled {
        let focus_here = focus.filter(|f| ids.contains(f));
        if let Some(ws) = state.workspace_mut(index) {
            sync_workspace(ws, &ids, focus_here, default_width);
        }
    }
}

/// 1 つのワークスペースの列を `tiled`（順序列にある Tiled の窓）に合わせる。`sync` の本体。
pub fn sync_workspace(
    ws: &mut Workspace,
    tiled: &[WindowId],
    focus: Option<WindowId>,
    default_width: ColumnWidth,
) {
    for col in &mut ws.columns {
        col.windows.retain(|w| tiled.contains(w));
        if col.last_focused.is_some_and(|l| !col.windows.contains(&l)) {
            col.last_focused = None;
        }
        // D52: 列にいない窓の高さは捨てる。窓が別の列へ移る（Consume・Expel・別のワークスペースへ・
        // Floating や Fullscreen を経て戻る）と、その窓の高さは `Auto` に戻る。同じ列の中の入れ替えでは残る。
        let windows = &col.windows;
        col.heights.retain(|id, _| windows.contains(id));
    }
    ws.columns.retain(|col| !col.windows.is_empty());
    for id in tiled {
        if ws.column_of(*id).is_some() {
            continue;
        }
        let anchor = ws
            .focus_history
            .iter()
            .rev()
            .find_map(|h| ws.column_of(*h).map(|(c, _)| c));
        let at = match anchor {
            Some(c) => c.saturating_add(1),
            None => ws.columns.len(),
        };
        let at = at.min(ws.columns.len());
        ws.columns.insert(at, Column::single(*id, default_width));
    }
    if let Some(f) = focus
        && let Some((c, _)) = ws.column_of(f)
        && let Some(col) = ws.columns.get_mut(c)
    {
        col.last_focused = Some(f);
    }
}

/// `Scrolling` のワークスペース `index` の Tiled の窓の矩形を、作業領域 `area` に対して計算し、ビューを調整する（D38・D39）。
///
/// # Errors
/// 列の足し算や座標が i32 に収まらなければ `GeometryOverflow`。1 つの列の窓が高さより多ければ `Layout(AreaTooSmall)`。
pub fn arrange_workspace(
    state: &mut State,
    index: WorkspaceId,
    area: Rect,
) -> Result<(), CoreError> {
    let Some(ws) = state.workspace(index) else {
        return Ok(());
    };
    let (widths, lefts, total) =
        column_layout(ws, area.width()).ok_or(CoreError::GeometryOverflow)?;
    // 列にある窓だけが列を持つので、このワークスペースの列から探せば足りる。
    let focus_col = state.focus.and_then(|f| ws.column_of(f)).map(|(c, _)| c);
    let view = match focus_col.and_then(|c| Some((lefts.get(c)?, widths.get(c)?))) {
        Some((left, width)) => adjust_view(ws.view_x, *left, *width, area.width(), total),
        None => clamp_view(ws.view_x, total, area.width()),
    };

    let mut placed: Vec<(WindowId, Rect)> = Vec::new();
    for ((col, width), left) in ws.columns.iter().zip(&widths).zip(&lefts) {
        let heights = column_heights(col, area.height()).ok_or(LayoutError::AreaTooSmall)?;
        let x = area
            .x()
            .checked_add(*left)
            .and_then(|x| x.checked_sub(view))
            .ok_or(CoreError::GeometryOverflow)?;
        let mut y = area.y();
        for (id, height) in col.windows.iter().zip(heights) {
            let rect = Rect::new(x, y, *width, height).ok_or(CoreError::GeometryOverflow)?;
            placed.push((*id, rect));
            y = y.checked_add(height).ok_or(CoreError::GeometryOverflow)?;
        }
    }

    if let Some(ws) = state.workspace_mut(index) {
        ws.view_x = view;
    }
    for (id, rect) in placed {
        if let Some(w) = state.windows.get_mut(&id) {
            w.rect = Some(rect);
        }
    }
    Ok(())
}

/// フォーカス窓の列での位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusPos {
    /// 属するワークスペース。
    pub ws: WorkspaceId,
    /// 列の位置。
    pub col: usize,
    /// 列の中の行。
    pub row: usize,
    /// フォーカス窓。
    pub id: WindowId,
}

/// フォーカス窓の列での位置。Tiled でなく列にない窓なら `None`。
///
/// # Errors
/// フォーカスが `None` なら `NoFocusedWindow`。
pub fn focus_pos(state: &State) -> Result<Option<FocusPos>, CoreError> {
    let id = state.focus.ok_or(CoreError::NoFocusedWindow)?;
    let Some(window) = state.windows.get(&id) else {
        return Ok(None);
    };
    let pos = state
        .workspace(window.workspace)
        .and_then(|ws| ws.column_of(id))
        .map(|(col, row)| FocusPos {
            ws: window.workspace,
            col,
            row,
            id,
        });
    Ok(pos)
}

/// D41: フォーカス窓 `id` を削除したあとのフォーカス先。
/// 列に窓が残るなら下の窓（なければ上）、列ごと消えるなら右の列（なければ左）の「最後にフォーカスした窓」。
/// `id` が列になければ `None`（呼び出し側が既存の規則を使う）。
#[must_use]
pub fn focus_after_removal(state: &State, id: WindowId) -> Option<WindowId> {
    let ws = state.workspace(state.windows.get(&id)?.workspace)?;
    let (col, row) = ws.column_of(id)?;
    let column = ws.columns.get(col)?;
    if column.windows.len() > 1 {
        let below = row.checked_add(1).and_then(|r| column.windows.get(r));
        let above = row.checked_sub(1).and_then(|r| column.windows.get(r));
        return below.or(above).copied();
    }
    let right = col.checked_add(1).and_then(|c| ws.columns.get(c));
    let left = col.checked_sub(1).and_then(|c| ws.columns.get(c));
    right.or(left).and_then(Column::focus_target)
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;
    use crate::domain::column::Ratio;
    use crate::domain::id::{IdKind, WorkspaceId};

    fn wid(n: u64) -> WindowId {
        WindowId::from_raw(n)
    }

    fn half() -> ColumnWidth {
        ColumnWidth::Proportion(Ratio::HALF)
    }

    #[test]
    fn heights_split_evenly_with_the_remainder_on_top() {
        assert_eq!(split_heights(600, 2), Some(vec![300, 300]));
        assert_eq!(split_heights(10, 3), Some(vec![4, 3, 3]));
        assert_eq!(split_heights(600, 1), Some(vec![600]));
    }

    #[test]
    fn heights_need_one_pixel_each() {
        assert_eq!(split_heights(3, 3), Some(vec![1, 1, 1]));
        assert_eq!(split_heights(2, 3), None);
        assert_eq!(split_heights(600, 0), None);
        assert_eq!(split_heights(0, 1), None);
        assert_eq!(split_heights(-5, 1), None);
    }

    fn prop(n: u32, d: u32) -> WindowHeight {
        Ratio::new(n, d).map_or(WindowHeight::Auto, WindowHeight::Proportion)
    }

    #[test]
    fn all_auto_windows_share_the_height_evenly() {
        let auto = WindowHeight::Auto;
        assert_eq!(distribute_heights(600, &[auto, auto]), Some(vec![300, 300]));
        assert_eq!(
            distribute_heights(10, &[auto, auto, auto]),
            Some(vec![4, 3, 3])
        );
        assert_eq!(distribute_heights(600, &[auto]), Some(vec![600]));
        // Auto だけの列は、これまでの等分（`split_heights`）と同じ。
        assert_eq!(
            distribute_heights(601, &[auto, auto, auto]),
            split_heights(601, 3)
        );
    }

    #[test]
    fn requests_are_taken_and_auto_windows_share_the_rest() {
        let auto = WindowHeight::Auto;
        assert_eq!(
            distribute_heights(600, &[prop(1, 3), auto]),
            Some(vec![200, 400])
        );
        assert_eq!(
            distribute_heights(600, &[WindowHeight::Fixed(580), auto, auto]),
            Some(vec![580, 10, 10])
        );
        // Auto が間にあっても、残りを等分する。
        assert_eq!(
            distribute_heights(600, &[auto, WindowHeight::Fixed(300), auto]),
            Some(vec![150, 300, 150])
        );
    }

    #[test]
    fn a_request_is_cut_to_leave_one_pixel_for_each_other_window() {
        let auto = WindowHeight::Auto;
        assert_eq!(
            distribute_heights(600, &[WindowHeight::Fixed(599), auto, auto]),
            Some(vec![598, 1, 1])
        );
        assert_eq!(
            distribute_heights(600, &[WindowHeight::Fixed(10_000), auto]),
            Some(vec![599, 1])
        );
        // 上にある Auto にも 1 px を残す。
        assert_eq!(
            distribute_heights(600, &[auto, WindowHeight::Fixed(10_000)]),
            Some(vec![1, 599])
        );
    }

    #[test]
    fn the_last_window_takes_the_rest_when_none_is_auto() {
        let two_thirds = prop(2, 3);
        assert_eq!(
            distribute_heights(600, &[two_thirds, two_thirds, two_thirds]),
            Some(vec![400, 199, 1])
        );
        // 最後の窓自身の要求は使わない。
        assert_eq!(
            distribute_heights(600, &[prop(1, 3), WindowHeight::Fixed(5)]),
            Some(vec![200, 400])
        );
    }

    #[test]
    fn distribution_needs_a_window_per_pixel() {
        let auto = WindowHeight::Auto;
        assert_eq!(
            distribute_heights(3, &[auto, auto, auto]),
            Some(vec![1, 1, 1])
        );
        assert_eq!(distribute_heights(2, &[auto, auto, auto]), None);
        assert_eq!(distribute_heights(600, &[]), None);
        assert_eq!(distribute_heights(0, &[auto]), None);
        assert_eq!(distribute_heights(-5, &[auto]), None);
    }

    /// どの要求の並びでも、窓の数が高さ以下なら、高さは 1 以上で合計がちょうど `total` になる（境界値を全部たどる）。
    #[test]
    fn every_distribution_fills_the_height_exactly() {
        let choices = [
            WindowHeight::Auto,
            WindowHeight::Fixed(1),
            WindowHeight::Fixed(7),
            WindowHeight::Fixed(100),
            prop(1, 3),
            prop(1, 1),
        ];
        for total in 1..=24 {
            for n in 1..=4usize {
                let mut index = vec![0usize; n];
                loop {
                    let requests: Vec<WindowHeight> = index
                        .iter()
                        .filter_map(|i| choices.get(*i))
                        .copied()
                        .collect();
                    let got = distribute_heights(total, &requests);
                    if n <= usize::try_from(total).unwrap_or(0) {
                        let heights = got.unwrap_or_default();
                        assert_eq!(heights.len(), n, "{total} {requests:?}");
                        assert!(heights.iter().all(|h| *h >= 1), "{total} {requests:?}");
                        assert_eq!(heights.iter().sum::<i32>(), total, "{total} {requests:?}");
                    } else {
                        assert_eq!(got, None, "{total} {requests:?}");
                    }
                    // 次の組み合わせ（n 桁の choices.len() 進数）。
                    let mut pos = 0;
                    while let Some(digit) = index.get_mut(pos) {
                        *digit += 1;
                        if *digit < choices.len() {
                            break;
                        }
                        *digit = 0;
                        pos += 1;
                    }
                    if pos == n {
                        break;
                    }
                }
            }
        }
    }

    #[test]
    fn column_heights_use_the_columns_requests() {
        let mut column = Column::single(wid(1), half());
        column.windows.push(wid(2));
        assert_eq!(column_heights(&column, 600), Some(vec![300, 300]));
        column.heights.insert(wid(1), prop(1, 3));
        assert_eq!(column_heights(&column, 600), Some(vec![200, 400]));
    }

    #[test]
    fn lefts_run_from_zero_without_gaps() {
        assert_eq!(
            column_lefts(&[450, 300, 600]),
            Some((vec![0, 450, 750], 1350))
        );
        assert_eq!(column_lefts(&[]), Some((vec![], 0)));
        assert_eq!(column_lefts(&[i32::MAX, 1]), None);
    }

    #[test]
    fn clamp_keeps_the_view_inside_the_columns() {
        assert_eq!(clamp_view(-10, 1350, 900), 0);
        assert_eq!(clamp_view(600, 1350, 900), 450);
        assert_eq!(clamp_view(100, 1350, 900), 100);
        // 総幅が作業領域より狭ければ 0。
        assert_eq!(clamp_view(50, 450, 900), 0);
    }

    #[test]
    fn adjust_scrolls_just_enough() {
        // 見えている列は動かさない。
        assert_eq!(adjust_view(450, 450, 450, 900, 1350), 450);
        // 右にはみ出す列: 右端が作業領域の右端に合うまで。
        assert_eq!(adjust_view(0, 900, 450, 900, 1350), 450);
        // 左にはみ出す列: 左端に合わせる。
        assert_eq!(adjust_view(450, 0, 450, 900, 1350), 0);
    }

    #[test]
    fn adjust_aligns_a_wide_column_to_its_left_edge() {
        assert_eq!(adjust_view(0, 450, 1200, 900, 1650), 450);
        // 幅がちょうど作業領域と同じ列も左端を合わせる。
        assert_eq!(adjust_view(0, 900, 900, 900, 1800), 900);
    }

    #[test]
    fn centering_puts_the_column_in_the_middle_and_stays_inside_the_content() {
        // 列 450〜900（中心 675）を、幅 900 の作業領域の中央（450）に置く: 675 − 450 = 225。
        assert_eq!(center_view(450, 450, 900, 1350), 225);
        // 端の列は、内容の端で止まる。
        assert_eq!(center_view(900, 450, 900, 1350), 450);
        assert_eq!(center_view(0, 450, 900, 1350), 0);
        // 内容が作業領域より狭ければ 0。
        assert_eq!(center_view(450, 450, 900, 900), 0);
        // 奇数の幅は、切り捨てた半分どうしで引く。
        assert_eq!(center_view(100, 301, 500, 2000), 0);
        assert_eq!(center_view(1000, 301, 500, 3000), 900);
    }

    #[test]
    fn adjust_is_idempotent() {
        for (view, left, width) in [(0, 0, 450), (450, 900, 450), (0, 900, 450), (700, 100, 300)] {
            let once = adjust_view(view, left, width, 900, 1800);
            assert_eq!(adjust_view(once, left, width, 900, 1800), once);
        }
    }

    #[test]
    fn column_widths_follow_the_area_width() {
        let columns = vec![
            Column::single(wid(1), half()),
            Column::single(wid(2), ColumnWidth::Fixed(200)),
        ];
        assert_eq!(column_widths(&columns, 900), vec![450, 200]);
        assert_eq!(column_widths(&columns, 1000), vec![500, 200]);
    }

    fn workspace() -> Workspace {
        Workspace::new(WorkspaceId::from_raw(1))
    }

    #[test]
    fn sync_adds_new_windows_to_the_right_of_the_last_focused_column() {
        let mut ws = workspace();
        ws.windows = vec![wid(1), wid(2), wid(3)];
        sync_workspace(&mut ws, &[wid(1)], Some(wid(1)), half());
        ws.record_focus(wid(1));
        // 2 を足す: 履歴の最後（2 自身）は列がないので飛ばして、1 の列の右へ。
        ws.record_focus(wid(2));
        sync_workspace(&mut ws, &[wid(1), wid(2)], Some(wid(2)), half());
        ws.record_focus(wid(1));
        ws.record_focus(wid(3));
        sync_workspace(&mut ws, &[wid(1), wid(2), wid(3)], Some(wid(3)), half());
        let order: Vec<_> = ws.columns.iter().flat_map(|c| c.windows.clone()).collect();
        // 3 は、直前にフォーカスされていた 1 の列の右隣に入る。
        assert_eq!(order, vec![wid(1), wid(3), wid(2)]);
    }

    #[test]
    fn sync_removes_windows_that_are_no_longer_tiled_and_empty_columns() {
        let mut ws = workspace();
        sync_workspace(&mut ws, &[wid(1), wid(2)], None, half());
        assert_eq!(ws.columns.len(), 2);
        sync_workspace(&mut ws, &[wid(2)], None, half());
        assert_eq!(ws.columns.len(), 1);
        assert_eq!(ws.column_of(wid(2)), Some((0, 0)));
        assert_eq!(ws.column_of(wid(1)), None);
    }

    #[test]
    fn a_window_that_leaves_its_column_loses_its_height() {
        let mut ws = workspace();
        sync_workspace(&mut ws, &[wid(1), wid(2), wid(3)], None, half());
        // 列 [1, 2] と列 [3]。1 と 3 に高さを持たせる。
        if let Some(c) = ws.columns.get_mut(1) {
            c.windows.clear();
        }
        if let Some(c) = ws.columns.first_mut() {
            c.windows.push(wid(2));
            c.heights.insert(wid(1), prop(1, 3));
        }
        ws.columns.retain(|c| !c.windows.is_empty());
        if let Some(c) = ws.columns.last_mut() {
            c.heights.insert(wid(3), WindowHeight::Fixed(50));
        }
        sync_workspace(&mut ws, &[wid(1), wid(2), wid(3)], None, half());
        // 同じ列にいる窓の高さは残る。
        assert_eq!(
            ws.columns.first().map(|c| c.height_of(wid(1))),
            Some(prop(1, 3))
        );
        assert_eq!(
            ws.columns.last().map(|c| c.height_of(wid(3))),
            Some(WindowHeight::Fixed(50))
        );
        // 1 が別の列（3 の列）へ移ると、1 の高さは Auto に戻る。3 の高さは残る。
        if let Some(c) = ws.columns.first_mut() {
            c.windows.retain(|w| *w != wid(1));
        }
        if let Some(c) = ws.columns.last_mut() {
            c.windows.push(wid(1));
        }
        sync_workspace(&mut ws, &[wid(1), wid(2), wid(3)], None, half());
        let last = ws.columns.last();
        assert_eq!(last.map(|c| c.height_of(wid(1))), Some(WindowHeight::Auto));
        assert_eq!(
            last.map(|c| c.height_of(wid(3))),
            Some(WindowHeight::Fixed(50))
        );
        assert!(
            ws.columns
                .iter()
                .all(|c| c.heights.keys().all(|id| c.windows.contains(id)))
        );
    }

    #[test]
    fn heights_of_removed_windows_are_dropped() {
        let mut ws = workspace();
        sync_workspace(&mut ws, &[wid(1)], None, half());
        if let Some(c) = ws.columns.first_mut() {
            c.windows.push(wid(2));
            c.heights.insert(wid(2), prop(1, 2));
        }
        sync_workspace(&mut ws, &[wid(1)], None, half());
        assert_eq!(ws.columns.first().map(|c| c.heights.len()), Some(0));
    }

    #[test]
    fn sync_records_the_focused_window_as_the_columns_last_focused() {
        let mut ws = workspace();
        ws.columns = vec![Column {
            windows: vec![wid(1), wid(2)],
            width: half(),
            last_focused: Some(wid(1)),
            restore_width: None,
            heights: alloc::collections::BTreeMap::new(),
        }];
        sync_workspace(&mut ws, &[wid(1), wid(2)], Some(wid(2)), half());
        assert_eq!(
            ws.columns.first().and_then(|c| c.last_focused),
            Some(wid(2))
        );
        // 消えた窓を指す last_focused は外れる。
        sync_workspace(&mut ws, &[wid(1)], None, half());
        assert_eq!(ws.columns.first().and_then(|c| c.last_focused), None);
    }

    #[test]
    fn move_column_shifts_the_others() {
        let mut ws = workspace();
        sync_workspace(&mut ws, &[wid(1), wid(2), wid(3)], None, half());
        ws.move_column(2, 0);
        let order: Vec<_> = ws.columns.iter().flat_map(|c| c.windows.clone()).collect();
        assert_eq!(order, vec![wid(3), wid(1), wid(2)]);
        ws.move_column(0, 9);
        ws.move_column(9, 0);
        let again: Vec<_> = ws.columns.iter().flat_map(|c| c.windows.clone()).collect();
        assert_eq!(again, order);
    }
}
