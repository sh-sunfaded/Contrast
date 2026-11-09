//! ビューの連続スクロール（`docs/core-spec-p12.md` の 18 節、D85〜D92）。
//!
//! 具体例のテストと、参照モデル（D88 の吸着を文章どおりに書いたもの）との比較の proptest。実装のコードは見ていない。

use contrast_core::{
    Action, ColumnWidth, Command, CoreError, ExternalKey, LayoutMode, WorkspaceMode,
};
use proptest::prelude::*;

use crate::common::{
    FIRST_KEY, OUTPUT_WIDTH, TestCore, act, apply_err, apply_ok, core, core_in, focus_key, open,
    rect_of,
};

/// 出力のキー（common の出力は 1）。
const OUT: u64 = 1;

fn begin(c: &mut TestCore, dnd: bool) {
    apply_ok(
        c,
        Command::ViewGestureBegin {
            output: ExternalKey::new(OUT),
            dnd,
        },
    );
}

fn update(c: &mut TestCore, offset: i32) {
    apply_ok(c, Command::ViewGestureUpdate { offset });
}

fn end(c: &mut TestCore, projected_offset: i32) {
    apply_ok(c, Command::ViewGestureEnd { projected_offset });
}

/// 窓 `k` の列の左端（作業領域の左端から）から、今の view_x を出す: view_x = 列の x − 矩形の x。
fn view_x(c: &TestCore, k: u64, col_x: i32) -> i32 {
    col_x - rect_of(c, k).x()
}

/// `widths` の列を 1 枚ずつ開いた Scrolling の Core（窓のキーは 11 から、フォーカスは最後の列）。
fn with_columns(widths: &[i32]) -> TestCore {
    let mut c = core_in(WorkspaceMode::Fixed, LayoutMode::Scrolling, 0);
    for (i, w) in widths.iter().enumerate() {
        open(&mut c, FIRST_KEY + i as u64);
        act(
            &mut c,
            Action::SetColumnWidth(ColumnWidth::fixed(*w).expect("幅")),
        );
    }
    c
}

/// 列の左端（隙間なし）。
fn lefts(widths: &[i32]) -> Vec<i32> {
    let mut x = 0;
    widths
        .iter()
        .map(|w| {
            let at = x;
            x += w;
            at
        })
        .collect()
}

#[test]
fn begin_without_columns_does_nothing() {
    let mut c = core(LayoutMode::Scrolling, 0);
    let before = c.snapshot();
    begin(&mut c, false);
    update(&mut c, 100);
    end(&mut c, 100);
    assert_eq!(c.snapshot(), before);
}

#[test]
fn updates_without_a_gesture_do_nothing() {
    let mut c = with_columns(&[450, 450, 450]);
    let before = c.snapshot();
    update(&mut c, -300);
    end(&mut c, -300);
    assert_eq!(c.snapshot(), before);
}

#[test]
fn the_view_follows_the_offset_without_limits() {
    let mut c = with_columns(&[450, 450, 450]);
    // フォーカスは最後の列（x=900）。D39 で view_x = 450。
    assert_eq!(view_x(&c, 11, 0), 450);
    begin(&mut c, false);
    update(&mut c, -200);
    assert_eq!(view_x(&c, 11, 0), 250);
    update(&mut c, -900); // 開始からの合計。0 未満でもよい。
    assert_eq!(view_x(&c, 11, 0), -450);
    update(&mut c, 800);
    assert_eq!(view_x(&c, 11, 0), 1250);
    c.check_invariants()
        .expect("ジェスチャー中も不変条件（I11 を除く）");
}

#[test]
fn ending_snaps_to_the_nearest_column_edge_and_focuses_the_column() {
    let mut c = with_columns(&[450, 450, 450]);
    begin(&mut c, false);
    update(&mut c, -400); // view 50
    end(&mut c, -440); // 目標 10 → 吸着点 0（列 0）。左向きなので、見えている限り左の列（列 0）。
    assert_eq!(view_x(&c, 11, 0), 0);
    assert_eq!(focus_key(&c), Some(11));
}

#[test]
fn moving_right_focuses_the_furthest_fully_visible_column() {
    let mut c = with_columns(&[300, 300, 300, 300]);
    act(&mut c, Action::FocusColumnFirst); // view 0
    begin(&mut c, false);
    update(&mut c, 250);
    end(&mut c, 290); // 目標 290 → 吸着点 300（列 1 の左）。右向き: 列 1, 2, 3 の右端 600..1200 ≤ 300+900 → 列 3。
    assert_eq!(view_x(&c, 11, 0), 300);
    assert_eq!(focus_key(&c), Some(14));
}

#[test]
fn dnd_scrolling_is_limited_and_a_still_dnd_keeps_the_view() {
    let mut c = with_columns(&[450, 450, 450]);
    begin(&mut c, true);
    update(&mut c, 5000); // R = 1350 まで
    assert_eq!(view_x(&c, 11, 0), 1350);
    update(&mut c, -5000); // −W = −900 まで
    assert_eq!(view_x(&c, 11, 0), -900);
    let mut d = with_columns(&[450, 450, 450]);
    let before = d.snapshot();
    begin(&mut d, true);
    end(&mut d, 0);
    assert_eq!(d.snapshot(), before, "動かなかった DnD はビューを保つ");
}

#[test]
fn equal_mode_ignores_gestures_and_unknown_outputs_are_errors() {
    let mut c = core(LayoutMode::Equal, 2);
    let before = c.snapshot();
    begin(&mut c, false);
    update(&mut c, 100);
    end(&mut c, 100);
    assert_eq!(c.snapshot(), before);
    let err = apply_err(
        &mut c,
        Command::ViewGestureBegin {
            output: ExternalKey::new(99),
            dnd: false,
        },
    );
    assert_eq!(err, CoreError::UnknownKey);
}

#[test]
fn closing_every_window_ends_the_gesture() {
    let mut c = with_columns(&[450]);
    begin(&mut c, false);
    update(&mut c, 300);
    apply_ok(&mut c, Command::WindowUnmapped(ExternalKey::new(11)));
    open(&mut c, 12);
    // 新しい窓は、ジェスチャーの外で D39 に従う（view 0）。
    assert_eq!(view_x(&c, 12, 0), 0);
}

// ---------------------------------------------------------------------------
// 参照モデル
// ---------------------------------------------------------------------------

/// D39: 列（左端 `left`、幅 `width`）が見える最小のスクロールと、クランプ。
fn d39(view: i32, left: i32, width: i32, w: i32, total: i32) -> i32 {
    let mut v = view;
    if width >= w || left < v {
        v = left;
    } else if left + width > v + w {
        v = left + width - w;
    }
    v.clamp(0, (total - w).max(0))
}

/// D88 の吸着。`(終わりの view_x, フォーカスする列)`。
fn snap(widths: &[i32], w: i32, start: i32, current: i32, projected: i32) -> (i32, usize) {
    let xs = lefts(widths);
    let n = widths.len();
    let left_pt = |i: usize| xs[i];
    let right_pt = |i: usize| xs[i] + widths[i] - w;
    let l = left_pt(0);
    let r = right_pt(n - 1);
    let mut points: Vec<(i32, usize)> = vec![(l, 0), (r, n - 1)];
    for i in 0..n {
        for p in [left_pt(i), right_pt(i)] {
            if l < p && p < r {
                points.push((p, i));
            }
        }
    }
    points.sort_by_key(|p| p.0);
    let t = start + projected;
    let (s, mut c) = *points
        .iter()
        .min_by_key(|p| (p.0 - t).abs())
        .expect("候補がある");
    if t >= current {
        for j in c + 1..n {
            if s + w < xs[j] + widths[j] {
                break;
            }
            c = j;
        }
    } else {
        for j in (0..c).rev() {
            if xs[j] < s {
                break;
            }
            c = j;
        }
    }
    let total: i32 = widths.iter().sum();
    (d39(s, xs[c], widths[c], w, total), c)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    /// どんな列の幅・開始の列・移動量でも、終わりのビューとフォーカスがモデルと一致する。
    #[test]
    fn ending_matches_the_reference_model(
        widths in prop::collection::vec(100i32..1300, 1..6),
        focus_from_left in 0usize..6,
        moves in prop::collection::vec(-2000i32..2000, 0..4),
        projected_extra in -1500i32..1500,
    ) {
        let mut c = with_columns(&widths);
        // 開始の列（フォーカス）を選ぶ。
        act(&mut c, Action::FocusColumnFirst);
        for _ in 0..focus_from_left.min(widths.len() - 1) {
            act(&mut c, Action::FocusDirection(contrast_core::Direction::Right));
        }
        let start = view_x(&c, 11, 0);
        begin(&mut c, false);
        let mut current = start;
        for m in &moves {
            update(&mut c, *m);
            current = start + m;
            prop_assert_eq!(view_x(&c, 11, 0), current);
        }
        let last = moves.last().copied().unwrap_or(0);
        let projected = last + projected_extra;
        end(&mut c, projected);
        let (want_view, want_col) = snap(&widths, OUTPUT_WIDTH, start, current, projected);
        prop_assert_eq!(view_x(&c, 11, 0), want_view);
        prop_assert_eq!(focus_key(&c), Some(FIRST_KEY + want_col as u64));
        c.check_invariants().expect("不変条件");
    }
}
