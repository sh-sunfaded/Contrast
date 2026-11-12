//! 不変条件 I1〜I6・I9〜I13 の検査（テストから呼ぶ）。I7・I8 は状態機械テストで確かめる。

use alloc::vec::Vec;

use crate::application::scrolling;
use crate::domain::column::LayoutMode;
use crate::domain::id::{OutputId, WindowId, WorkspaceId, WorkspaceIndex};
use crate::domain::state::State;
use crate::domain::window::WindowState;

/// 見つかった不変条件の違反。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvariantViolation {
    /// I1: 窓がちょうど 1 つのワークスペースに属していない。
    I1(WindowId),
    /// I2: 順序列に重複か、存在しない窓がある。
    I2(WorkspaceIndex),
    /// I3: 出力の表示ワークスペースが範囲外か、2 つの出力で重なっている。
    I3(OutputId),
    /// I4: フォーカスが存在しない窓か、表示されていない窓を指している。
    I4(WindowId),
    /// I5: 表示中の Tiled 窓が作業領域からはみ出すか、重なっている。
    I5(WindowId),
    /// I6: ExternalKey と ID の対応が全単射でない。
    I6,
    /// I9: 同じワークスペースに Fullscreen の窓が 2 枚以上ある。
    I9(WorkspaceIndex),
    /// I10: `Scrolling` で、Tiled の窓がちょうど 1 つの列に属していない、空の列がある、列の幅が 1 未満、
    /// または表示中のワークスペースの列で窓の高さの合計が作業領域の高さに等しくない（D55。窓の数が高さ以下のとき）。
    I10(WorkspaceIndex),
    /// I11: `Scrolling` で、フォーカスが Tiled の窓にあるのに、その列が D39 の条件を満たしていない。
    I11(WindowId),
    /// I12: ワークスペースの id が重複している。
    /// `Dynamic` では、列の末尾が空でない・列が空のときも（D70・D74）。
    I12(WorkspaceIndex),
    /// I13（D83）: `Dynamic` で、持ち主の出力・表示・アクティブな出力・フォーカスの整合が崩れている。
    I13,
    /// I14（D91）: ジェスチャーが 2 つ以上ある、または表示されていない・列の無いワークスペースにある。
    I14,
}

/// I7 の比較用の状態の複製。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateSnapshot(State);

impl StateSnapshot {
    /// 状態の複製から作る。
    pub(crate) const fn new(state: State) -> Self {
        Self(state)
    }
}

/// I1〜I6、I9〜I11 を順に検査し、最初の違反を返す。
///
/// # Errors
/// 違反があればその種類。
pub fn check(state: &State) -> Result<(), InvariantViolation> {
    check_i1_i2(state)?;
    check_i3(state)?;
    check_i4(state)?;
    check_i5(state)?;
    check_i6(state)?;
    check_i9(state)?;
    check_i10(state)?;
    check_i11(state)?;
    check_i12(state)?;
    check_i13(state)?;
    check_i14(state)
}

/// I14（D91）: ジェスチャーは高々 1 つで、表示中の、列のあるワークスペースにある。
fn check_i14(state: &State) -> Result<(), InvariantViolation> {
    let with: Vec<&crate::domain::workspace::Workspace> = state
        .workspaces
        .iter()
        .filter(|ws| ws.gesture.is_some())
        .collect();
    let ok = with.len() <= 1
        && with
            .iter()
            .all(|ws| !ws.columns.is_empty() && state.output_showing(ws.id).is_some());
    if ok {
        Ok(())
    } else {
        Err(InvariantViolation::I14)
    }
}

/// ワークスペースの全体の並びでの位置（違反の報告用）。
fn pos(state: &State, id: WorkspaceId) -> WorkspaceIndex {
    state.index_of(id).unwrap_or(WorkspaceIndex::new(u8::MAX))
}

/// I12（D63・D70・D74）: id は互いに異なる。`Dynamic` では、各出力の列（出力が 0 台なら持ち主の無い列）は
/// 1 個以上で、末尾は空（個数の上限のときを除く）。
fn check_i12(state: &State) -> Result<(), InvariantViolation> {
    let mut seen = alloc::collections::BTreeSet::new();
    for ws in &state.workspaces {
        if !seen.insert(ws.id) {
            return Err(InvariantViolation::I12(pos(state, ws.id)));
        }
    }
    // D84: `OutputAdded` の直後は、元の出力の列の末尾の空の補充を次の命令まで延ばす（niri の `add_output`）。
    if state.is_dynamic() && !state.trailing_deferred {
        let at_limit = state.workspaces.len() >= crate::domain::state::MAX_DYNAMIC_WORKSPACES;
        let owners: Vec<Option<OutputId>> = if state.outputs.is_empty() {
            alloc::vec![None]
        } else {
            state.outputs.keys().copied().map(Some).collect()
        };
        for owner in owners {
            match state
                .list_of(owner)
                .last()
                .and_then(|id| state.workspace(*id))
            {
                None => return Err(InvariantViolation::I12(WorkspaceIndex::new(0))),
                Some(last) if !last.windows.is_empty() && !at_limit => {
                    return Err(InvariantViolation::I12(pos(state, last.id)));
                }
                Some(_) => {}
            }
        }
    }
    Ok(())
}

/// I13（D83）: `Dynamic` の持ち主の出力・表示・アクティブな出力・フォーカス。全体の並びは D73 の順。
fn check_i13(state: &State) -> Result<(), InvariantViolation> {
    if !state.is_dynamic() {
        return Ok(());
    }
    if state.outputs.is_empty() {
        if state.active_output.is_some() || state.workspaces.iter().any(|ws| ws.output.is_some()) {
            return Err(InvariantViolation::I13);
        }
        return Ok(());
    }
    let owners_ok = state
        .workspaces
        .iter()
        .all(|ws| ws.output.is_some_and(|o| state.outputs.contains_key(&o)));
    let shown_ok = state.outputs.values().all(|o| {
        state
            .workspace(o.shown)
            .is_some_and(|ws| ws.output == Some(o.id))
    });
    let ordered = state
        .workspaces
        .windows(2)
        .all(|pair| matches!(pair, [a, b] if a.output <= b.output));
    let active = state
        .active_output
        .filter(|o| state.outputs.contains_key(o));
    let focus_ok = match (state.focus, active) {
        (None, _) => true,
        (Some(f), Some(a)) => {
            let shown = state.outputs.get(&a).map(|o| o.shown);
            state
                .windows
                .get(&f)
                .is_some_and(|w| Some(w.workspace) == shown)
        }
        (Some(_), None) => false,
    };
    if owners_ok && shown_ok && ordered && active.is_some() && focus_ok {
        Ok(())
    } else {
        Err(InvariantViolation::I13)
    }
}

/// I1・I2: 所属と順序列の整合。
fn check_i1_i2(state: &State) -> Result<(), InvariantViolation> {
    for ws in &state.workspaces {
        let mut seen: Vec<WindowId> = Vec::with_capacity(ws.windows.len());
        for id in &ws.windows {
            let belongs = state.windows.get(id).is_some_and(|w| w.workspace == ws.id);
            if seen.contains(id) || !belongs {
                return Err(InvariantViolation::I2(pos(state, ws.id)));
            }
            seen.push(*id);
        }
        if ws.focus_history.iter().any(|id| !ws.windows.contains(id)) {
            return Err(InvariantViolation::I2(pos(state, ws.id)));
        }
    }
    for w in state.windows.values() {
        let count = state
            .workspaces
            .iter()
            .filter(|ws| ws.windows.contains(&w.id))
            .count();
        // 1 つのワークスペースにだけあり、それが `w.workspace` であること（後者は上の順序列の検査が保証する）。
        if count != 1 {
            return Err(InvariantViolation::I1(w.id));
        }
    }
    Ok(())
}

/// I3: 各出力の表示ワークスペースは範囲内で、互いに異なる。
fn check_i3(state: &State) -> Result<(), InvariantViolation> {
    let mut shown: Vec<WorkspaceId> = Vec::with_capacity(state.outputs.len());
    for o in state.outputs.values() {
        if state.workspace(o.shown).is_none() || shown.contains(&o.shown) {
            return Err(InvariantViolation::I3(o.id));
        }
        shown.push(o.shown);
    }
    Ok(())
}

/// I4: フォーカスは None か表示中の窓。
fn check_i4(state: &State) -> Result<(), InvariantViolation> {
    match state.focus {
        Some(id) if !state.is_window_visible(id) => Err(InvariantViolation::I4(id)),
        _ => Ok(()),
    }
}

/// I5: 表示中の Tiled 窓は作業領域内に収まり、互いに重ならない。
fn check_i5(state: &State) -> Result<(), InvariantViolation> {
    for o in state.outputs.values() {
        // 作業領域が作れない出力は handler が拒否するので、ここには来ない。
        let Ok(area) = o.work_area() else {
            continue;
        };
        let Some(ws) = state.workspace(o.shown) else {
            continue;
        };
        let mut placed: Vec<(WindowId, crate::domain::geometry::Rect)> = Vec::new();
        for id in &ws.windows {
            let Some(w) = state.windows.get(id) else {
                continue;
            };
            if w.state != WindowState::Tiled {
                continue;
            }
            let Some(rect) = w.rect else {
                return Err(InvariantViolation::I5(*id));
            };
            // Scrolling（D47）: 縦だけが作業領域に収まればよい。横はビューの外に出てよい。
            let inside = if state.layout.mode == LayoutMode::Scrolling {
                rect.y() >= area.y() && rect.bottom() <= area.bottom()
            } else {
                area.intersect(&rect) == Some(rect)
            };
            let overlaps = placed.iter().any(|(_, r)| r.intersect(&rect).is_some());
            if !inside || overlaps {
                return Err(InvariantViolation::I5(*id));
            }
            placed.push((*id, rect));
        }
    }
    Ok(())
}

/// I6: ExternalKey と ID の対応が全単射で、登録済みの実体と一致する。
fn check_i6(state: &State) -> Result<(), InvariantViolation> {
    let windows_ok = state.window_keys.is_bijective()
        && state.window_keys.len() == state.windows.len()
        && state
            .windows
            .keys()
            .all(|id| state.window_keys.key_of(*id).is_some());
    let outputs_ok = state.output_keys.is_bijective()
        && state.output_keys.len() == state.outputs.len()
        && state
            .outputs
            .keys()
            .all(|id| state.output_keys.key_of(*id).is_some());
    if windows_ok && outputs_ok {
        Ok(())
    } else {
        Err(InvariantViolation::I6)
    }
}

/// I9: 同じワークスペースの Fullscreen の窓は高々 1 枚（D26）。
fn check_i9(state: &State) -> Result<(), InvariantViolation> {
    for ws in &state.workspaces {
        let fullscreen = ws
            .windows
            .iter()
            .filter_map(|id| state.windows.get(id))
            .filter(|w| w.state == WindowState::Fullscreen)
            .count();
        if fullscreen > 1 {
            return Err(InvariantViolation::I9(pos(state, ws.id)));
        }
    }
    Ok(())
}

/// I10: `Scrolling` では、各 Tiled の窓がちょうど 1 つの列に属し、空の列がなく、列の幅は 1 以上。
/// さらに（D55）、表示中のワークスペースの各列で、窓の高さの合計は作業領域の高さに等しい。
fn check_i10(state: &State) -> Result<(), InvariantViolation> {
    if state.layout.mode != LayoutMode::Scrolling {
        return Ok(());
    }
    for ws in &state.workspaces {
        let bad_column = ws
            .columns
            .iter()
            .any(|col| col.windows.is_empty() || !column_width_ok(col.width));
        let tiled_ok = ws.windows.iter().all(|id| {
            let tiled = state
                .windows
                .get(id)
                .is_some_and(|w| w.state == WindowState::Tiled);
            let count = ws.columns.iter().filter(|c| c.windows.contains(id)).count();
            if tiled { count == 1 } else { count == 0 }
        });
        let in_columns: usize = ws.columns.iter().map(|c| c.windows.len()).sum();
        let tiled_total = ws
            .windows
            .iter()
            .filter(|id| {
                state
                    .windows
                    .get(id)
                    .is_some_and(|w| w.state == WindowState::Tiled)
            })
            .count();
        if bad_column || !tiled_ok || in_columns != tiled_total {
            return Err(InvariantViolation::I10(pos(state, ws.id)));
        }
    }
    check_i10_heights(state)
}

/// I10（D55）: 表示中のワークスペースの各列で、窓の高さの合計が作業領域の高さに等しい。
/// 窓の数が高さより多い列は検査しない（その配置は `Layout(AreaTooSmall)` で拒否される）。
fn check_i10_heights(state: &State) -> Result<(), InvariantViolation> {
    for output in state.outputs.values() {
        let Ok(area) = output.work_area() else {
            continue;
        };
        let Some(ws) = state.workspace(output.shown) else {
            continue;
        };
        for column in &ws.columns {
            let fits = usize::try_from(area.height()).is_ok_and(|h| column.windows.len() <= h);
            if !fits {
                continue;
            }
            let sum = column.windows.iter().try_fold(0_i32, |sum, id| {
                let rect = state.windows.get(id)?.rect?;
                sum.checked_add(rect.height())
            });
            if sum != Some(area.height()) {
                return Err(InvariantViolation::I10(pos(state, ws.id)));
            }
        }
    }
    Ok(())
}

/// 列の幅が 1 以上か（`Fixed` が 1 未満でない）。
fn column_width_ok(width: crate::domain::column::ColumnWidth) -> bool {
    !matches!(width, crate::domain::column::ColumnWidth::Fixed(px) if px < 1)
}

/// I11: `Scrolling` で、フォーカスが Tiled の窓にあるとき、その列は D39 の条件を満たす（ビューがそれ以上動かない）。
fn check_i11(state: &State) -> Result<(), InvariantViolation> {
    if state.layout.mode != LayoutMode::Scrolling {
        return Ok(());
    }
    let Some(id) = state.focus else {
        return Ok(());
    };
    let Some(w) = state.windows.get(&id) else {
        return Ok(());
    };
    if w.state != WindowState::Tiled {
        return Ok(());
    }
    let Some(ws) = state.workspace(w.workspace) else {
        return Ok(());
    };
    // ジェスチャー中のワークスペースには当てない（D91）。
    if ws.gesture.is_some() {
        return Ok(());
    }
    let Some(area_width) = scrolling::shown_area_width(state, w.workspace) else {
        return Ok(());
    };
    let layout = scrolling::column_layout(ws, area_width);
    let ok = ws
        .column_of(id)
        .and_then(|(c, _)| {
            let (widths, lefts, total) = layout.as_ref()?;
            let (left, width) = (lefts.get(c)?, widths.get(c)?);
            Some(scrolling::adjust_view(ws.view_x, *left, *width, area_width, *total) == ws.view_x)
        })
        .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(InvariantViolation::I11(id))
    }
}

/// 壊した State が、その不変条件の variant を返すことの確認（I1〜I11）。
#[cfg(test)]
mod tests {
    use super::{InvariantViolation, check};
    use crate::domain::column::{Column, ColumnWidth, LayoutMode, Ratio};
    use crate::domain::geometry::Rect;
    use crate::domain::id::{ExternalKey, IdKind, OutputId, WindowId, WorkspaceId, WorkspaceIndex};
    use crate::domain::insets::Insets;
    use crate::domain::output::{Output, OutputName};
    use crate::domain::state::State;
    use crate::domain::window::{Window, WindowMeta, WindowState};

    /// 試験用の作業領域。
    fn area() -> Option<Rect> {
        Rect::new(0, 0, 100, 100)
    }

    /// 窓 `raw` を Tiled・ワークスペース 0・作業領域全体で足す（キーも登録する）。
    fn add_window(state: &mut State, raw: u64, window_state: WindowState) -> Option<()> {
        let id = WindowId::from_raw(raw);
        let index = WorkspaceId::from_raw(1);
        let mut w = Window::new(id, WindowMeta::default(), window_state, index);
        if window_state == WindowState::Tiled {
            w.rect = area();
        }
        state.windows.insert(id, w);
        state.workspace_mut(index)?.push_window(id);
        state.window_keys.insert(ExternalKey::new(raw), id).ok()
    }

    /// 全不変条件を満たす State: 出力 1 つがワークスペース 0 を表示し、窓 1 が Tiled でフォーカス。
    fn valid() -> Option<State> {
        let mut state = State::new(2);
        let oid = OutputId::from_raw(1);
        let output = Output {
            id: oid,
            name: OutputName::new("test"),
            geometry: area()?,
            shown: WorkspaceId::from_raw(1),
            insets: Insets::default(),
        };
        state.outputs.insert(oid, output);
        state.output_keys.insert(ExternalKey::new(1000), oid).ok()?;
        add_window(&mut state, 1, WindowState::Tiled)?;
        state.focus = Some(WindowId::from_raw(1));
        Some(state)
    }

    /// 壊す前の State は検査を通る。
    #[test]
    fn valid_state_passes() -> Result<(), &'static str> {
        let state = valid().ok_or("setup")?;
        assert_eq!(check(&state), Ok(()));
        Ok(())
    }

    /// I1: どのワークスペースの順序列にもない窓。
    #[test]
    fn detects_i1() -> Result<(), &'static str> {
        let mut state = valid().ok_or("setup")?;
        state
            .workspace_mut(WorkspaceId::from_raw(1))
            .ok_or("ws")?
            .windows
            .clear();
        assert_eq!(
            check(&state),
            Err(InvariantViolation::I1(WindowId::from_raw(1)))
        );
        Ok(())
    }

    /// I2: 順序列の重複。
    #[test]
    fn detects_i2() -> Result<(), &'static str> {
        let mut state = valid().ok_or("setup")?;
        let index = WorkspaceId::from_raw(1);
        state
            .workspace_mut(index)
            .ok_or("ws")?
            .windows
            .push(WindowId::from_raw(1));
        assert_eq!(
            check(&state),
            Err(InvariantViolation::I2(WorkspaceIndex::new(0)))
        );
        Ok(())
    }

    /// I3: 表示ワークスペースが範囲外。
    #[test]
    fn detects_i3() -> Result<(), &'static str> {
        let mut state = valid().ok_or("setup")?;
        let oid = OutputId::from_raw(1);
        state.outputs.get_mut(&oid).ok_or("output")?.shown = WorkspaceId::from_raw(99);
        assert_eq!(check(&state), Err(InvariantViolation::I3(oid)));
        Ok(())
    }

    /// I3: 2 つの出力が同じワークスペースを表示している。
    #[test]
    fn detects_i3_when_two_outputs_share_a_workspace() -> Result<(), &'static str> {
        let mut state = valid().ok_or("setup")?;
        let second = OutputId::from_raw(2);
        let output = Output {
            id: second,
            name: OutputName::new("second"),
            geometry: area().ok_or("area")?,
            shown: WorkspaceId::from_raw(1),
            insets: Insets::default(),
        };
        state.outputs.insert(second, output);
        state
            .output_keys
            .insert(ExternalKey::new(1001), second)
            .map_err(|_| "key")?;
        assert_eq!(check(&state), Err(InvariantViolation::I3(second)));
        Ok(())
    }

    /// I4: フォーカスが存在しない窓を指す。
    #[test]
    fn detects_i4() -> Result<(), &'static str> {
        let mut state = valid().ok_or("setup")?;
        let ghost = WindowId::from_raw(99);
        state.focus = Some(ghost);
        assert_eq!(check(&state), Err(InvariantViolation::I4(ghost)));
        Ok(())
    }

    /// I5: Tiled 窓が作業領域からはみ出す。
    #[test]
    fn detects_i5() -> Result<(), &'static str> {
        let mut state = valid().ok_or("setup")?;
        let id = WindowId::from_raw(1);
        state.windows.get_mut(&id).ok_or("window")?.rect = Rect::new(0, 0, 200, 100);
        assert_eq!(check(&state), Err(InvariantViolation::I5(id)));
        Ok(())
    }

    /// I6: 窓のない ID がキー表にある。
    #[test]
    fn detects_i6() -> Result<(), &'static str> {
        let mut state = valid().ok_or("setup")?;
        state
            .window_keys
            .insert(ExternalKey::new(2), WindowId::from_raw(77))
            .map_err(|_| "key")?;
        assert_eq!(check(&state), Err(InvariantViolation::I6));
        Ok(())
    }

    /// I9: 同じワークスペースに Fullscreen の窓が 2 枚。
    #[test]
    fn detects_i9() -> Result<(), &'static str> {
        let mut state = valid().ok_or("setup")?;
        state
            .windows
            .get_mut(&WindowId::from_raw(1))
            .ok_or("window")?
            .state = WindowState::Fullscreen;
        add_window(&mut state, 2, WindowState::Fullscreen).ok_or("setup")?;
        assert_eq!(
            check(&state),
            Err(InvariantViolation::I9(WorkspaceIndex::new(0)))
        );
        Ok(())
    }

    /// Scrolling の State: `valid()` を、窓 1 が幅 1/1 の 1 列に入っている状態にしたもの。
    fn scrolling_valid() -> Option<State> {
        let mut state = valid()?;
        state.layout.mode = LayoutMode::Scrolling;
        let ws = state.workspace_mut(WorkspaceId::from_raw(1))?;
        ws.columns.push(Column::single(
            WindowId::from_raw(1),
            ColumnWidth::Proportion(Ratio::FULL),
        ));
        Some(state)
    }

    /// Scrolling の健全な State は検査を通る。
    #[test]
    fn valid_scrolling_state_passes() -> Result<(), &'static str> {
        let state = scrolling_valid().ok_or("setup")?;
        assert_eq!(check(&state), Ok(()));
        Ok(())
    }

    /// I5（Scrolling）: 横にビューの外へ出ていてもよい。
    #[test]
    fn scrolling_allows_rects_outside_the_view_horizontally() -> Result<(), &'static str> {
        let mut state = scrolling_valid().ok_or("setup")?;
        state
            .windows
            .get_mut(&WindowId::from_raw(1))
            .ok_or("window")?
            .rect = Rect::new(-50, 0, 100, 100);
        assert_eq!(check(&state), Ok(()));
        Ok(())
    }

    /// I5（Scrolling）: 縦が作業領域からはみ出す。
    #[test]
    fn detects_i5_scrolling_vertical_overflow() -> Result<(), &'static str> {
        let mut state = scrolling_valid().ok_or("setup")?;
        let id = WindowId::from_raw(1);
        state.windows.get_mut(&id).ok_or("window")?.rect = Rect::new(0, 10, 100, 100);
        assert_eq!(check(&state), Err(InvariantViolation::I5(id)));
        Ok(())
    }

    /// I5（Scrolling）: Tiled の窓が重なる。
    #[test]
    fn detects_i5_scrolling_overlap() -> Result<(), &'static str> {
        let mut state = scrolling_valid().ok_or("setup")?;
        add_window(&mut state, 2, WindowState::Tiled).ok_or("setup")?;
        state
            .workspace_mut(WorkspaceId::from_raw(1))
            .ok_or("ws")?
            .columns
            .push(Column::single(
                WindowId::from_raw(2),
                ColumnWidth::Proportion(Ratio::FULL),
            ));
        assert_eq!(
            check(&state),
            Err(InvariantViolation::I5(WindowId::from_raw(2)))
        );
        Ok(())
    }

    /// I10（D55）: 列の窓の高さの合計が作業領域の高さに足りない。
    #[test]
    fn detects_i10_when_a_column_does_not_fill_the_height() -> Result<(), &'static str> {
        let mut state = scrolling_valid().ok_or("setup")?;
        let id = WindowId::from_raw(1);
        state.windows.get_mut(&id).ok_or("window")?.rect = Rect::new(0, 0, 100, 50);
        assert_eq!(
            check(&state),
            Err(InvariantViolation::I10(WorkspaceIndex::new(0)))
        );
        Ok(())
    }

    /// I10（D55）: 列に窓が複数あるとき、高さの合計が作業領域の高さに等しければ通り、等しくなければ違反。
    #[test]
    fn checks_the_sum_of_heights_in_a_stacked_column() -> Result<(), &'static str> {
        let mut state = scrolling_valid().ok_or("setup")?;
        add_window(&mut state, 2, WindowState::Tiled).ok_or("add 2")?;
        add_window(&mut state, 3, WindowState::Tiled).ok_or("add 3")?;
        let index = WorkspaceId::from_raw(1);
        let column = state
            .workspace_mut(index)
            .ok_or("ws")?
            .columns
            .first_mut()
            .ok_or("column")?;
        column.windows.push(WindowId::from_raw(2));
        column.windows.push(WindowId::from_raw(3));
        let rects = [(1, 0, 30), (2, 30, 30), (3, 60, 40)];
        for (raw, y, h) in rects {
            state
                .windows
                .get_mut(&WindowId::from_raw(raw))
                .ok_or("window")?
                .rect = Rect::new(0, y, 100, h);
        }
        assert_eq!(check(&state), Ok(()));
        // 合計が 100 に足りない（隙間ができる）。
        state
            .windows
            .get_mut(&WindowId::from_raw(3))
            .ok_or("window")?
            .rect = Rect::new(0, 60, 100, 30);
        assert_eq!(
            check(&state),
            Err(InvariantViolation::I10(WorkspaceIndex::new(0)))
        );
        Ok(())
    }

    /// I10: Tiled の窓がどの列にもない。
    #[test]
    fn detects_i10_when_a_tiled_window_has_no_column() -> Result<(), &'static str> {
        let mut state = scrolling_valid().ok_or("setup")?;
        let index = WorkspaceId::from_raw(1);
        state.workspace_mut(index).ok_or("ws")?.columns.clear();
        assert_eq!(
            check(&state),
            Err(InvariantViolation::I10(WorkspaceIndex::new(0)))
        );
        Ok(())
    }

    /// I10: 窓が 2 つの列にある。
    #[test]
    fn detects_i10_when_a_window_is_in_two_columns() -> Result<(), &'static str> {
        let mut state = scrolling_valid().ok_or("setup")?;
        let index = WorkspaceId::from_raw(1);
        state
            .workspace_mut(index)
            .ok_or("ws")?
            .columns
            .push(Column::single(
                WindowId::from_raw(1),
                ColumnWidth::Proportion(Ratio::FULL),
            ));
        assert_eq!(
            check(&state),
            Err(InvariantViolation::I10(WorkspaceIndex::new(0)))
        );
        Ok(())
    }

    /// I10: 空の列がある。
    #[test]
    fn detects_i10_when_a_column_is_empty() -> Result<(), &'static str> {
        let mut state = scrolling_valid().ok_or("setup")?;
        let index = WorkspaceId::from_raw(1);
        let ws = state.workspace_mut(index).ok_or("ws")?;
        ws.columns.push(Column {
            windows: alloc::vec::Vec::new(),
            width: ColumnWidth::Proportion(Ratio::FULL),
            last_focused: None,
            restore_width: None,
            heights: alloc::collections::BTreeMap::new(),
        });
        assert_eq!(
            check(&state),
            Err(InvariantViolation::I10(WorkspaceIndex::new(0)))
        );
        Ok(())
    }

    /// I10: 列の幅が 1 未満。
    #[test]
    fn detects_i10_when_a_column_width_is_below_one() -> Result<(), &'static str> {
        let mut state = scrolling_valid().ok_or("setup")?;
        let index = WorkspaceId::from_raw(1);
        let ws = state.workspace_mut(index).ok_or("ws")?;
        ws.columns.first_mut().ok_or("column")?.width = ColumnWidth::Fixed(0);
        assert_eq!(
            check(&state),
            Err(InvariantViolation::I10(WorkspaceIndex::new(0)))
        );
        Ok(())
    }

    /// I10: Floating の窓が列にいる。
    #[test]
    fn detects_i10_when_a_floating_window_is_in_a_column() -> Result<(), &'static str> {
        let mut state = scrolling_valid().ok_or("setup")?;
        let index = WorkspaceId::from_raw(1);
        add_window(&mut state, 2, WindowState::Floating).ok_or("setup")?;
        let ws = state.workspace_mut(index).ok_or("ws")?;
        ws.columns
            .first_mut()
            .ok_or("column")?
            .windows
            .push(WindowId::from_raw(2));
        assert_eq!(
            check(&state),
            Err(InvariantViolation::I10(WorkspaceIndex::new(0)))
        );
        Ok(())
    }

    /// I10 は Equal では検査しない（列は空のまま）。
    #[test]
    fn i10_is_ignored_in_equal_mode() -> Result<(), &'static str> {
        let state = valid().ok_or("setup")?;
        assert!(state.workspaces.iter().all(|ws| ws.columns.is_empty()));
        assert_eq!(check(&state), Ok(()));
        Ok(())
    }

    /// I11: フォーカス列が見える位置にビューがない。
    #[test]
    fn detects_i11_when_the_view_is_out_of_range() -> Result<(), &'static str> {
        let mut state = scrolling_valid().ok_or("setup")?;
        state
            .workspace_mut(WorkspaceId::from_raw(1))
            .ok_or("ws")?
            .view_x = 50;
        assert_eq!(
            check(&state),
            Err(InvariantViolation::I11(WindowId::from_raw(1)))
        );
        Ok(())
    }

    /// I10: 列の窓が順序列にない（数は合っていても、Tiled の窓がどの列にもない）。
    #[test]
    fn detects_i10_when_a_column_holds_a_window_that_does_not_exist() -> Result<(), &'static str> {
        let mut state = scrolling_valid().ok_or("setup")?;
        let index = WorkspaceId::from_raw(1);
        let ws = state.workspace_mut(index).ok_or("ws")?;
        ws.columns.clear();
        ws.columns.push(Column::single(
            WindowId::from_raw(99),
            ColumnWidth::Proportion(Ratio::FULL),
        ));
        assert_eq!(
            check(&state),
            Err(InvariantViolation::I10(WorkspaceIndex::new(0)))
        );
        Ok(())
    }

    /// I6: 窓の対応表で、逆引きだけが食い違う（要素数は合い、窓の数とも合う）。
    #[test]
    fn detects_i6_when_only_the_reverse_lookup_is_wrong() -> Result<(), &'static str> {
        let mut state = valid().ok_or("setup")?;
        state
            .window_keys
            .corrupt_reverse(WindowId::from_raw(1), ExternalKey::new(500));
        assert_eq!(check(&state), Err(InvariantViolation::I6));
        Ok(())
    }

    /// 窓 `raw` をキーなしで足す（ワークスペース 0・Tiled）。
    fn add_extra_window_without_key(state: &mut State, raw: u64) -> Option<()> {
        let id = WindowId::from_raw(raw);
        let index = WorkspaceId::from_raw(1);
        let mut w = Window::new(id, WindowMeta::default(), WindowState::Floating, index);
        w.rect = None;
        state.windows.insert(id, w);
        state.workspace_mut(index)?.push_window(id);
        Some(())
    }

    /// I6: 窓の数と対応の数は合うが、キーのない窓がある（別の ID のキーが余っている）。
    #[test]
    fn detects_i6_when_only_a_window_has_no_key() -> Result<(), &'static str> {
        let mut state = valid().ok_or("setup")?;
        add_extra_window_without_key(&mut state, 2).ok_or("setup")?;
        state
            .window_keys
            .insert(ExternalKey::new(3), WindowId::from_raw(77))
            .map_err(|_| "key")?;
        assert_eq!(check(&state), Err(InvariantViolation::I6));
        Ok(())
    }

    /// I6: 出力の対応表で、逆引きだけが食い違う。
    #[test]
    fn detects_i6_when_only_the_output_reverse_lookup_is_wrong() -> Result<(), &'static str> {
        let mut state = valid().ok_or("setup")?;
        state
            .output_keys
            .corrupt_reverse(OutputId::from_raw(1), ExternalKey::new(900));
        assert_eq!(check(&state), Err(InvariantViolation::I6));
        Ok(())
    }

    /// I6: 出力の対応が 1 件余る（数だけが合わない）。
    #[test]
    fn detects_i6_when_only_the_output_count_differs() -> Result<(), &'static str> {
        let mut state = valid().ok_or("setup")?;
        state
            .output_keys
            .insert(ExternalKey::new(1001), OutputId::from_raw(55))
            .map_err(|_| "key")?;
        assert_eq!(check(&state), Err(InvariantViolation::I6));
        Ok(())
    }

    /// I6: 出力の数と対応の数は合うが、キーのない出力がある。
    #[test]
    fn detects_i6_when_only_an_output_has_no_key() -> Result<(), &'static str> {
        let mut state = valid().ok_or("setup")?;
        let second = OutputId::from_raw(2);
        let output = Output {
            id: second,
            name: OutputName::new("second"),
            geometry: area().ok_or("area")?,
            shown: WorkspaceId::from_raw(2),
            insets: Insets::default(),
        };
        state.outputs.insert(second, output);
        state
            .output_keys
            .insert(ExternalKey::new(1002), OutputId::from_raw(55))
            .map_err(|_| "key")?;
        assert_eq!(check(&state), Err(InvariantViolation::I6));
        Ok(())
    }

    /// `Dynamic` の列（出力 0 台なら持ち主の無い列）の末尾が空でない状態。
    fn dynamic_with_busy_last() -> State {
        let mut state = State::new(1);
        state.layout.workspaces = crate::domain::column::WorkspaceMode::Dynamic;
        if let Some(ws) = state.workspaces.last_mut() {
            ws.windows.push(WindowId::from_raw(1));
        }
        state
    }

    #[test]
    fn i12_requires_a_trailing_empty_workspace_in_dynamic_mode() {
        let state = dynamic_with_busy_last();
        assert!(matches!(
            super::check_i12(&state),
            Err(InvariantViolation::I12(_))
        ));
    }

    #[test]
    fn i12_is_relaxed_only_while_the_trailing_empty_is_deferred() {
        // D84: `OutputAdded` の直後だけ、末尾の空を要求しない。
        let mut state = dynamic_with_busy_last();
        state.trailing_deferred = true;
        assert_eq!(super::check_i12(&state), Ok(()));
        // 空の末尾があれば、延ばしていなくても通る。
        let mut ok = State::new(1);
        ok.layout.workspaces = crate::domain::column::WorkspaceMode::Dynamic;
        assert_eq!(super::check_i12(&ok), Ok(()));
    }

    #[test]
    fn i12_does_not_require_a_trailing_empty_at_the_workspace_limit() {
        let mut state = dynamic_with_busy_last();
        while state.workspaces.len() < crate::domain::state::MAX_DYNAMIC_WORKSPACES {
            let id = WorkspaceId::from_raw(state.workspaces.len() as u64 + 10);
            state
                .workspaces
                .insert(0, crate::domain::workspace::Workspace::new(id));
        }
        assert_eq!(super::check_i12(&state), Ok(()));
        state.workspaces.remove(0);
        assert!(super::check_i12(&state).is_err());
    }

    #[test]
    fn i12_rejects_duplicate_workspace_ids() {
        let mut state = State::new(2);
        let id = WorkspaceId::from_raw(1);
        for ws in &mut state.workspaces {
            ws.id = id;
        }
        assert!(super::check_i12(&state).is_err());
    }

    /// I10: 表示されていないワークスペースの列に、順序列にない窓がある（列は空でなく、幅も正しく、
    /// 順序列の窓はない）。列の窓の総数だけが Tiled の窓の数と食い違う。
    /// 表示中でないので、後段の高さの合計の検査には掛からない（総数の検査だけが見つける）。
    #[test]
    fn detects_i10_when_only_the_total_of_column_windows_differs() -> Result<(), &'static str> {
        let mut state = scrolling_valid().ok_or("setup")?;
        let hidden = state.workspace_mut(WorkspaceId::from_raw(2)).ok_or("ws")?;
        hidden.columns.push(Column::single(
            WindowId::from_raw(99),
            ColumnWidth::Proportion(Ratio::FULL),
        ));
        assert_eq!(
            super::check_i10(&state),
            Err(InvariantViolation::I10(WorkspaceIndex::new(1)))
        );
        Ok(())
    }

    /// `Dynamic` で出力が 1 つ、ワークスペースが 2 つ（持ち主の無い列が先、出力 1 の列が後）の State。
    /// I13 を満たす（持ち主なしの列は、持ち主の検査だけを破るために使う）。
    fn dynamic_with_output() -> Option<State> {
        let mut state = State::new(2);
        state.layout.workspaces = crate::domain::column::WorkspaceMode::Dynamic;
        let oid = OutputId::from_raw(1);
        let shown = WorkspaceId::from_raw(2);
        state.workspace_mut(shown)?.output = Some(oid);
        state.workspace_mut(WorkspaceId::from_raw(1))?.output = Some(oid);
        state.outputs.insert(
            oid,
            Output {
                id: oid,
                name: OutputName::new("test"),
                geometry: area()?,
                shown,
                insets: Insets::default(),
            },
        );
        state.active_output = Some(oid);
        Some(state)
    }

    /// I13: 出力が 0 台で、アクティブな出力だけが残っている。
    #[test]
    fn i13_rejects_an_active_output_when_there_are_no_outputs() {
        let mut state = State::new(1);
        state.layout.workspaces = crate::domain::column::WorkspaceMode::Dynamic;
        assert_eq!(super::check_i13(&state), Ok(()));
        state.active_output = Some(OutputId::from_raw(1));
        assert_eq!(super::check_i13(&state), Err(InvariantViolation::I13));
    }

    /// I13: 出力が 0 台で、列の持ち主だけが残っている。
    #[test]
    fn i13_rejects_an_owner_when_there_are_no_outputs() {
        let mut state = State::new(1);
        state.layout.workspaces = crate::domain::column::WorkspaceMode::Dynamic;
        if let Some(ws) = state.workspaces.first_mut() {
            ws.output = Some(OutputId::from_raw(1));
        }
        assert_eq!(super::check_i13(&state), Err(InvariantViolation::I13));
    }

    /// I13: 健全な `Dynamic` の State は通り、`Fixed` では何も見ない。
    #[test]
    fn i13_accepts_a_consistent_dynamic_state() -> Result<(), &'static str> {
        let mut state = dynamic_with_output().ok_or("setup")?;
        assert_eq!(super::check_i13(&state), Ok(()));
        state.layout.workspaces = crate::domain::column::WorkspaceMode::Fixed;
        state.active_output = None;
        assert_eq!(super::check_i13(&state), Ok(()));
        Ok(())
    }

    /// I13: 持ち主の無い列があるだけで（表示・並び・アクティブな出力・フォーカスは正しい）、違反になる。
    #[test]
    fn i13_rejects_a_workspace_without_an_owner() -> Result<(), &'static str> {
        let mut state = dynamic_with_output().ok_or("setup")?;
        // 持ち主なし（`None`）は並びの先頭に置いたままにして、並びの検査は通す。
        state
            .workspace_mut(WorkspaceId::from_raw(1))
            .ok_or("ws")?
            .output = None;
        assert_eq!(super::check_i13(&state), Err(InvariantViolation::I13));
        Ok(())
    }

    /// 表示中のワークスペースの持ち主が、その出力でない（ほかは正しい）。
    #[test]
    fn i13_rejects_a_shown_workspace_owned_by_another_output() -> Result<(), &'static str> {
        let mut state = dynamic_with_output().ok_or("setup")?;
        let other = OutputId::from_raw(2);
        state.outputs.insert(
            other,
            Output {
                id: other,
                name: OutputName::new("other"),
                geometry: area().ok_or("area")?,
                shown: WorkspaceId::from_raw(1),
                insets: Insets::default(),
            },
        );
        // ワークスペース 1 の持ち主は出力 1 のままなので、出力 2 の表示は持ち主と合わない。
        assert_eq!(super::check_i13(&state), Err(InvariantViolation::I13));
        Ok(())
    }

    /// 並びが持ち主の順でない（ほかは正しい）。
    #[test]
    fn i13_rejects_a_wrong_order() -> Result<(), &'static str> {
        let mut state = dynamic_with_output().ok_or("setup")?;
        let other = OutputId::from_raw(2);
        state.outputs.insert(
            other,
            Output {
                id: other,
                name: OutputName::new("other"),
                geometry: area().ok_or("area")?,
                shown: WorkspaceId::from_raw(3),
                insets: Insets::default(),
            },
        );
        let mut third = crate::domain::workspace::Workspace::new(WorkspaceId::from_raw(3));
        third.output = Some(other);
        // 出力 2 の列を、出力 1 の列の前に置く（並びが降順になる）。
        state.workspaces.insert(0, third);
        assert_eq!(super::check_i13(&state), Err(InvariantViolation::I13));
        Ok(())
    }

    /// アクティブな出力が無い（ほかは正しい）。
    #[test]
    fn i13_rejects_a_missing_active_output() -> Result<(), &'static str> {
        let mut state = dynamic_with_output().ok_or("setup")?;
        state.active_output = None;
        assert_eq!(super::check_i13(&state), Err(InvariantViolation::I13));
        Ok(())
    }

    /// フォーカスが、アクティブな出力の表示中のワークスペースにない（ほかは正しい）。
    #[test]
    fn i13_rejects_a_focus_outside_the_shown_workspace() -> Result<(), &'static str> {
        let mut state = dynamic_with_output().ok_or("setup")?;
        // 窓 1 は、表示中でない（ワークスペース 1 の）窓。
        let id = WindowId::from_raw(1);
        let w = Window::new(
            id,
            WindowMeta::default(),
            WindowState::Floating,
            WorkspaceId::from_raw(1),
        );
        state.windows.insert(id, w);
        state.focus = Some(id);
        assert_eq!(super::check_i13(&state), Err(InvariantViolation::I13));
        // 表示中のワークスペースの窓なら通る。
        if let Some(w) = state.windows.get_mut(&id) {
            w.workspace = WorkspaceId::from_raw(2);
        }
        assert_eq!(super::check_i13(&state), Ok(()));
        Ok(())
    }

    /// ジェスチャー。
    fn gesture() -> Option<crate::domain::workspace::ViewGesture> {
        Some(crate::domain::workspace::ViewGesture {
            start_view: 0,
            offset: 0,
            dnd: false,
        })
    }

    /// 出力 1・2 がワークスペース 1・2 を表示し、どちらにも列がある State（ジェスチャーなし）。
    fn two_shown_with_columns() -> Option<State> {
        let mut state = State::new(2);
        for raw in 1..=2u64 {
            let oid = OutputId::from_raw(raw);
            state.outputs.insert(
                oid,
                Output {
                    id: oid,
                    name: OutputName::new("test"),
                    geometry: area()?,
                    shown: WorkspaceId::from_raw(raw),
                    insets: Insets::default(),
                },
            );
            state
                .workspace_mut(WorkspaceId::from_raw(raw))?
                .columns
                .push(Column::single(
                    WindowId::from_raw(raw),
                    ColumnWidth::Proportion(Ratio::FULL),
                ));
        }
        Some(state)
    }

    /// I14: ジェスチャーが 0 個・1 個（表示中で列がある）なら通る。
    #[test]
    fn i14_accepts_at_most_one_valid_gesture() -> Result<(), &'static str> {
        let mut state = two_shown_with_columns().ok_or("setup")?;
        assert_eq!(super::check_i14(&state), Ok(()));
        state
            .workspace_mut(WorkspaceId::from_raw(1))
            .ok_or("ws")?
            .gesture = gesture();
        assert_eq!(super::check_i14(&state), Ok(()));
        Ok(())
    }

    /// I14: ジェスチャーが 2 個ある（それぞれは表示中で列がある）。
    #[test]
    fn i14_rejects_two_gestures() -> Result<(), &'static str> {
        let mut state = two_shown_with_columns().ok_or("setup")?;
        for ws in &mut state.workspaces {
            ws.gesture = gesture();
        }
        assert_eq!(super::check_i14(&state), Err(InvariantViolation::I14));
        Ok(())
    }

    /// I14: ジェスチャーが 1 個で、列が無い（表示中）。
    #[test]
    fn i14_rejects_a_gesture_on_a_workspace_without_columns() -> Result<(), &'static str> {
        let mut state = two_shown_with_columns().ok_or("setup")?;
        let ws = state.workspace_mut(WorkspaceId::from_raw(1)).ok_or("ws")?;
        ws.columns.clear();
        ws.gesture = gesture();
        assert_eq!(super::check_i14(&state), Err(InvariantViolation::I14));
        Ok(())
    }

    /// I14: ジェスチャーが 1 個で、列はあるが表示されていない。
    #[test]
    fn i14_rejects_a_gesture_on_a_hidden_workspace() -> Result<(), &'static str> {
        let mut state = two_shown_with_columns().ok_or("setup")?;
        state.outputs.remove(&OutputId::from_raw(1));
        state
            .workspace_mut(WorkspaceId::from_raw(1))
            .ok_or("ws")?
            .gesture = gesture();
        assert_eq!(super::check_i14(&state), Err(InvariantViolation::I14));
        Ok(())
    }
}
