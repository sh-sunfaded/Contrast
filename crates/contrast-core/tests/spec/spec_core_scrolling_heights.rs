//! スクロール列レイアウトの N3b: 窓の高さと center-column（core-spec 14 節、D49〜D56）。
//! 出力は幅 900 × 高さ 600。列の幅は 450。窓はキー 11 から。`stacked(n)` は n 枚を 1 つの列に積み、フォーカスは 11。
use std::collections::BTreeMap;

use contrast_core::{
    Action, Columns, Command, Core, CoreConfig, DefaultPolicy, Direction, LayoutMode, Ratio, Rect,
    WindowHeight, WindowState,
};
use proptest::prelude::*;

use crate::common::{
    TestCore, apply_noop, apply_ok, assert_effect_order, dialog, key, meta, out, output_added,
    rect, rect_of, setup, window_mapped,
};

fn act(a: Action) -> Command {
    Command::Action(a)
}

fn ratio(n: u32, d: u32) -> Ratio {
    Ratio::new(n, d).expect("有効な比")
}

fn proportion(n: u32, d: u32) -> WindowHeight {
    WindowHeight::Proportion(ratio(n, d))
}

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

fn stacked(windows: u64) -> TestCore {
    let mut c = scrolling(windows);
    apply_ok(&mut c, act(Action::FocusColumnFirst));
    for _ in 1..windows {
        apply_ok(&mut c, act(Action::ConsumeWindowIntoColumn));
    }
    c
}

fn height_of(c: &TestCore, k: u64) -> i32 {
    rect_of(c, k).expect("矩形がある").height()
}

#[test]
fn a_proportional_height_is_taken_and_auto_windows_share_the_rest() {
    let mut c = stacked(2);
    apply_ok(&mut c, act(Action::SetWindowHeight(proportion(1, 3))));
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 450, 200)));
    assert_eq!(rect_of(&c, 12), Some(rect(0, 200, 450, 400)));
}

#[test]
fn a_fixed_height_leaves_at_least_one_pixel_for_each_following_window() {
    let mut c = stacked(3);
    apply_ok(
        &mut c,
        act(Action::SetWindowHeight(
            WindowHeight::fixed(580).expect("1 以上"),
        )),
    );
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 450, 580)));
    assert_eq!(rect_of(&c, 12), Some(rect(0, 580, 450, 10)));
    assert_eq!(rect_of(&c, 13), Some(rect(0, 590, 450, 10)));
    apply_ok(
        &mut c,
        act(Action::SetWindowHeight(
            WindowHeight::fixed(599).expect("1 以上"),
        )),
    );
    assert_eq!(height_of(&c, 11), 598);
    assert_eq!(height_of(&c, 12), 1);
    assert_eq!(height_of(&c, 13), 1);
}

#[test]
fn when_no_window_is_auto_the_last_one_takes_the_rest() {
    let mut c = stacked(3);
    for _ in 0..2 {
        apply_ok(&mut c, act(Action::SetWindowHeight(proportion(2, 3))));
        apply_ok(&mut c, act(Action::FocusDirection(Direction::Down)));
    }
    apply_ok(&mut c, act(Action::SetWindowHeight(proportion(2, 3))));
    // 要求は 400・400・400。1 枚目は 400、2 枚目は残り 200 から後ろの 1 px を引いた 199、3 枚目は残りの 1。
    assert_eq!(height_of(&c, 11), 400);
    assert_eq!(height_of(&c, 12), 199);
    assert_eq!(height_of(&c, 13), 1);
}

#[test]
fn switching_preset_heights_goes_to_the_next_taller_preset_and_wraps() {
    let mut c = stacked(2);
    // 今の高さは 300（Auto）。プリセットは 200・300・400。
    apply_ok(&mut c, act(Action::SwitchPresetWindowHeight));
    assert_eq!((height_of(&c, 11), height_of(&c, 12)), (400, 200));
    apply_ok(&mut c, act(Action::SwitchPresetWindowHeight));
    assert_eq!((height_of(&c, 11), height_of(&c, 12)), (200, 400));
    apply_ok(&mut c, act(Action::SwitchPresetWindowHeight));
    assert_eq!((height_of(&c, 11), height_of(&c, 12)), (300, 300));
}

#[test]
fn resetting_the_height_makes_the_window_auto_again() {
    let mut c = stacked(2);
    apply_ok(&mut c, act(Action::SetWindowHeight(proportion(1, 3))));
    apply_ok(&mut c, act(Action::ResetWindowHeight));
    assert_eq!((height_of(&c, 11), height_of(&c, 12)), (300, 300));
    apply_ok(&mut c, act(Action::SetWindowHeight(proportion(1, 3))));
    apply_ok(&mut c, act(Action::SetWindowHeight(WindowHeight::Auto)));
    assert_eq!((height_of(&c, 11), height_of(&c, 12)), (300, 300));
}

#[test]
fn a_window_that_changes_column_gets_an_auto_height() {
    let mut c = stacked(2);
    apply_ok(&mut c, act(Action::SetWindowHeight(proportion(1, 3))));
    apply_ok(&mut c, act(Action::ExpelWindowFromColumn)); // 11 は、右隣の新しい列へ
    apply_ok(&mut c, act(Action::FocusDirection(Direction::Left))); // 列 [12]
    apply_ok(&mut c, act(Action::ConsumeWindowIntoColumn)); // 11 が [12] の最下段へ
    assert_eq!(rect_of(&c, 12), Some(rect(0, 0, 450, 300)));
    assert_eq!(rect_of(&c, 11), Some(rect(0, 300, 450, 300)));
}

#[test]
fn centering_a_column_scrolls_the_view_when_it_can() {
    let mut c = scrolling(3); // 列は x = 0・450・900。ビューは 450。
    apply_ok(&mut c, act(Action::FocusDirection(Direction::Left))); // 12
    apply_ok(&mut c, act(Action::CenterColumn));
    // 12 の列（450〜900、中心 675）を作業領域の中央（450）に置く: ビューは 225。
    assert_eq!(rect_of(&c, 11), Some(rect(-225, 0, 450, 600)));
    assert_eq!(rect_of(&c, 12), Some(rect(225, 0, 450, 600)));
    assert_eq!(rect_of(&c, 13), Some(rect(675, 0, 450, 600)));
}

#[test]
fn centering_stops_at_the_ends_of_the_content() {
    let mut c = scrolling(3); // フォーカス 13（最後の列）。ビューはすでに右端の 450。
    apply_noop(&mut c, act(Action::CenterColumn));
    apply_ok(&mut c, act(Action::FocusColumnFirst));
    apply_noop(&mut c, act(Action::CenterColumn));
}

#[test]
fn window_height_actions_and_centering_do_nothing_in_equal_mode() {
    let mut c = setup(900, 2);
    for a in [
        Action::CenterColumn,
        Action::SwitchPresetWindowHeight,
        Action::ResetWindowHeight,
        Action::SetWindowHeight(proportion(1, 3)),
    ] {
        apply_noop(&mut c, act(a));
    }
}

fn height_choice() -> impl Strategy<Value = WindowHeight> {
    prop_oneof![
        prop::sample::select(vec![(1u32, 3u32), (1, 2), (2, 3)])
            .prop_map(|(n, d)| proportion(n, d)),
        (1i32..=700).prop_map(|px| WindowHeight::fixed(px).expect("1 以上")),
        Just(WindowHeight::Auto),
    ]
}

fn direction() -> impl Strategy<Value = Direction> {
    prop::sample::select(vec![
        Direction::Left,
        Direction::Right,
        Direction::Up,
        Direction::Down,
    ])
}

fn op() -> impl Strategy<Value = Command> {
    prop_oneof![
        4 => (11u64..=16, any::<bool>()).prop_map(|(k, d)| Command::WindowMapped {
            key: key(k),
            meta: if d { dialog() } else { meta() },
        }),
        2 => (11u64..=16).prop_map(|k| Command::WindowUnmapped(key(k))),
        3 => direction().prop_map(|d| act(Action::FocusDirection(d))),
        2 => direction().prop_map(|d| act(Action::MoveWindow(d))),
        3 => Just(act(Action::ConsumeWindowIntoColumn)),
        2 => Just(act(Action::ExpelWindowFromColumn)),
        3 => height_choice().prop_map(|h| act(Action::SetWindowHeight(h))),
        2 => Just(act(Action::SwitchPresetWindowHeight)),
        2 => Just(act(Action::ResetWindowHeight)),
        2 => Just(act(Action::CenterColumn)),
        1 => Just(act(Action::MaximizeColumn)),
        1 => Just(act(Action::SwitchPresetColumnWidth)),
        1 => Just(act(Action::ToggleFloating)),
        1 => Just(act(Action::ToggleFullscreen)),
    ]
}

/// 表示中のワークスペースの各列で、Tiled の窓の高さが、作業領域の高さを隙間なく埋める。
fn assert_columns_tile_the_height(c: &TestCore) {
    let view = c.view();
    let shown = view
        .shown_workspace(out(c, 1))
        .expect("表示中のワークスペース");
    let mut by_x: BTreeMap<i32, Vec<Rect>> = BTreeMap::new();
    for id in view.workspace_windows(shown).unwrap_or(&[]) {
        let w = view.window(*id).expect("窓");
        if w.state == WindowState::Tiled {
            let r = w.rect.expect("表示中の Tiled の窓には矩形がある");
            by_x.entry(r.x()).or_default().push(r);
        }
    }
    for rects in by_x.values_mut() {
        rects.sort_by_key(|r| r.y());
        let mut y = 0;
        for r in rects.iter() {
            assert_eq!(r.y(), y, "隙間なく並ぶ: {rects:?}");
            assert!(r.height() >= 1, "高さは 1 以上: {rects:?}");
            y = r.bottom();
        }
        assert_eq!(y, 600, "列の高さを埋める: {rects:?}");
    }
}

proptest! {
    /// 高さの操作・積み・センタリングをどう混ぜても、不変条件が保たれ、列が高さを埋め、Err のとき状態が変わらない。
    #[test]
    fn heights_keep_the_columns_tiled(ops in prop::collection::vec(op(), 0..60)) {
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
            assert_columns_tile_the_height(&c);
        }
    }
}
