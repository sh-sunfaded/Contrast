//! スクロール列レイアウト（LayoutMode::Scrolling）の仕様（core-spec 12 節、D35〜D48）。
//! 出力は幅 900 × 高さ 600。既定の列幅は 1/2（450）。プリセットは 1/3・1/2・2/3（300・450・600）。
//! 窓はキー 11 から。`scrolling(n)` のフォーカスは最後に開いた窓。
use contrast_core::{
    Action, ColumnWidth, Columns, Command, Core, CoreConfig, DefaultPolicy, Direction, LayoutMode,
    Ratio, WindowState,
};
use proptest::prelude::*;

use crate::common::{
    TestCore, apply_err, apply_noop, apply_ok, assert_effect_order, dialog, key, kinds, meta, out,
    output_added, rect, rect_of, setup, state_of, win, window_mapped, ws,
};

fn act(a: Action) -> Command {
    Command::Action(a)
}

fn ratio(n: u32, d: u32) -> Ratio {
    Ratio::new(n, d).expect("有効な比")
}

fn fixed(px: i32) -> ColumnWidth {
    ColumnWidth::fixed(px).expect("1 以上の幅")
}

/// Scrolling の Core。出力 1 台（幅 900）と、窓 n 枚（キー 11 から）。
fn scrolling(windows: u64) -> TestCore {
    let mut c = Core::new(
        CoreConfig::new(4).with_layout(LayoutMode::Scrolling),
        DefaultPolicy::new(),
        Columns::new(),
    );
    apply_ok(&mut c, output_added(1, 900));
    for i in 0..windows {
        apply_ok(&mut c, window_mapped(11 + i));
    }
    c
}

fn width_of(c: &TestCore, k: u64) -> i32 {
    rect_of(c, k).expect("矩形がある").width()
}

#[test]
fn the_first_window_opens_in_a_half_width_column() {
    let c = scrolling(1);
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 450, 600)));
    assert_eq!(c.view().focus(), Some(win(&c, 11)));
}

#[test]
fn new_windows_open_in_new_columns_to_the_right() {
    let c = scrolling(2);
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 450, 600)));
    assert_eq!(rect_of(&c, 12), Some(rect(450, 0, 450, 600)));
}

#[test]
fn the_view_scrolls_just_enough_to_show_the_focused_column() {
    let mut c = scrolling(2);
    let a = apply_ok(&mut c, window_mapped(13));
    // 列は x = 0・450・900。フォーカス列の右端 1350 が見えるよう、ビューは 450 だけスクロールする。
    assert_eq!(rect_of(&c, 11), Some(rect(-450, 0, 450, 600)));
    assert_eq!(rect_of(&c, 12), Some(rect(0, 0, 450, 600)));
    assert_eq!(rect_of(&c, 13), Some(rect(450, 0, 450, 600)));
    assert_eq!(
        kinds(&a.effects),
        vec![
            "Registered",
            "Configure",
            "Configure",
            "Configure",
            "SetFocus",
            "Damage"
        ]
    );
}

#[test]
fn focusing_a_column_scrolls_back_only_when_needed() {
    let mut c = scrolling(3);
    // 12 の列（450〜900）は、すでに見えている: スクロールしない。
    apply_ok(&mut c, act(Action::FocusDirection(Direction::Left)));
    assert_eq!(c.view().focus(), Some(win(&c, 12)));
    assert_eq!(rect_of(&c, 12), Some(rect(0, 0, 450, 600)));
    // 11 の列は左にはみ出している: ビューが 0 に戻る。
    apply_ok(&mut c, act(Action::FocusDirection(Direction::Left)));
    assert_eq!(c.view().focus(), Some(win(&c, 11)));
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 450, 600)));
    assert_eq!(rect_of(&c, 13), Some(rect(900, 0, 450, 600)));
}

#[test]
fn focusing_the_first_and_last_column() {
    let mut c = scrolling(3);
    apply_ok(&mut c, act(Action::FocusColumnFirst));
    assert_eq!(c.view().focus(), Some(win(&c, 11)));
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 450, 600)));
    apply_ok(&mut c, act(Action::FocusColumnLast));
    assert_eq!(c.view().focus(), Some(win(&c, 13)));
    assert_eq!(rect_of(&c, 13), Some(rect(450, 0, 450, 600)));
}

#[test]
fn focus_at_the_edges_changes_nothing() {
    let mut c = scrolling(3);
    apply_noop(&mut c, act(Action::FocusDirection(Direction::Right)));
    apply_noop(&mut c, act(Action::FocusColumnLast));
    apply_noop(&mut c, act(Action::FocusDirection(Direction::Up)));
    apply_noop(&mut c, act(Action::FocusDirection(Direction::Down)));
    apply_ok(&mut c, act(Action::FocusColumnFirst));
    apply_noop(&mut c, act(Action::FocusDirection(Direction::Left)));
    apply_noop(&mut c, act(Action::FocusColumnFirst));
}

#[test]
fn moving_a_column_swaps_it_with_its_neighbour() {
    let mut c = scrolling(3);
    let w13 = win(&c, 13);
    apply_ok(&mut c, act(Action::MoveWindow(Direction::Left)));
    // 列の並びは 11・13・12。フォーカスは 13 のまま。
    assert_eq!(rect_of(&c, 11), Some(rect(-450, 0, 450, 600)));
    assert_eq!(rect_of(&c, 13), Some(rect(0, 0, 450, 600)));
    assert_eq!(rect_of(&c, 12), Some(rect(450, 0, 450, 600)));
    assert_eq!(c.view().focus(), Some(w13));
}

#[test]
fn moving_a_column_to_the_first_or_last_place() {
    let mut c = scrolling(3);
    apply_ok(&mut c, act(Action::MoveColumnToFirst));
    // 並びは 13・11・12。13 の列が x = 0 になり、ビューは 0。
    assert_eq!(rect_of(&c, 13), Some(rect(0, 0, 450, 600)));
    assert_eq!(rect_of(&c, 11), Some(rect(450, 0, 450, 600)));
    assert_eq!(rect_of(&c, 12), Some(rect(900, 0, 450, 600)));
    apply_ok(&mut c, act(Action::MoveColumnToLast));
    assert_eq!(rect_of(&c, 11), Some(rect(-450, 0, 450, 600)));
    assert_eq!(rect_of(&c, 12), Some(rect(0, 0, 450, 600)));
    assert_eq!(rect_of(&c, 13), Some(rect(450, 0, 450, 600)));
}

#[test]
fn moving_a_column_at_the_edge_changes_nothing() {
    let mut c = scrolling(3);
    apply_noop(&mut c, act(Action::MoveWindow(Direction::Right)));
    apply_noop(&mut c, act(Action::MoveColumnToLast));
    apply_ok(&mut c, act(Action::FocusColumnFirst));
    apply_noop(&mut c, act(Action::MoveWindow(Direction::Left)));
    apply_noop(&mut c, act(Action::MoveColumnToFirst));
}

#[test]
fn set_column_width_resizes_only_the_focused_column() {
    let mut c = scrolling(2);
    apply_ok(
        &mut c,
        act(Action::SetColumnWidth(ColumnWidth::Proportion(ratio(1, 3)))),
    );
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 450, 600)));
    assert_eq!(rect_of(&c, 12), Some(rect(450, 0, 300, 600)));
    apply_ok(&mut c, act(Action::SetColumnWidth(fixed(200))));
    assert_eq!(rect_of(&c, 12), Some(rect(450, 0, 200, 600)));
}

#[test]
fn a_column_wider_than_the_work_area_is_aligned_to_the_left_edge() {
    let mut c = scrolling(2);
    apply_ok(&mut c, act(Action::SetColumnWidth(fixed(1200))));
    assert_eq!(rect_of(&c, 12), Some(rect(0, 0, 1200, 600)));
    assert_eq!(rect_of(&c, 11), Some(rect(-450, 0, 450, 600)));
}

#[test]
fn switching_preset_widths_goes_to_the_next_wider_preset_and_wraps() {
    let mut c = scrolling(1);
    assert_eq!(width_of(&c, 11), 450);
    apply_ok(&mut c, act(Action::SwitchPresetColumnWidth));
    assert_eq!(width_of(&c, 11), 600);
    apply_ok(&mut c, act(Action::SwitchPresetColumnWidth));
    assert_eq!(width_of(&c, 11), 300);
    apply_ok(&mut c, act(Action::SwitchPresetColumnWidth));
    assert_eq!(width_of(&c, 11), 450);
}

#[test]
fn maximizing_a_column_toggles_between_full_width_and_the_previous_width() {
    let mut c = scrolling(1);
    apply_ok(&mut c, act(Action::MaximizeColumn));
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 900, 600)));
    apply_ok(&mut c, act(Action::MaximizeColumn));
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 450, 600)));
}

#[test]
fn consuming_a_window_stacks_the_next_columns_window_below() {
    let mut c = scrolling(2);
    apply_ok(&mut c, act(Action::FocusColumnFirst));
    apply_ok(&mut c, act(Action::ConsumeWindowIntoColumn));
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 450, 300)));
    assert_eq!(rect_of(&c, 12), Some(rect(0, 300, 450, 300)));
    assert_eq!(c.view().focus(), Some(win(&c, 11)), "フォーカスは動かない");
}

#[test]
fn expelling_a_window_moves_it_to_a_new_column_on_the_right() {
    let mut c = scrolling(2);
    apply_ok(&mut c, act(Action::FocusColumnFirst));
    apply_ok(&mut c, act(Action::ConsumeWindowIntoColumn));
    apply_ok(&mut c, act(Action::FocusDirection(Direction::Down)));
    assert_eq!(c.view().focus(), Some(win(&c, 12)));
    apply_ok(&mut c, act(Action::ExpelWindowFromColumn));
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 450, 600)));
    assert_eq!(rect_of(&c, 12), Some(rect(450, 0, 450, 600)));
    assert_eq!(
        c.view().focus(),
        Some(win(&c, 12)),
        "フォーカスは窓についていく"
    );
}

#[test]
fn consuming_and_expelling_need_a_neighbour_or_a_stack() {
    let mut c = scrolling(2);
    apply_noop(&mut c, act(Action::ConsumeWindowIntoColumn)); // 右隣がない
    let mut c = scrolling(1);
    apply_noop(&mut c, act(Action::ExpelWindowFromColumn)); // 列に窓が 1 枚だけ
}

#[test]
fn windows_in_a_column_are_focused_and_moved_up_and_down() {
    let mut c = scrolling(2);
    apply_ok(&mut c, act(Action::FocusColumnFirst));
    apply_ok(&mut c, act(Action::ConsumeWindowIntoColumn)); // 列 = [11, 12]、フォーカス 11
    apply_noop(&mut c, act(Action::FocusDirection(Direction::Up)));
    apply_ok(&mut c, act(Action::FocusDirection(Direction::Down)));
    assert_eq!(c.view().focus(), Some(win(&c, 12)));
    apply_noop(&mut c, act(Action::FocusDirection(Direction::Down)));
    apply_ok(&mut c, act(Action::MoveWindow(Direction::Up)));
    // 列 = [12, 11]。フォーカスは 12 のまま。
    assert_eq!(rect_of(&c, 12), Some(rect(0, 0, 450, 300)));
    assert_eq!(rect_of(&c, 11), Some(rect(0, 300, 450, 300)));
    assert_eq!(c.view().focus(), Some(win(&c, 12)));
    apply_noop(&mut c, act(Action::MoveWindow(Direction::Up)));
}

#[test]
fn closing_the_last_window_of_a_column_removes_the_column() {
    let mut c = scrolling(3);
    apply_ok(&mut c, act(Action::FocusDirection(Direction::Left))); // 12
    apply_ok(&mut c, Command::WindowUnmapped(key(12)));
    // 右の列（13）にフォーカスが移る。総幅 900 で収まるので、ビューは 0。
    assert_eq!(c.view().focus(), Some(win(&c, 13)));
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 450, 600)));
    assert_eq!(rect_of(&c, 13), Some(rect(450, 0, 450, 600)));
}

#[test]
fn closing_the_focused_window_in_a_stack_focuses_the_one_below_or_above() {
    let mut c = scrolling(2);
    apply_ok(&mut c, act(Action::FocusColumnFirst));
    apply_ok(&mut c, act(Action::ConsumeWindowIntoColumn)); // [11, 12]、フォーカス 11
    apply_ok(&mut c, Command::WindowUnmapped(key(11)));
    assert_eq!(c.view().focus(), Some(win(&c, 12)));
    assert_eq!(rect_of(&c, 12), Some(rect(0, 0, 450, 600)));
}

#[test]
fn dialogs_float_and_do_not_open_a_column() {
    let mut c = scrolling(1);
    apply_ok(
        &mut c,
        Command::WindowMapped {
            key: key(12),
            meta: dialog(),
        },
    );
    assert_eq!(state_of(&c, 12), WindowState::Floating);
    assert_eq!(rect_of(&c, 12), Some(rect(225, 150, 450, 300)));
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 450, 600)));
}

#[test]
fn scrolling_only_actions_do_nothing_in_equal_mode() {
    let mut c = setup(900, 2);
    for a in [
        Action::FocusColumnFirst,
        Action::FocusColumnLast,
        Action::MoveColumnToFirst,
        Action::MoveColumnToLast,
        Action::ConsumeWindowIntoColumn,
        Action::ExpelWindowFromColumn,
        Action::SwitchPresetColumnWidth,
        Action::MaximizeColumn,
        Action::SetColumnWidth(ColumnWidth::Proportion(ratio(1, 3))),
    ] {
        apply_noop(&mut c, act(a));
    }
}

#[test]
fn ratios_must_be_positive_and_at_most_one() {
    assert!(Ratio::new(0, 3).is_none());
    assert!(Ratio::new(1, 0).is_none());
    assert!(Ratio::new(4, 3).is_none());
    let r = ratio(2, 3);
    assert_eq!((r.num(), r.den()), (2, 3));
    assert!(ColumnWidth::fixed(0).is_none());
    assert!(ColumnWidth::fixed(-5).is_none());
}

#[test]
fn an_unknown_window_keeps_scrolling_state_when_rejected() {
    let mut c = scrolling(3);
    apply_err(&mut c, Command::WindowUnmapped(key(99)));
    apply_err(
        &mut c,
        Command::WindowMapped {
            key: key(11),
            meta: meta(),
        },
    );
}

fn direction() -> impl Strategy<Value = Direction> {
    prop::sample::select(vec![
        Direction::Left,
        Direction::Right,
        Direction::Up,
        Direction::Down,
    ])
}

fn width_choice() -> impl Strategy<Value = ColumnWidth> {
    prop_oneof![
        prop::sample::select(vec![(1u32, 3u32), (1, 2), (2, 3), (1, 1)])
            .prop_map(|(n, d)| ColumnWidth::Proportion(ratio(n, d))),
        (50i32..=1500).prop_map(fixed),
    ]
}

fn op() -> impl Strategy<Value = Command> {
    prop_oneof![
        4 => (11u64..=16, any::<bool>()).prop_map(|(k, d)| Command::WindowMapped {
            key: key(k),
            meta: if d { dialog() } else { meta() },
        }),
        2 => (11u64..=16).prop_map(|k| Command::WindowUnmapped(key(k))),
        2 => (11u64..=16).prop_map(|k| Command::WindowFocusRequested(key(k))),
        3 => direction().prop_map(|d| act(Action::FocusDirection(d))),
        3 => direction().prop_map(|d| act(Action::MoveWindow(d))),
        1 => Just(act(Action::FocusColumnFirst)),
        1 => Just(act(Action::FocusColumnLast)),
        1 => Just(act(Action::MoveColumnToFirst)),
        1 => Just(act(Action::MoveColumnToLast)),
        2 => Just(act(Action::ConsumeWindowIntoColumn)),
        2 => Just(act(Action::ExpelWindowFromColumn)),
        2 => Just(act(Action::SwitchPresetColumnWidth)),
        1 => Just(act(Action::MaximizeColumn)),
        2 => width_choice().prop_map(|w| act(Action::SetColumnWidth(w))),
        1 => Just(act(Action::ToggleFloating)),
        1 => Just(act(Action::ToggleFullscreen)),
        1 => (0u8..=4).prop_map(|n| act(Action::SwitchWorkspace(ws(n)))),
        1 => (0u8..=4).prop_map(|n| act(Action::MoveToWorkspace(ws(n)))),
    ]
}

/// 表示中のワークスペースの Tiled の窓は、重ならず、縦は作業領域に収まる。
/// フォーカスが Tiled の窓にあれば、その列は見える（幅が作業領域以下なら全体が、広ければ左端が合う）。
fn assert_scrolling_geometry(c: &TestCore) {
    let view = c.view();
    let shown = view
        .shown_workspace(out(c, 1))
        .expect("出力は表示中のワークスペースを持つ");
    let mut tiled = Vec::new();
    for id in view.workspace_windows(shown).unwrap_or(&[]) {
        let w = view.window(*id).expect("窓");
        if w.state == WindowState::Tiled {
            tiled.push((*id, w.rect.expect("表示中の Tiled の窓には矩形がある")));
        }
    }
    for (i, (_, a)) in tiled.iter().enumerate() {
        assert!(
            a.y() >= 0 && a.bottom() <= 600,
            "縦が作業領域に収まる: {a:?}"
        );
        for (_, b) in tiled.iter().skip(i + 1) {
            assert!(a.intersect(b).is_none(), "重ならない: {a:?} {b:?}");
        }
    }
    if let Some(f) = view.focus() {
        let w = view.window(f).expect("窓");
        if w.state == WindowState::Tiled {
            let r = w.rect.expect("矩形");
            if r.width() <= 900 {
                assert!(
                    r.x() >= 0 && r.right() <= 900,
                    "フォーカス列が見える: {r:?}"
                );
            } else {
                assert_eq!(r.x(), 0, "広い列は左端を合わせる");
            }
        }
    }
}

proptest! {
    /// Scrolling で、どんな操作列でも、不変条件・Effect の順序・Err 時の状態不変・配置の性質が保たれる。
    #[test]
    fn scrolling_keeps_its_invariants(ops in prop::collection::vec(op(), 0..60)) {
        let mut c = Core::new(
            CoreConfig::new(4).with_layout(LayoutMode::Scrolling),
            DefaultPolicy::new(),
            Columns::new(),
        );
        c.apply(output_added(1, 900)).expect("出力を足す");
        for cmd in ops {
            let before = c.snapshot();
            match c.apply(cmd) {
                Ok(applied) => assert_effect_order(&applied.effects),
                Err(_) => prop_assert_eq!(c.snapshot(), before),
            }
            prop_assert_eq!(c.check_invariants(), Ok(()));
            assert_scrolling_geometry(&c);
        }
    }

    /// 同じ操作列は、同じ結果になる（決定性）。
    #[test]
    fn scrolling_is_deterministic(ops in prop::collection::vec(op(), 0..40)) {
        let make = || {
            let mut c = Core::new(
                CoreConfig::new(4).with_layout(LayoutMode::Scrolling),
                DefaultPolicy::new(),
                Columns::new(),
            );
            c.apply(output_added(1, 900)).expect("出力を足す");
            c
        };
        let (mut a, mut b) = (make(), make());
        for cmd in ops {
            prop_assert_eq!(a.apply(cmd.clone()), b.apply(cmd));
            prop_assert_eq!(a.snapshot(), b.snapshot());
        }
    }
}

#[test]
fn equal_mode_keeps_working_with_the_default_config() {
    // 既定の LayoutMode は Equal: 既存の仕様のまま、窓は出力を等分する。
    let c = setup(900, 3);
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 300, 600)));
}
