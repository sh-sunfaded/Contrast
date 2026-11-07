//! `ConsumeOrExpelWindowLeft`・`ConsumeOrExpelWindowRight`（core-spec 15 節、D57〜D61）。
//!
//! 具体例のテストと、参照モデルとの比較（proptest）。モデルは、spec の文章（D58・D59）をそのまま書いたもので、
//! 実装のコードは見ていない。

use contrast_core::{
    Action, ColumnWidth, Command, CoreError, Direction, LayoutMode, WindowHeight,
};
use proptest::prelude::*;

use crate::common::{
    FIRST_KEY, OUTPUT_WIDTH, TestCore, act, apply_err, columns, core, focus_key, rect_of, scrolling,
};

fn fixed(px: i32) -> ColumnWidth {
    ColumnWidth::fixed(px).expect("1 以上の幅")
}

/// 窓のキー `FIRST_KEY` から `n` 枚。
fn keys(n: u64) -> Vec<u64> {
    (0..n).map(|i| FIRST_KEY + i).collect()
}

fn left(c: &mut TestCore) {
    act(c, Action::ConsumeOrExpelWindowLeft);
}

fn right(c: &mut TestCore) {
    act(c, Action::ConsumeOrExpelWindowRight);
}

/// 列を `(窓のキーの並び, 幅)` の並びで比べるための短い書き方。
fn cols(c: &TestCore, n: u64) -> Vec<Vec<u64>> {
    columns(c, &keys(n)).into_iter().map(|(k, _)| k).collect()
}

#[test]
fn a_lone_window_is_consumed_into_the_left_column_at_the_bottom() {
    let mut c = scrolling(3);
    assert_eq!(cols(&c, 3), [vec![11], vec![12], vec![13]]);
    left(&mut c);
    assert_eq!(cols(&c, 3), [vec![11], vec![12, 13]]);
    // フォーカスは窓についていく。
    assert_eq!(focus_key(&c), Some(13));
}

#[test]
fn a_window_in_a_stacked_column_is_expelled_to_a_new_column_on_the_left() {
    let mut c = scrolling(3);
    left(&mut c); // [[11], [12, 13]]
    left(&mut c); // 13 を左隣の新しい列へ
    assert_eq!(cols(&c, 3), [vec![11], vec![13], vec![12]]);
    assert_eq!(focus_key(&c), Some(13));
}

#[test]
fn a_lone_window_is_consumed_into_the_right_column_at_the_bottom() {
    let mut c = scrolling(3);
    act(&mut c, Action::FocusDirection(Direction::Left));
    act(&mut c, Action::FocusDirection(Direction::Left));
    assert_eq!(focus_key(&c), Some(11));
    right(&mut c);
    // 右隣の列 [12] の最下段へ。元の列は消える。
    assert_eq!(cols(&c, 3), [vec![12, 11], vec![13]]);
    assert_eq!(focus_key(&c), Some(11));
}

#[test]
fn a_window_in_a_stacked_column_is_expelled_to_a_new_column_on_the_right() {
    let mut c = scrolling(3);
    left(&mut c); // [[11], [12, 13]]、フォーカスは 13
    right(&mut c); // 13 を右隣の新しい列へ
    assert_eq!(cols(&c, 3), [vec![11], vec![12], vec![13]]);
    assert_eq!(focus_key(&c), Some(13));
}

#[test]
fn at_the_edges_a_lone_window_stays_and_nothing_changes() {
    let mut c = scrolling(3);
    // 右端の列で、右へ。
    let before = c.snapshot();
    let applied = c.apply(Command::Action(Action::ConsumeOrExpelWindowRight));
    assert!(applied.expect("Ok").effects.is_empty());
    assert_eq!(c.snapshot(), before);
    // 左端の列で、左へ。
    act(&mut c, Action::FocusDirection(Direction::Left));
    act(&mut c, Action::FocusDirection(Direction::Left));
    let before = c.snapshot();
    let applied = c.apply(Command::Action(Action::ConsumeOrExpelWindowLeft));
    assert!(applied.expect("Ok").effects.is_empty());
    assert_eq!(c.snapshot(), before);
}

#[test]
fn an_expelled_window_gets_the_width_of_the_column_it_left() {
    let mut c = scrolling(2);
    left(&mut c); // [[11, 12]]、幅 450
    act(&mut c, Action::SetColumnWidth(fixed(600)));
    assert_eq!(rect_of(&c, 12).width(), 600);
    // 既定の幅（450）ではなく、元の列の幅（600）で出る。
    right(&mut c);
    assert_eq!(cols(&c, 2), [vec![11], vec![12]]);
    assert_eq!(rect_of(&c, 12).width(), 600);
    assert_eq!(rect_of(&c, 11).width(), 600);
    // 左へ出すときも同じ。
    left(&mut c); // 12 は列に 1 枚なので、左隣（11 の列）へ入る
    act(&mut c, Action::SetColumnWidth(fixed(300)));
    left(&mut c); // 2 枚の列から左へ出る
    assert_eq!(cols(&c, 2), [vec![12], vec![11]]);
    assert_eq!(rect_of(&c, 12).width(), 300);
}

#[test]
fn a_window_that_moves_to_another_column_loses_its_height() {
    let mut c = scrolling(3);
    left(&mut c); // [[11], [12, 13]]
    act(&mut c, Action::SetWindowHeight(WindowHeight::fixed(100).expect("1 以上")));
    assert_eq!(rect_of(&c, 13).height(), 100);
    // 2 枚の列から出ると、別の列の窓になる（D52: 高さは Auto に戻る）。
    left(&mut c);
    assert_eq!(cols(&c, 3), [vec![11], vec![13], vec![12]]);
    assert_eq!(rect_of(&c, 13).height(), 600);
    // 1 枚の窓が左の列へ入るときも、Auto で分け合う。
    left(&mut c);
    assert_eq!(cols(&c, 3), [vec![11, 13], vec![12]]);
    assert_eq!(rect_of(&c, 11).height(), 300);
    assert_eq!(rect_of(&c, 13).height(), 300);
}

#[test]
fn equal_mode_changes_nothing() {
    let mut c = core(LayoutMode::Equal, 3);
    let before = c.snapshot();
    for a in [
        Action::ConsumeOrExpelWindowLeft,
        Action::ConsumeOrExpelWindowRight,
    ] {
        let applied = c.apply(Command::Action(a)).expect("Ok");
        assert!(applied.effects.is_empty());
    }
    assert_eq!(c.snapshot(), before);
}

#[test]
fn a_floating_focus_window_changes_nothing() {
    let mut c = scrolling(3);
    act(&mut c, Action::ToggleFloating);
    let before = c.snapshot();
    for a in [
        Action::ConsumeOrExpelWindowLeft,
        Action::ConsumeOrExpelWindowRight,
    ] {
        let applied = c.apply(Command::Action(a)).expect("Ok");
        assert!(applied.effects.is_empty());
    }
    assert_eq!(c.snapshot(), before);
}

#[test]
fn without_a_focus_window_it_is_no_focused_window() {
    let mut c = scrolling(0);
    for a in [
        Action::ConsumeOrExpelWindowLeft,
        Action::ConsumeOrExpelWindowRight,
    ] {
        assert_eq!(
            apply_err(&mut c, Command::Action(a)),
            CoreError::NoFocusedWindow
        );
    }
}

// ---- 参照モデルとの比較 ----

/// 参照モデル: 列の並び（窓のキーの並びと、px の幅）とフォーカス窓。
#[derive(Debug, Clone, PartialEq)]
struct Model {
    cols: Vec<(Vec<u64>, i32)>,
    focus: u64,
}

impl Model {
    /// 窓 `n` 枚が、それぞれの列（既定の幅 450）に入った状態。フォーカスは最後の窓。
    fn new(n: u64) -> Self {
        Self {
            cols: keys(n).into_iter().map(|k| (vec![k], 450)).collect(),
            focus: FIRST_KEY + n - 1,
        }
    }

    /// フォーカス窓の（列, 列の中の位置）。
    fn pos(&self) -> (usize, usize) {
        for (c, (col, _)) in self.cols.iter().enumerate() {
            if let Some(r) = col.iter().position(|k| *k == self.focus) {
                return (c, r);
            }
        }
        panic!("フォーカス窓がモデルにない");
    }

    /// D58・D59。
    fn consume_or_expel(&mut self, to_left: bool) {
        let (c, r) = self.pos();
        if self.cols[c].0.len() == 1 {
            let target = if to_left {
                c.checked_sub(1)
            } else {
                (c + 1 < self.cols.len()).then_some(c + 1)
            };
            let Some(t) = target else { return };
            let (col, _) = self.cols.remove(c);
            // 右へのとき、元の列を消すと、右隣の列の位置が 1 つ前に詰まる。
            let t = if to_left { t } else { t - 1 };
            self.cols[t].0.push(col[0]);
        } else {
            let width = self.cols[c].1;
            let id = self.cols[c].0.remove(r);
            let at = if to_left { c } else { c + 1 };
            self.cols.insert(at, (vec![id], width));
        }
    }

    /// D42: 列の中の上下。端では何もしない。
    fn focus_vertical(&mut self, down: bool) {
        let (c, r) = self.pos();
        let col = &self.cols[c].0;
        let next = if down {
            (r + 1 < col.len()).then_some(r + 1)
        } else {
            r.checked_sub(1)
        };
        if let Some(n) = next {
            self.focus = col[n];
        }
    }

    fn set_width(&mut self, px: i32) {
        let (c, _) = self.pos();
        self.cols[c].1 = px;
    }
}

#[derive(Debug, Clone)]
enum Op {
    ConsumeOrExpelLeft,
    ConsumeOrExpelRight,
    Up,
    Down,
    Width(i32),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => Just(Op::ConsumeOrExpelLeft),
        3 => Just(Op::ConsumeOrExpelRight),
        1 => Just(Op::Up),
        1 => Just(Op::Down),
        1 => prop::sample::select(vec![300, 450, 600, 750]).prop_map(Op::Width),
    ]
}

/// Core の見た目を、モデルと同じ形（列の並び、幅、フォーカス）にする。
fn observe(c: &TestCore, n: u64) -> Model {
    Model {
        cols: columns(c, &keys(n)),
        focus: focus_key(c).expect("フォーカスがある"),
    }
}

proptest! {
    /// どんな操作の列でも、Core はモデルと同じ列・幅・フォーカスになり、不変条件が保たれる。
    #[test]
    fn matches_the_reference_model(n in 1u64..6, ops in prop::collection::vec(op(), 0..40)) {
        let mut c = scrolling(n);
        let mut m = Model::new(n);
        prop_assert_eq!(observe(&c, n), m.clone());
        for o in ops {
            match o {
                Op::ConsumeOrExpelLeft => {
                    act(&mut c, Action::ConsumeOrExpelWindowLeft);
                    m.consume_or_expel(true);
                }
                Op::ConsumeOrExpelRight => {
                    act(&mut c, Action::ConsumeOrExpelWindowRight);
                    m.consume_or_expel(false);
                }
                Op::Up => {
                    act(&mut c, Action::FocusDirection(Direction::Up));
                    m.focus_vertical(false);
                }
                Op::Down => {
                    act(&mut c, Action::FocusDirection(Direction::Down));
                    m.focus_vertical(true);
                }
                Op::Width(px) => {
                    act(&mut c, Action::SetColumnWidth(fixed(px)));
                    m.set_width(px);
                }
            }
            prop_assert_eq!(observe(&c, n), m.clone());
        }
    }

    /// 窓の数は変わらず、列は空にならず、窓は重複しない。
    #[test]
    fn keeps_every_window_exactly_once(n in 1u64..6, lefts in prop::collection::vec(any::<bool>(), 0..30)) {
        let mut c = scrolling(n);
        for to_left in lefts {
            let a = if to_left {
                Action::ConsumeOrExpelWindowLeft
            } else {
                Action::ConsumeOrExpelWindowRight
            };
            act(&mut c, a);
            let all: Vec<u64> = columns(&c, &keys(n)).into_iter().flat_map(|(k, _)| k).collect();
            let mut sorted = all.clone();
            sorted.sort_unstable();
            prop_assert_eq!(sorted, keys(n));
        }
    }
}

/// 出力の幅（モデルの前提: 既定の列幅 450 は 900 の 1/2）。
#[test]
fn the_model_assumes_a_half_width_default() {
    assert_eq!(OUTPUT_WIDTH / 2, 450);
    let c = scrolling(1);
    assert_eq!(rect_of(&c, FIRST_KEY).width(), 450);
}
