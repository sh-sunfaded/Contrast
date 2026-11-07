//! 動的なワークスペース（core-spec 16 節、D62〜D71）。
//!
//! 具体例のテストと、参照モデルとの比較（proptest）。モデルは spec の文章（D64〜D68）をそのまま書いたもので、実装のコードは見ていない。

use contrast_core::{
    Action, Command, CoreError, Direction, Effect, ExternalKey, IdKind, LayoutMode, WorkspaceIndex,
    WorkspaceMode,
};
use proptest::prelude::*;

use crate::common::{
    FIRST_KEY, TestCore, act, apply_err, apply_ok, close, columns, core_in, dynamic, focus_key,
    open, rect_of, shown, workspaces,
};

fn ws(n: u8) -> WorkspaceIndex {
    WorkspaceIndex::new(n)
}

/// `(id, 窓の並び)` を短く書く。
fn w(id: u64, keys: &[u64]) -> (u64, Vec<u64>) {
    (id, keys.to_vec())
}

#[test]
fn it_starts_with_one_empty_workspace() {
    let c = dynamic(0);
    assert_eq!(workspaces(&c), [w(1, &[])]);
    assert_eq!(c.view().workspace_count(), 1);
    assert_eq!(shown(&c), 0);
}

#[test]
fn a_window_in_the_last_workspace_adds_a_new_empty_one() {
    let mut c = dynamic(0);
    open(&mut c, 11);
    assert_eq!(workspaces(&c), [w(1, &[11]), w(2, &[])]);
    assert_eq!(shown(&c), 0);
    // 次の窓も、表示中のワークスペースに入る。末尾は空のまま。
    open(&mut c, 12);
    assert_eq!(workspaces(&c), [w(1, &[11, 12]), w(2, &[])]);
}

#[test]
fn switching_beyond_the_end_goes_to_the_last_empty_workspace() {
    let mut c = dynamic(2); // [{11,12}, 空]
    act(&mut c, Action::SwitchWorkspace(ws(7)));
    assert_eq!(shown(&c), 1);
    assert_eq!(focus_key(&c), None);
    // 空の末尾のワークスペースに窓を開くと、その下に新しい空ができる。
    open(&mut c, 13);
    assert_eq!(workspaces(&c), [w(1, &[11, 12]), w(2, &[13]), w(3, &[])]);
    assert_eq!(shown(&c), 1);
}

#[test]
fn a_workspace_left_empty_is_removed_when_you_leave_it() {
    let mut c = dynamic(0);
    open(&mut c, 11);
    act(&mut c, Action::SwitchWorkspace(ws(1))); // 空の末尾へ
    open(&mut c, 12); // [{11}, {12}, 空]
    assert_eq!(workspaces(&c), [w(1, &[11]), w(2, &[12]), w(3, &[])]);
    // 窓を閉じても、表示中のあいだは残る（D65）。
    close(&mut c, 12);
    assert_eq!(workspaces(&c), [w(1, &[11]), w(2, &[]), w(3, &[])]);
    assert_eq!(shown(&c), 1);
    // 離れると消える。id は位置から独立（残ったものの id は変わらない）。
    act(&mut c, Action::SwitchWorkspace(ws(0)));
    assert_eq!(workspaces(&c), [w(1, &[11]), w(3, &[])]);
    assert_eq!(shown(&c), 0);
    // 消えた id は再利用しない。
    act(&mut c, Action::SwitchWorkspace(ws(1)));
    open(&mut c, 13);
    assert_eq!(workspaces(&c), [w(1, &[11]), w(3, &[13]), w(4, &[])]);
}

#[test]
fn closing_a_window_in_a_hidden_workspace_does_not_remove_it_until_the_next_switch() {
    let mut c = dynamic(0);
    open(&mut c, 11);
    act(&mut c, Action::SwitchWorkspace(ws(1)));
    open(&mut c, 12);
    act(&mut c, Action::SwitchWorkspace(ws(0)));
    close(&mut c, 12); // 裏のワークスペースの窓が閉じる
    assert_eq!(workspaces(&c), [w(1, &[11]), w(2, &[]), w(3, &[])]);
    // 「実際に切り替わらない」切り替え（今と同じ位置）では、整理しない。
    act(&mut c, Action::SwitchWorkspace(ws(0)));
    assert_eq!(workspaces(&c).len(), 3);
    // 切り替えると、整理される。
    act(&mut c, Action::SwitchWorkspace(ws(2)));
    assert_eq!(workspaces(&c), [w(1, &[11]), w(3, &[])]);
    assert_eq!(shown(&c), 1);
}

#[test]
fn moving_the_column_to_another_workspace_follows_it_and_cleans_up() {
    let mut c = dynamic(1); // [{11}, 空]
    act(&mut c, Action::MoveColumnToWorkspace(ws(5))); // 末尾に丸める
    // 元のワークスペース（空になった）は整理で消え、移した先の下に新しい空ができる。
    assert_eq!(workspaces(&c), [w(2, &[11]), w(3, &[])]);
    assert_eq!(shown(&c), 0);
    assert_eq!(focus_key(&c), Some(11));
}

#[test]
fn the_whole_column_moves_and_keeps_its_width_and_stacking() {
    let mut c = dynamic(3); // 11, 12, 13 が別の列
    act(&mut c, Action::ConsumeOrExpelWindowLeft); // 13 が 12 の列へ
    act(
        &mut c,
        Action::SetColumnWidth(contrast_core::ColumnWidth::fixed(600).expect("幅")),
    );
    assert_eq!(columns(&c, &[11, 12, 13]).len(), 2);
    act(&mut c, Action::MoveColumnToWorkspace(ws(1)));
    // 列の 2 枚が、そろって移る。11 は元のワークスペースに残る。
    assert_eq!(workspaces(&c), [w(1, &[11]), w(2, &[12, 13]), w(3, &[])]);
    assert_eq!(shown(&c), 1);
    let cols = columns(&c, &[12, 13]);
    assert_eq!(cols, [(vec![12, 13], 600)]);
    assert_eq!(rect_of(&c, 12).height() + rect_of(&c, 13).height(), 600);
    assert_eq!(focus_key(&c), Some(13));
}

#[test]
fn a_moved_column_goes_to_the_right_of_the_last_focused_column() {
    let mut c = dynamic(0);
    open(&mut c, 11);
    open(&mut c, 12); // 列: 11 12
    act(&mut c, Action::SwitchWorkspace(ws(1)));
    open(&mut c, 13);
    open(&mut c, 14); // 2 つ目のワークスペース: 13 14、最後にフォーカスしたのは 14
    act(&mut c, Action::FocusDirection(Direction::Left)); // 13 にフォーカス
    act(&mut c, Action::SwitchWorkspace(ws(0)));
    act(&mut c, Action::MoveColumnToWorkspace(ws(1))); // 12（フォーカス）を 2 つ目へ
    // 2 つ目では、最後にフォーカスした列は 13。その右隣に入る。
    assert_eq!(
        columns(&c, &[13, 12, 14])
            .into_iter()
            .map(|(k, _)| k)
            .collect::<Vec<_>>(),
        [vec![13], vec![12], vec![14]]
    );
}

#[test]
fn a_floating_window_moves_alone() {
    let mut c = dynamic(2);
    act(&mut c, Action::ToggleFloating); // 12 をフロートに
    act(&mut c, Action::MoveColumnToWorkspace(ws(1)));
    assert_eq!(workspaces(&c), [w(1, &[11]), w(2, &[12]), w(3, &[])]);
    assert_eq!(shown(&c), 1);
}

#[test]
fn move_column_without_a_focus_window_is_no_focused_window() {
    let mut c = dynamic(0);
    assert_eq!(
        apply_err(
            &mut c,
            Command::Action(Action::MoveColumnToWorkspace(ws(0)))
        ),
        CoreError::NoFocusedWindow
    );
}

#[test]
fn moving_to_the_current_workspace_changes_nothing() {
    let mut c = dynamic(1);
    let before = c.snapshot();
    let applied = c
        .apply(Command::Action(Action::MoveColumnToWorkspace(ws(0))))
        .expect("Ok");
    assert!(applied.effects.is_empty());
    assert_eq!(c.snapshot(), before);
}

#[test]
fn moving_a_workspace_down_swaps_it_with_the_next() {
    let mut c = dynamic(0);
    open(&mut c, 11);
    act(&mut c, Action::SwitchWorkspace(ws(1)));
    open(&mut c, 12);
    act(&mut c, Action::SwitchWorkspace(ws(0))); // [{11}(表示), {12}, 空]
    act(&mut c, Action::MoveWorkspaceDown);
    assert_eq!(workspaces(&c), [w(2, &[12]), w(1, &[11]), w(3, &[])]);
    assert_eq!(shown(&c), 1);
    assert_eq!(focus_key(&c), Some(11));
    // 末尾まで動かすと、間にできた空は整理され、下に新しい空ができる（niri の順序と同じ結果）。
    act(&mut c, Action::MoveWorkspaceDown);
    assert_eq!(workspaces(&c), [w(2, &[12]), w(1, &[11]), w(4, &[])]);
    assert_eq!(shown(&c), 1);
}

#[test]
fn moving_a_workspace_up_swaps_it_with_the_previous_and_stops_at_the_top() {
    let mut c = dynamic(0);
    open(&mut c, 11);
    act(&mut c, Action::SwitchWorkspace(ws(1)));
    open(&mut c, 12); // [{11}, {12}(表示), 空]
    act(&mut c, Action::MoveWorkspaceUp);
    assert_eq!(workspaces(&c), [w(2, &[12]), w(1, &[11]), w(3, &[])]);
    assert_eq!(shown(&c), 0);
    // 先頭では何もしない。
    let before = c.snapshot();
    let applied = c
        .apply(Command::Action(Action::MoveWorkspaceUp))
        .expect("Ok");
    assert!(applied.effects.is_empty());
    assert_eq!(c.snapshot(), before);
}

#[test]
fn moving_the_empty_last_workspace_up_keeps_it_and_adds_a_new_last() {
    let mut c = dynamic(1); // [{11}, 空]
    act(&mut c, Action::SwitchWorkspace(ws(1))); // 空の末尾が表示中
    act(&mut c, Action::MoveWorkspaceUp);
    assert_eq!(workspaces(&c), [w(2, &[]), w(1, &[11]), w(3, &[])]);
    assert_eq!(shown(&c), 0);
}

#[test]
fn moving_the_last_workspace_down_does_nothing() {
    let mut c = dynamic(1);
    act(&mut c, Action::SwitchWorkspace(ws(1)));
    let before = c.snapshot();
    let applied = c
        .apply(Command::Action(Action::MoveWorkspaceDown))
        .expect("Ok");
    assert!(applied.effects.is_empty());
    assert_eq!(c.snapshot(), before);
}

#[test]
fn show_workspace_is_emitted_when_the_position_stays_the_same() {
    // [空(表示), {11}, 空]: 11 へ切り替えると空が消え、11 は位置 0 になる（位置は 0 → 0）。
    let mut c = dynamic(0);
    open(&mut c, 11); // [{11}, 空]
    act(&mut c, Action::SwitchWorkspace(ws(1)));
    act(&mut c, Action::MoveWorkspaceUp); // [空, {11}, 空'] 表示は先頭の空
    assert_eq!(shown(&c), 0);
    assert_eq!(workspaces(&c).len(), 3);
    let applied = c
        .apply(Command::Action(Action::SwitchWorkspace(ws(1))))
        .expect("Ok");
    assert_eq!(shown(&c), 0, "先頭の空が消えて、11 は位置 0 になる");
    let shown_id = c.view().workspace_id(ws(0)).expect("id");
    assert!(
        applied.effects.iter().any(
            |e| matches!(e, Effect::ShowWorkspace { workspace, .. } if *workspace == shown_id)
        ),
        "位置が 0 のままでも、表示するワークスペースが変わったので ShowWorkspace が出る: {:?}",
        applied.effects
    );
}

#[test]
fn fixed_mode_is_unchanged_and_supports_the_new_actions_where_defined() {
    let mut c = core_in(WorkspaceMode::Fixed, LayoutMode::Scrolling, 2);
    assert_eq!(c.view().workspace_count(), 9);
    // 範囲外は従来どおりエラー。
    assert_eq!(
        apply_err(&mut c, Command::Action(Action::SwitchWorkspace(ws(9)))),
        CoreError::WorkspaceIndexOutOfRange
    );
    assert_eq!(
        apply_err(
            &mut c,
            Command::Action(Action::MoveColumnToWorkspace(ws(9)))
        ),
        CoreError::WorkspaceIndexOutOfRange
    );
    // 列ごとの移動は Fixed でも使える。
    act(&mut c, Action::MoveColumnToWorkspace(ws(3)));
    assert_eq!(shown(&c), 3);
    assert_eq!(c.view().workspace_count(), 9);
    // ワークスペースの入れ替えは Dynamic だけ。Fixed では状態不変で Ok。
    let before = c.snapshot();
    for a in [Action::MoveWorkspaceDown, Action::MoveWorkspaceUp] {
        let applied = c.apply(Command::Action(a)).expect("Ok");
        assert!(applied.effects.is_empty());
    }
    assert_eq!(c.snapshot(), before);
    // Fixed の id は位置 + 1。
    for i in 0..9 {
        assert_eq!(
            c.view().workspace_id(ws(i)).expect("id").raw(),
            u64::from(i) + 1
        );
    }
}

#[test]
fn move_column_is_a_no_op_in_equal_mode() {
    let mut c = core_in(WorkspaceMode::Dynamic, LayoutMode::Equal, 2);
    let before = c.snapshot();
    let applied = c
        .apply(Command::Action(Action::MoveColumnToWorkspace(ws(1))))
        .expect("Ok");
    assert!(applied.effects.is_empty());
    assert_eq!(c.snapshot(), before);
}

// ---- 参照モデルとの比較 ----

/// 参照モデルのワークスペース 1 つ。
#[derive(Debug, Clone, PartialEq)]
struct Ws {
    id: u64,
    windows: Vec<u64>,
    /// フォーカスの履歴（最新が末尾）。
    hist: Vec<u64>,
}

/// 参照モデル（`Dynamic`、`Scrolling`、窓はすべて別の列）。
#[derive(Debug, Clone)]
struct Model {
    wss: Vec<Ws>,
    active: usize,
    focus: Option<u64>,
    next_id: u64,
    next_key: u64,
}

impl Model {
    fn new() -> Self {
        Self {
            wss: vec![Ws {
                id: 1,
                windows: Vec::new(),
                hist: Vec::new(),
            }],
            active: 0,
            focus: None,
            next_id: 2,
            next_key: FIRST_KEY,
        }
    }

    fn record(&mut self, ws: usize, k: u64) {
        let w = &mut self.wss[ws];
        w.hist.retain(|h| *h != k);
        w.hist.push(k);
    }

    /// D64: 末尾が空でなければ、新しい空を足す。
    fn trailing(&mut self) {
        if !self.wss.last().expect("1 個以上").windows.is_empty() {
            self.wss.push(Ws {
                id: self.next_id,
                windows: Vec::new(),
                hist: Vec::new(),
            });
            self.next_id += 1;
        }
    }

    /// D65: 末尾以外で、窓がなく、表示されていないものを消す。
    fn cleanup(&mut self) {
        let last = self.wss.len() - 1;
        for i in (0..last).rev() {
            if i != self.active && self.wss[i].windows.is_empty() {
                self.wss.remove(i);
                if self.active > i {
                    self.active -= 1;
                }
            }
        }
    }

    fn open(&mut self) {
        let k = self.next_key;
        self.next_key += 1;
        let a = self.active;
        self.wss[a].windows.push(k);
        self.record(a, k);
        self.focus = Some(k);
        self.trailing();
    }

    /// フォーカス窓以外の窓を閉じる。D65: 整理はしない。
    fn close(&mut self, k: u64) {
        for w in &mut self.wss {
            w.windows.retain(|x| *x != k);
            w.hist.retain(|x| *x != k);
        }
    }

    fn preferred(&self, ws: usize) -> Option<u64> {
        let w = &self.wss[ws];
        w.hist.last().or_else(|| w.windows.first()).copied()
    }

    fn switch(&mut self, n: u8) {
        let t = usize::from(n).min(self.wss.len() - 1);
        if t == self.active {
            return;
        }
        self.active = t;
        self.focus = self.preferred(t);
        if let Some(f) = self.focus {
            self.record(t, f);
        }
        self.cleanup();
        self.trailing();
    }

    fn move_column(&mut self, n: u8) {
        let Some(f) = self.focus else { return };
        let t = usize::from(n).min(self.wss.len() - 1);
        let a = self.active;
        if t == a {
            return;
        }
        self.wss[a].windows.retain(|x| *x != f);
        self.wss[a].hist.retain(|x| *x != f);
        self.wss[t].windows.push(f);
        self.record(t, f);
        self.active = t;
        self.cleanup();
        self.trailing();
    }

    fn move_workspace(&mut self, down: bool) {
        let a = self.active;
        let n = if down {
            (a + 1).min(self.wss.len() - 1)
        } else {
            a.saturating_sub(1)
        };
        if n == a {
            return;
        }
        self.wss.swap(a, n);
        self.active = n;
        self.cleanup();
        self.trailing();
    }

    /// 窓のキーを、閉じてよい（フォーカス窓以外の）順に並べる。
    fn closable(&self) -> Vec<u64> {
        let mut keys: Vec<u64> = self
            .wss
            .iter()
            .flat_map(|w| w.windows.iter().copied())
            .filter(|k| Some(*k) != self.focus)
            .collect();
        keys.sort_unstable();
        keys
    }
}

#[derive(Debug, Clone)]
enum Op {
    Open,
    Close(usize),
    Switch(u8),
    MoveColumn(u8),
    Down,
    Up,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => Just(Op::Open),
        2 => any::<usize>().prop_map(Op::Close),
        3 => (0u8..6).prop_map(Op::Switch),
        3 => (0u8..6).prop_map(Op::MoveColumn),
        2 => Just(Op::Down),
        2 => Just(Op::Up),
    ]
}

/// Core の見た目を、モデルと同じ形にそろえて比べる。
fn same(c: &TestCore, m: &Model) -> Result<(), TestCaseError> {
    let want: Vec<(u64, Vec<u64>)> = m
        .wss
        .iter()
        .map(|w| {
            let mut k = w.windows.clone();
            k.sort_unstable();
            (w.id, k)
        })
        .collect();
    prop_assert_eq!(workspaces(c), want);
    prop_assert_eq!(usize::from(shown(c)), m.active);
    prop_assert_eq!(focus_key(c), m.focus);
    Ok(())
}

proptest! {
    /// どんな操作の列でも、Core はモデルと同じワークスペースの並び（id と窓）・表示・フォーカスになり、不変条件が保たれる。
    #[test]
    fn matches_the_reference_model(ops in prop::collection::vec(op(), 0..50)) {
        let mut c = dynamic(0);
        let mut m = Model::new();
        same(&c, &m)?;
        for o in ops {
            match o {
                Op::Open => {
                    let k = m.next_key;
                    open(&mut c, k);
                    m.open();
                }
                Op::Close(i) => {
                    let keys = m.closable();
                    if keys.is_empty() {
                        continue;
                    }
                    let k = keys[i % keys.len()];
                    close(&mut c, k);
                    m.close(k);
                }
                Op::Switch(n) => {
                    act(&mut c, Action::SwitchWorkspace(ws(n)));
                    m.switch(n);
                }
                Op::MoveColumn(n) => {
                    if m.focus.is_none() {
                        continue;
                    }
                    act(&mut c, Action::MoveColumnToWorkspace(ws(n)));
                    m.move_column(n);
                }
                Op::Down => {
                    act(&mut c, Action::MoveWorkspaceDown);
                    m.move_workspace(true);
                }
                Op::Up => {
                    act(&mut c, Action::MoveWorkspaceUp);
                    m.move_workspace(false);
                }
            }
            same(&c, &m)?;
            // 末尾は常に空で、id は互いに異なる（D63・D70）。
            let list = workspaces(&c);
            prop_assert!(list.last().is_some_and(|(_, k)| k.is_empty()));
            let mut ids: Vec<u64> = list.iter().map(|(id, _)| *id).collect();
            ids.sort_unstable();
            ids.dedup();
            prop_assert_eq!(ids.len(), list.len());
        }
    }

    /// `Fixed` では、同じ操作の列でも、個数は変わらず id は位置 + 1 のまま。
    #[test]
    fn fixed_mode_never_changes_the_workspace_list(ops in prop::collection::vec(op(), 0..30)) {
        let mut c = core_in(WorkspaceMode::Fixed, LayoutMode::Scrolling, 0);
        let mut next = FIRST_KEY;
        for o in ops {
            match o {
                Op::Open => {
                    open(&mut c, next);
                    next += 1;
                }
                Op::Switch(n) => act(&mut c, Action::SwitchWorkspace(ws(n))),
                Op::MoveColumn(n) => {
                    if c.view().focus().is_some() {
                        act(&mut c, Action::MoveColumnToWorkspace(ws(n)));
                    }
                }
                Op::Down => act(&mut c, Action::MoveWorkspaceDown),
                Op::Up => act(&mut c, Action::MoveWorkspaceUp),
                Op::Close(_) => {}
            }
            prop_assert_eq!(c.view().workspace_count(), 9);
            for i in 0..9u8 {
                prop_assert_eq!(c.view().workspace_id(ws(i)).map(|id| id.raw()), Some(u64::from(i) + 1));
            }
        }
    }
}

/// モデルの前提: 窓のキーは `ExternalKey` の通し番号（11 から）。
#[test]
fn the_model_assumes_keys_start_at_eleven() {
    let c = dynamic(1);
    assert!(c.view().window_id(ExternalKey::new(FIRST_KEY)).is_some());
    let _ = apply_ok;
}
