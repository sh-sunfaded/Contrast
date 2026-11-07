//! 複数モニタ（`docs/core-spec-p11.md` の 17 節、D72〜D84）。
//!
//! 具体例のテストと、参照モデルとの比較（proptest）。モデルは niri の動作（`src/layout/mod.rs` の
//! `add_output`・`remove_output`・`move_column_to_output`、`src/layout/monitor.rs` の `Monitor::new`・
//! `append_workspaces`・`clean_up_workspaces`、`src/niri.rs` の `output_left_of` ほか）を仕様の文章から書いたもので、
//! 実装のコードは見ていない。

use std::collections::{BTreeMap, BTreeSet};

use contrast_core::{
    Action, Command, CoreConfig, CoreError, DefaultPolicy, Direction, Effect, ExternalKey,
    LayoutMode, OutputId, OutputName, WindowState, WorkspaceId, WorkspaceIndex,
    WorkspaceMode,
};
use proptest::prelude::*;

use crate::common::{FIRST_KEY, TestCore, act, apply_err, apply_ok, focus_key, open, rect};

/// 出力の名前とキーと矩形。A の右に B、A の下に C、B の右に D。
const OUTPUTS: [(&str, u64, (i32, i32, i32, i32)); 4] = [
    ("A", 1, (0, 0, 900, 600)),
    ("B", 2, (900, 0, 900, 600)),
    ("C", 3, (0, 600, 900, 600)),
    ("D", 4, (1800, 0, 900, 600)),
];

/// 出力が 0 台の `Dynamic`・`Scrolling` の Core。
fn empty_dynamic() -> TestCore {
    contrast_core::Core::new(
        CoreConfig::new(9)
            .with_layout(LayoutMode::Scrolling)
            .with_workspace_mode(WorkspaceMode::Dynamic),
        DefaultPolicy::new(),
        contrast_core::Columns::new(),
    )
}

/// 出力が 0 台の `Fixed`・`Scrolling` の Core。
fn empty_fixed() -> TestCore {
    contrast_core::Core::new(
        CoreConfig::new(4).with_layout(LayoutMode::Scrolling),
        DefaultPolicy::new(),
        contrast_core::Columns::new(),
    )
}

/// 出力 `n`（`OUTPUTS` の添字）を足す。
fn add(c: &mut TestCore, n: usize) {
    let (name, key, (x, y, w, h)) = OUTPUTS[n];
    apply_ok(
        c,
        Command::OutputAdded {
            key: ExternalKey::new(key),
            name: OutputName::new(name),
            geometry: rect(x, y, w, h),
        },
    );
}

/// 出力 `n` を外す。
fn remove(c: &mut TestCore, n: usize) {
    apply_ok(c, Command::OutputRemoved(ExternalKey::new(OUTPUTS[n].1)));
}

/// 出力 `n` の OutputId。
fn oid(c: &TestCore, n: usize) -> OutputId {
    c.view()
        .output_id(ExternalKey::new(OUTPUTS[n].1))
        .expect("出力がある")
}

/// 窓 `id` のキー（11 から）。
fn key_of(c: &TestCore, id: contrast_core::WindowId) -> u64 {
    let want = c.view().window_key(id).expect("キーがある");
    (FIRST_KEY..FIRST_KEY + 256)
        .find(|k| ExternalKey::new(*k) == want)
        .expect("テストのキー")
}

/// ワークスペース `ws` の窓のキー（昇順）。
fn keys_in(c: &TestCore, ws: WorkspaceId) -> Vec<u64> {
    let mut keys: Vec<u64> = c
        .view()
        .workspace_windows_by_id(ws)
        .expect("ワークスペースがある")
        .iter()
        .map(|w| key_of(c, *w))
        .collect();
    keys.sort_unstable();
    keys
}

/// 出力 `n` の列: ワークスペースごとの窓のキー。
fn list(c: &TestCore, n: usize) -> Vec<Vec<u64>> {
    c.view()
        .output_workspaces(oid(c, n))
        .into_iter()
        .map(|ws| keys_in(c, ws))
        .collect()
}

/// 出力 `n` が表示している、列の中の位置。
fn shown_at(c: &TestCore, n: usize) -> usize {
    let o = oid(c, n);
    let id = c
        .view()
        .shown_workspace(o)
        .and_then(|i| c.view().workspace_id(i))
        .expect("表示中");
    c.view()
        .output_workspaces(o)
        .iter()
        .position(|w| *w == id)
        .expect("自分の列にある")
}

/// アクティブな出力の `OUTPUTS` の添字。
fn active(c: &TestCore) -> Option<usize> {
    let a = c.view().active_output()?;
    (0..OUTPUTS.len()).find(|n| c.view().output_id(ExternalKey::new(OUTPUTS[*n].1)) == Some(a))
}

/// 窓 `k` が属するワークスペースの id。
fn ws_of(c: &TestCore, k: u64) -> WorkspaceId {
    let id = c.view().window_id(ExternalKey::new(k)).expect("窓がある");
    c.view().window(id).expect("窓がある").workspace_id
}

#[test]
fn one_output_lists_the_same_workspaces_as_the_whole() {
    let mut c = empty_dynamic();
    add(&mut c, 0);
    open(&mut c, 11);
    let whole: Vec<WorkspaceId> = (0..c.view().workspace_count())
        .filter_map(|i| c.view().workspace_id(WorkspaceIndex::new(i)))
        .collect();
    assert_eq!(c.view().output_workspaces(oid(&c, 0)), whole);
    assert_eq!(list(&c, 0), [vec![11], vec![]]);
}

#[test]
fn each_output_has_its_own_list_and_the_first_output_stays_active() {
    let mut c = empty_dynamic();
    add(&mut c, 0);
    add(&mut c, 1);
    assert_eq!(list(&c, 0), [Vec::<u64>::new()]);
    assert_eq!(list(&c, 1), [Vec::<u64>::new()]);
    assert_eq!(active(&c), Some(0));
    open(&mut c, 11);
    assert_eq!(list(&c, 0), [vec![11], vec![]]);
    assert_eq!(list(&c, 1), [Vec::<u64>::new()]);
}

#[test]
fn focus_monitor_moves_the_active_output_and_new_windows_follow_it() {
    let mut c = empty_dynamic();
    add(&mut c, 0);
    add(&mut c, 1);
    open(&mut c, 11);
    act(&mut c, Action::FocusMonitor(Direction::Right));
    assert_eq!(active(&c), Some(1));
    assert_eq!(focus_key(&c), None, "B は空");
    open(&mut c, 12);
    assert_eq!(list(&c, 1), [vec![12], vec![]]);
    assert_eq!(focus_key(&c), Some(12));
    act(&mut c, Action::FocusMonitor(Direction::Left));
    assert_eq!(active(&c), Some(0));
    assert_eq!(focus_key(&c), Some(11), "A の表示中のワークスペースのフォーカスに戻る");
}

#[test]
fn focus_monitor_without_a_candidate_changes_nothing() {
    let mut c = empty_dynamic();
    add(&mut c, 0);
    add(&mut c, 1);
    open(&mut c, 11);
    let before = c.snapshot();
    let applied = c
        .apply(Command::Action(Action::FocusMonitor(Direction::Left)))
        .expect("ok");
    assert!(applied.effects.is_empty());
    assert_eq!(c.snapshot(), before);
    // B から下: C は無い。
    act(&mut c, Action::FocusMonitor(Direction::Right));
    let before = c.snapshot();
    act(&mut c, Action::FocusMonitor(Direction::Down));
    assert_eq!(c.snapshot(), before);
}

#[test]
fn only_outputs_overlapping_on_the_other_axis_are_candidates() {
    let mut c = empty_dynamic();
    add(&mut c, 1); // B (900,0)
    add(&mut c, 2); // C (0,600): B の左下。y が接するだけで重ならない。
    assert_eq!(active(&c), Some(1));
    let before = c.snapshot();
    act(&mut c, Action::FocusMonitor(Direction::Left));
    act(&mut c, Action::FocusMonitor(Direction::Down));
    assert_eq!(c.snapshot(), before);
    // A を足すと、B の左は A、A の下は C。
    add(&mut c, 0);
    act(&mut c, Action::FocusMonitor(Direction::Left));
    assert_eq!(active(&c), Some(0));
    act(&mut c, Action::FocusMonitor(Direction::Down));
    assert_eq!(active(&c), Some(2));
    act(&mut c, Action::FocusMonitor(Direction::Up));
    assert_eq!(active(&c), Some(0));
}

#[test]
fn the_nearest_center_wins() {
    let mut c = empty_dynamic();
    add(&mut c, 0);
    add(&mut c, 3); // D: A から 1800 右
    add(&mut c, 1); // B: A から 900 右
    act(&mut c, Action::FocusMonitor(Direction::Right));
    assert_eq!(active(&c), Some(1));
    act(&mut c, Action::FocusMonitor(Direction::Right));
    assert_eq!(active(&c), Some(3));
}

#[test]
fn equal_distances_go_to_the_smaller_output_id() {
    let mut c = empty_dynamic();
    add(&mut c, 0);
    // A の右に、同じ中心の x の出力を 2 つ（上下にずらして、どちらも A と y で重なる）。
    apply_ok(
        &mut c,
        Command::OutputAdded {
            key: ExternalKey::new(20),
            name: OutputName::new("E"),
            geometry: rect(900, 300, 900, 600),
        },
    );
    apply_ok(
        &mut c,
        Command::OutputAdded {
            key: ExternalKey::new(21),
            name: OutputName::new("F"),
            geometry: rect(900, -300, 900, 600),
        },
    );
    act(&mut c, Action::FocusMonitor(Direction::Right));
    assert_eq!(
        c.view().active_output(),
        c.view().output_id(ExternalKey::new(20))
    );
}

#[test]
fn move_column_to_monitor_moves_the_column_and_the_focus_follows() {
    let mut c = empty_dynamic();
    add(&mut c, 0);
    add(&mut c, 1);
    open(&mut c, 11);
    open(&mut c, 12);
    act(&mut c, Action::ConsumeOrExpelWindowLeft); // 12 を 11 の列へ
    open(&mut c, 13);
    act(&mut c, Action::FocusMonitor(Direction::Right));
    open(&mut c, 14);
    act(&mut c, Action::FocusMonitor(Direction::Left));
    act(&mut c, Action::FocusColumnFirst); // 列 {11,12}
    let focused = focus_key(&c).expect("フォーカスあり");
    act(&mut c, Action::MoveColumnToMonitor(Direction::Right));
    assert_eq!(active(&c), Some(1));
    assert_eq!(focus_key(&c), Some(focused));
    assert_eq!(list(&c, 0), [vec![13], vec![]]);
    assert_eq!(list(&c, 1), [vec![11, 12, 14], vec![]]);
    // 14 の列の右に、列ごと（2 枚が同じ x）入る。
    let x = |k: u64| crate::common::rect_of(&c, k).x();
    assert!(x(14) < x(11));
    assert_eq!(x(11), x(12));
}

#[test]
fn moving_the_last_column_leaves_the_shown_workspace_in_place() {
    let mut c = empty_dynamic();
    add(&mut c, 0);
    add(&mut c, 1);
    open(&mut c, 11);
    act(&mut c, Action::MoveColumnToMonitor(Direction::Right));
    assert_eq!(list(&c, 0), [Vec::<u64>::new(), vec![]]);
    assert_eq!(shown_at(&c, 0), 0);
    assert_eq!(list(&c, 1), [vec![11], vec![]]);
    // A に戻って切り替えると、空になったワークスペースは整理される。
    act(&mut c, Action::FocusMonitor(Direction::Left));
    act(&mut c, Action::SwitchWorkspace(WorkspaceIndex::new(1)));
    assert_eq!(list(&c, 0), [Vec::<u64>::new()]);
}

#[test]
fn a_floating_window_moves_alone() {
    let mut c = empty_dynamic();
    add(&mut c, 0);
    add(&mut c, 1);
    open(&mut c, 11);
    open(&mut c, 12);
    act(&mut c, Action::ToggleFloating); // 12 は浮動
    act(&mut c, Action::MoveColumnToMonitor(Direction::Right));
    assert_eq!(list(&c, 0), [vec![11], vec![]]);
    assert_eq!(list(&c, 1), [vec![12], vec![]]);
    let id = c.view().window_id(ExternalKey::new(12)).expect("窓");
    assert_eq!(
        c.view().window(id).map(|w| w.state),
        Some(WindowState::Floating)
    );
    assert_eq!(focus_key(&c), Some(12));
}

#[test]
fn move_column_to_monitor_needs_a_focused_window() {
    let mut c = empty_dynamic();
    add(&mut c, 0);
    add(&mut c, 1);
    let err = apply_err(
        &mut c,
        Command::Action(Action::MoveColumnToMonitor(Direction::Right)),
    );
    assert_eq!(err, CoreError::NoFocusedWindow);
    // 相手が無ければ、フォーカスが無くても状態不変で Ok。
    let before = c.snapshot();
    act(&mut c, Action::MoveColumnToMonitor(Direction::Left));
    assert_eq!(c.snapshot(), before);
}

#[test]
fn monitor_actions_do_nothing_in_fixed_mode() {
    let mut c = empty_fixed();
    add(&mut c, 0);
    add(&mut c, 1);
    open(&mut c, 11);
    let before = c.snapshot();
    act(&mut c, Action::FocusMonitor(Direction::Right));
    act(&mut c, Action::MoveColumnToMonitor(Direction::Right));
    apply_ok(
        &mut c,
        Command::OutputFocusRequested(ExternalKey::new(OUTPUTS[1].1)),
    );
    assert_eq!(c.snapshot(), before);
    let err = apply_err(&mut c, Command::OutputFocusRequested(ExternalKey::new(99)));
    assert_eq!(err, CoreError::UnknownKey);
}

#[test]
fn move_column_to_monitor_does_nothing_in_equal_mode() {
    let mut c = contrast_core::Core::new(
        CoreConfig::new(9).with_workspace_mode(WorkspaceMode::Dynamic),
        DefaultPolicy::new(),
        contrast_core::Columns::new(),
    );
    add(&mut c, 0);
    add(&mut c, 1);
    open(&mut c, 11);
    let before = c.snapshot();
    act(&mut c, Action::MoveColumnToMonitor(Direction::Right));
    assert_eq!(c.snapshot(), before);
    // FocusMonitor は Equal でも動く。
    act(&mut c, Action::FocusMonitor(Direction::Right));
    assert_eq!(active(&c), Some(1));
}

#[test]
fn output_focus_requested_activates_the_output() {
    let mut c = empty_dynamic();
    add(&mut c, 0);
    add(&mut c, 1);
    open(&mut c, 11);
    act(&mut c, Action::FocusMonitor(Direction::Right));
    open(&mut c, 12);
    apply_ok(
        &mut c,
        Command::OutputFocusRequested(ExternalKey::new(OUTPUTS[0].1)),
    );
    assert_eq!(active(&c), Some(0));
    assert_eq!(focus_key(&c), Some(11));
    let before = c.snapshot();
    let applied = c
        .apply(Command::OutputFocusRequested(ExternalKey::new(OUTPUTS[0].1)))
        .expect("ok");
    assert!(applied.effects.is_empty(), "すでにアクティブ");
    assert_eq!(c.snapshot(), before);
    let err = apply_err(&mut c, Command::OutputFocusRequested(ExternalKey::new(99)));
    assert_eq!(err, CoreError::UnknownKey);
}

#[test]
fn clicking_a_window_on_another_output_activates_that_output() {
    let mut c = empty_dynamic();
    add(&mut c, 0);
    add(&mut c, 1);
    open(&mut c, 11);
    act(&mut c, Action::FocusMonitor(Direction::Right));
    open(&mut c, 12);
    apply_ok(&mut c, Command::WindowFocusRequested(ExternalKey::new(11)));
    assert_eq!(active(&c), Some(0));
    assert_eq!(focus_key(&c), Some(11));
}

#[test]
fn switch_workspace_counts_within_the_active_output() {
    let mut c = empty_dynamic();
    add(&mut c, 0);
    add(&mut c, 1);
    open(&mut c, 11);
    act(&mut c, Action::SwitchWorkspace(WorkspaceIndex::new(1)));
    open(&mut c, 12); // A: [{11}, {12}, 空]
    act(&mut c, Action::FocusMonitor(Direction::Right));
    open(&mut c, 13); // B: [{13}, 空]
    act(&mut c, Action::SwitchWorkspace(WorkspaceIndex::new(5)));
    assert_eq!(shown_at(&c, 1), 1, "B の列の末尾に丸める");
    assert_eq!(shown_at(&c, 0), 1, "A は変わらない");
    act(&mut c, Action::SwitchWorkspace(WorkspaceIndex::new(0)));
    assert_eq!(shown_at(&c, 1), 0);
    assert_eq!(focus_key(&c), Some(13));
    // A の 0 番は A の列の先頭のまま（B と入れ替わらない）。
    assert_eq!(list(&c, 0), [vec![11], vec![12], vec![]]);
}

#[test]
fn move_column_to_workspace_uses_the_active_outputs_list() {
    let mut c = empty_dynamic();
    add(&mut c, 0);
    add(&mut c, 1);
    open(&mut c, 11);
    act(&mut c, Action::FocusMonitor(Direction::Right));
    open(&mut c, 12);
    open(&mut c, 13);
    act(&mut c, Action::MoveColumnToWorkspace(WorkspaceIndex::new(1)));
    assert_eq!(list(&c, 1), [vec![12], vec![13], vec![]]);
    assert_eq!(list(&c, 0), [vec![11], vec![]]);
    assert_eq!(shown_at(&c, 1), 1);
}

#[test]
fn removing_an_output_moves_its_workspaces_with_windows_to_the_primary() {
    let mut c = empty_dynamic();
    add(&mut c, 0);
    add(&mut c, 1);
    open(&mut c, 11);
    act(&mut c, Action::FocusMonitor(Direction::Right));
    open(&mut c, 12);
    act(&mut c, Action::SwitchWorkspace(WorkspaceIndex::new(1)));
    open(&mut c, 13); // B: [{12}, {13}, 空]
    act(&mut c, Action::FocusMonitor(Direction::Left));
    remove(&mut c, 1);
    assert_eq!(list(&c, 0), [vec![11], vec![12], vec![13], vec![]]);
    assert_eq!(shown_at(&c, 0), 0);
    assert_eq!(active(&c), Some(0));
    assert_eq!(focus_key(&c), Some(11));
}

#[test]
fn removing_the_active_output_moves_the_focus_to_the_previous_one() {
    let mut c = empty_dynamic();
    add(&mut c, 0);
    add(&mut c, 1);
    open(&mut c, 11);
    act(&mut c, Action::FocusMonitor(Direction::Right));
    open(&mut c, 12);
    remove(&mut c, 1);
    assert_eq!(active(&c), Some(0));
    // A の表示中のワークスペースのフォーカス（11）。12 のワークスペースは A の末尾の空の前へ。
    assert_eq!(focus_key(&c), Some(11));
    assert_eq!(list(&c, 0), [vec![11], vec![12], vec![]]);
}

#[test]
fn an_output_that_comes_back_gets_its_workspaces_and_shown_workspace_back() {
    let mut c = empty_dynamic();
    add(&mut c, 0);
    add(&mut c, 1);
    open(&mut c, 11);
    act(&mut c, Action::FocusMonitor(Direction::Right));
    open(&mut c, 12);
    act(&mut c, Action::SwitchWorkspace(WorkspaceIndex::new(1)));
    open(&mut c, 13); // B: [{12}, {13}, 空]、表示は {13}
    let shown_ws = ws_of(&c, 13);
    remove(&mut c, 1);
    assert_eq!(list(&c, 0), [vec![11], vec![12], vec![13], vec![]]);
    add(&mut c, 1);
    assert_eq!(list(&c, 0), [vec![11], vec![]]);
    assert_eq!(list(&c, 1), [vec![12], vec![13], vec![]]);
    assert_eq!(
        c.view()
            .shown_workspace(oid(&c, 1))
            .and_then(|i| c.view().workspace_id(i)),
        Some(shown_ws)
    );
    assert_eq!(active(&c), Some(0), "戻った出力はアクティブにならない");
}

#[test]
fn a_workspace_that_got_a_window_on_another_output_stays_there() {
    let mut c = empty_dynamic();
    add(&mut c, 0);
    add(&mut c, 1);
    act(&mut c, Action::FocusMonitor(Direction::Right));
    open(&mut c, 12);
    remove(&mut c, 1); // A: [{12}, 空] ... 12 のワークスペースは元の出力 B
    act(&mut c, Action::SwitchWorkspace(WorkspaceIndex::new(0)));
    open(&mut c, 13); // A で窓が入ったので、元の出力は A になる
    add(&mut c, 1);
    assert_eq!(list(&c, 0), [vec![12, 13], vec![]]);
    assert_eq!(list(&c, 1), [Vec::<u64>::new()]);
}

#[test]
fn the_last_output_going_away_keeps_the_windows_and_they_come_back() {
    let mut c = empty_dynamic();
    add(&mut c, 0);
    open(&mut c, 11);
    act(&mut c, Action::SwitchWorkspace(WorkspaceIndex::new(1)));
    open(&mut c, 12);
    remove(&mut c, 0);
    assert_eq!(c.view().active_output(), None);
    assert_eq!(c.view().focus(), None);
    // 持ち主の無い列: 窓のあるものと、末尾の空。
    assert_eq!(c.view().workspace_count(), 3);
    add(&mut c, 0);
    assert_eq!(list(&c, 0), [vec![11], vec![12], vec![]]);
    assert_eq!(shown_at(&c, 0), 1, "最後に表示していたワークスペース");
    assert_eq!(active(&c), Some(0));
    assert_eq!(focus_key(&c), Some(12));
}

#[test]
fn show_workspace_is_sent_only_when_the_shown_id_changes() {
    let mut c = empty_dynamic();
    add(&mut c, 0);
    add(&mut c, 1);
    open(&mut c, 11);
    let applied = c
        .apply(Command::Action(Action::FocusMonitor(Direction::Right)))
        .expect("ok");
    assert!(
        !applied
            .effects
            .iter()
            .any(|e| matches!(e, Effect::ShowWorkspace { .. })),
        "どちらの表示も変わらない"
    );
    assert!(applied.effects.contains(&Effect::SetFocus(None)));
}

// ---------------------------------------------------------------------------
// 参照モデル（niri の動作から書いたもの）
// ---------------------------------------------------------------------------

/// モデルのワークスペース。
#[derive(Clone, Debug)]
struct MWs {
    /// モデルの中の id（`last_active` の照合だけに使う）。
    id: u64,
    windows: BTreeSet<u64>,
    /// 元の出力（`OUTPUTS` の添字）。
    original: Option<usize>,
}

/// モデルのモニタ。
#[derive(Clone, Debug)]
struct MMon {
    output: usize,
    ws: Vec<MWs>,
    active: usize,
}

/// モデル全体（niri の `MonitorSet` に当たる）。
#[derive(Clone, Debug)]
struct Model {
    /// 足した順（OutputId 昇順と同じ）。
    mons: Vec<MMon>,
    active: usize,
    primary: usize,
    last_active: BTreeMap<usize, u64>,
    /// 出力が 0 台のときの列。
    unowned: Vec<MWs>,
    next_id: u64,
}

impl Model {
    fn new() -> Self {
        Self {
            mons: Vec::new(),
            active: 0,
            primary: 0,
            last_active: BTreeMap::new(),
            unowned: vec![MWs {
                id: 1,
                windows: BTreeSet::new(),
                original: None,
            }],
            next_id: 2,
        }
    }

    fn new_ws(&mut self, original: Option<usize>) -> MWs {
        let id = self.next_id;
        self.next_id += 1;
        MWs {
            id,
            windows: BTreeSet::new(),
            original,
        }
    }

    /// 各列の末尾を空にする。
    fn ensure_trailing(&mut self) {
        for i in 0..self.mons.len() {
            if self.mons[i]
                .ws
                .last()
                .is_none_or(|w| !w.windows.is_empty())
            {
                let o = self.mons[i].output;
                let ws = self.new_ws(Some(o));
                self.mons[i].ws.push(ws);
            }
        }
        if self.mons.is_empty() && self.unowned.last().is_none_or(|w| !w.windows.is_empty()) {
            let ws = self.new_ws(None);
            self.unowned.push(ws);
        }
    }

    /// niri の `clean_up_workspaces`。
    fn clean(ws: &mut Vec<MWs>, active: &mut usize) {
        if ws.is_empty() {
            return;
        }
        for idx in (0..ws.len() - 1).rev() {
            if *active == idx {
                continue;
            }
            if ws[idx].windows.is_empty() {
                ws.remove(idx);
                if *active > idx {
                    *active -= 1;
                }
            }
        }
    }

    fn mon_of(&self, output: usize) -> Option<usize> {
        self.mons.iter().position(|m| m.output == output)
    }

    /// 窓 `k` の場所 `(モニタ, ワークスペース)`。出力が無いときは `(None, 位置)`。
    fn find(&self, k: u64) -> Option<(Option<usize>, usize)> {
        for (m, mon) in self.mons.iter().enumerate() {
            if let Some(i) = mon.ws.iter().position(|w| w.windows.contains(&k)) {
                return Some((Some(m), i));
            }
        }
        self.unowned
            .iter()
            .position(|w| w.windows.contains(&k))
            .map(|i| (None, i))
    }

    fn add_output(&mut self, output: usize) {
        if self.mons.is_empty() {
            let mut ws: Vec<MWs> = Vec::new();
            let n = self.unowned.len();
            for (i, w) in std::mem::take(&mut self.unowned).into_iter().enumerate() {
                if !w.windows.is_empty() || i + 1 == n {
                    ws.push(w);
                }
            }
            if ws.last().is_none_or(|w| !w.windows.is_empty()) {
                let e = self.new_ws(Some(output));
                ws.push(e);
            }
            let want = self.last_active.remove(&output);
            let active = ws
                .iter()
                .position(|w| Some(w.id) == want)
                .unwrap_or(0);
            self.mons.push(MMon { output, ws, active });
            self.active = 0;
            self.primary = 0;
            return;
        }
        let mut moved = Vec::new();
        for m in 0..self.mons.len() {
            let mon = &mut self.mons[m];
            for i in (0..mon.ws.len()).rev() {
                if mon.ws[i].original != Some(output) {
                    continue;
                }
                let w = mon.ws.remove(i);
                if !w.windows.is_empty() {
                    moved.push(w);
                }
                if i <= mon.active {
                    mon.active = mon.active.saturating_sub(1);
                }
            }
        }
        moved.reverse();
        let e = self.new_ws(Some(output));
        moved.push(e);
        let want = self.last_active.remove(&output);
        let active = moved
            .iter()
            .position(|w| Some(w.id) == want)
            .unwrap_or(0);
        self.mons.push(MMon {
            output,
            ws: moved,
            active,
        });
    }

    fn remove_output(&mut self, output: usize) {
        let Some(idx) = self.mon_of(output) else {
            return;
        };
        let mon = self.mons.remove(idx);
        self.last_active.insert(output, mon.ws[mon.active].id);
        let with_windows: Vec<MWs> = mon
            .ws
            .into_iter()
            .filter(|w| !w.windows.is_empty())
            .collect();
        if self.mons.is_empty() {
            self.unowned = with_windows;
            self.ensure_trailing();
            return;
        }
        if self.primary >= idx {
            self.primary = self.primary.saturating_sub(1);
        }
        if self.active >= idx {
            self.active = self.active.saturating_sub(1);
        }
        // 移し先ごとに、元の順で。
        let mut per_target: BTreeMap<usize, Vec<MWs>> = BTreeMap::new();
        for w in with_windows {
            let target = w
                .original
                .and_then(|o| self.mon_of(o))
                .unwrap_or(self.primary);
            per_target.entry(target).or_default().push(w);
        }
        for (target, wss) in per_target {
            let mon = &mut self.mons[target];
            let empty_was_active = mon.active == mon.ws.len() - 1;
            let empty = mon.ws.pop().expect("末尾の空");
            mon.ws.extend(wss);
            mon.ws.push(empty);
            if empty_was_active {
                mon.active = mon.ws.len() - 1;
            }
            let (ws, active) = (&mut mon.ws, &mut mon.active);
            Self::clean(ws, active);
        }
    }

    fn open(&mut self, k: u64) {
        if self.mons.is_empty() {
            self.unowned[0].windows.insert(k);
        } else {
            let a = self.active;
            let mon = &mut self.mons[a];
            let i = mon.active;
            mon.ws[i].windows.insert(k);
            mon.ws[i].original = Some(mon.output);
        }
        self.ensure_trailing();
    }

    fn close(&mut self, k: u64) {
        if let Some((m, i)) = self.find(k) {
            match m {
                Some(m) => {
                    self.mons[m].ws[i].windows.remove(&k);
                }
                None => {
                    self.unowned[i].windows.remove(&k);
                }
            }
        }
    }

    fn switch(&mut self, index: usize) {
        if self.mons.is_empty() {
            return;
        }
        let a = self.active;
        let mon = &mut self.mons[a];
        let t = index.min(mon.ws.len() - 1);
        if t == mon.active {
            return;
        }
        mon.active = t;
        let (ws, active) = (&mut mon.ws, &mut mon.active);
        Self::clean(ws, active);
    }

    /// 窓 `k`（フォーカス窓）を、アクティブな出力の列の `index` へ（MoveColumnToWorkspace、列は 1 枚ずつ）。
    fn move_column_to_workspace(&mut self, k: u64, index: usize) {
        if self.mons.is_empty() {
            return;
        }
        let a = self.active;
        let mon = &mut self.mons[a];
        let t = index.min(mon.ws.len() - 1);
        if t == mon.active {
            return;
        }
        let from = mon.active;
        mon.ws[from].windows.remove(&k);
        mon.ws[t].windows.insert(k);
        mon.ws[t].original = Some(mon.output);
        mon.active = t;
        self.ensure_trailing();
        let mon = &mut self.mons[a];
        let (ws, active) = (&mut mon.ws, &mut mon.active);
        Self::clean(ws, active);
    }

    fn move_workspace(&mut self, down: bool) {
        if self.mons.is_empty() {
            return;
        }
        let a = self.active;
        let mon = &mut self.mons[a];
        let cur = mon.active;
        let next = if down {
            (cur + 1).min(mon.ws.len() - 1)
        } else {
            cur.saturating_sub(1)
        };
        if next == cur {
            return;
        }
        mon.ws.swap(cur, next);
        mon.active = next;
        self.ensure_trailing();
        let mon = &mut self.mons[a];
        let (ws, active) = (&mut mon.ws, &mut mon.active);
        Self::clean(ws, active);
    }

    /// niri の `output_*_of`。
    fn neighbor(&self, dir: Direction) -> Option<usize> {
        let cur = self.mons.get(self.active)?.output;
        let g = |o: usize| OUTPUTS[o].2;
        let center = |o: usize| {
            let (x, y, w, h) = g(o);
            (x + w / 2, y + h / 2)
        };
        let (cx, cy) = center(cur);
        let (x0, y0, w0, h0) = g(cur);
        let mut best: Option<(i32, usize)> = None;
        for (m, mon) in self.mons.iter().enumerate() {
            let o = mon.output;
            let (x, y, w, h) = g(o);
            let (ox, oy) = center(o);
            let (ok, dist) = match dir {
                Direction::Left => (ox < cx && y < y0 + h0 && y0 < y + h, cx - ox),
                Direction::Right => (ox > cx && y < y0 + h0 && y0 < y + h, ox - cx),
                Direction::Up => (oy < cy && x < x0 + w0 && x0 < x + w, cy - oy),
                Direction::Down => (oy > cy && x < x0 + w0 && x0 < x + w, oy - cy),
                _ => (false, 0),
            };
            if ok && best.is_none_or(|(d, _)| dist < d) {
                best = Some((dist, m));
            }
        }
        best.map(|(_, m)| m)
    }

    fn focus_monitor(&mut self, dir: Direction) {
        if let Some(m) = self.neighbor(dir) {
            self.active = m;
        }
    }

    /// 窓 `k`（フォーカス窓）を相手のモニタの表示中のワークスペースへ（列は 1 枚ずつ）。
    fn move_column_to_monitor(&mut self, k: u64, target: usize) {
        let a = self.active;
        let from = self.mons[a].active;
        self.mons[a].ws[from].windows.remove(&k);
        let mon = &mut self.mons[target];
        let t = mon.active;
        mon.ws[t].windows.insert(k);
        mon.ws[t].original = Some(mon.output);
        self.active = target;
        self.ensure_trailing();
    }
}

#[derive(Clone, Debug)]
enum Op {
    Add(usize),
    Remove(usize),
    Open,
    Close(usize),
    FocusMonitor(Direction),
    MoveColumnToMonitor(Direction),
    Switch(u8),
    MoveColumnToWorkspace(u8),
    MoveWorkspace(bool),
    FocusColumn(bool),
}

fn dir() -> impl Strategy<Value = Direction> {
    prop_oneof![
        Just(Direction::Left),
        Just(Direction::Right),
        Just(Direction::Up),
        Just(Direction::Down),
    ]
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        2 => (0usize..3).prop_map(Op::Add),
        1 => (0usize..3).prop_map(Op::Remove),
        4 => Just(Op::Open),
        2 => (0usize..8).prop_map(Op::Close),
        3 => dir().prop_map(Op::FocusMonitor),
        3 => dir().prop_map(Op::MoveColumnToMonitor),
        2 => (0u8..5).prop_map(Op::Switch),
        2 => (0u8..5).prop_map(Op::MoveColumnToWorkspace),
        1 => any::<bool>().prop_map(Op::MoveWorkspace),
        1 => any::<bool>().prop_map(Op::FocusColumn),
    ]
}

/// Core の、出力ごとの列（窓のキーの集合）と表示中の位置。出力が 0 台なら全体の並び。
fn observed(c: &TestCore) -> (Vec<(usize, Vec<Vec<u64>>, usize)>, Vec<Vec<u64>>) {
    let mut per = Vec::new();
    for n in 0..3 {
        if c.view().output_id(ExternalKey::new(OUTPUTS[n].1)).is_some() {
            per.push((n, list(c, n), shown_at(c, n)));
        }
    }
    let whole = (0..c.view().workspace_count())
        .map(|i| {
            let id = c
                .view()
                .workspace_id(WorkspaceIndex::new(i))
                .expect("id");
            keys_in(c, id)
        })
        .collect();
    (per, whole)
}

fn expected(m: &Model) -> (Vec<(usize, Vec<Vec<u64>>, usize)>, Vec<Vec<u64>>) {
    let mut per: Vec<(usize, Vec<Vec<u64>>, usize)> = m
        .mons
        .iter()
        .map(|mon| {
            (
                mon.output,
                mon.ws
                    .iter()
                    .map(|w| w.windows.iter().copied().collect())
                    .collect(),
                mon.active,
            )
        })
        .collect();
    per.sort_by_key(|p| p.0);
    let mut whole: Vec<Vec<u64>> = Vec::new();
    // 全体の並び: 出力を OutputId 昇順（= 足した順）に。
    for mon in &m.mons {
        whole.extend(mon.ws.iter().map(|w| w.windows.iter().copied().collect()));
    }
    if m.mons.is_empty() {
        whole.extend(m.unowned.iter().map(|w| w.windows.iter().copied().collect()));
    }
    (per, whole)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    /// どんな操作の列でも、出力ごとの列（窓の集合）・表示・アクティブな出力がモデルと一致し、
    /// フォーカスはアクティブな出力の表示中のワークスペースにあり（空なら None）、不変条件が保たれる。
    #[test]
    fn matches_the_reference_model(ops in prop::collection::vec(op(), 0..60)) {
        let mut c = empty_dynamic();
        let mut m = Model::new();
        let mut next_key = FIRST_KEY;
        let mut open_keys: Vec<u64> = Vec::new();
        for op in ops {
            match op {
                Op::Add(n) => {
                    if m.mon_of(n).is_some() { continue; }
                    add(&mut c, n);
                    m.add_output(n);
                }
                Op::Remove(n) => {
                    if m.mon_of(n).is_none() { continue; }
                    remove(&mut c, n);
                    m.remove_output(n);
                }
                Op::Open => {
                    if next_key >= FIRST_KEY + 200 { continue; }
                    open(&mut c, next_key);
                    m.open(next_key);
                    open_keys.push(next_key);
                    next_key += 1;
                }
                Op::Close(i) => {
                    if open_keys.is_empty() { continue; }
                    let k = open_keys.remove(i % open_keys.len());
                    apply_ok(&mut c, Command::WindowUnmapped(ExternalKey::new(k)));
                    m.close(k);
                }
                Op::FocusMonitor(d) => {
                    act(&mut c, Action::FocusMonitor(d));
                    m.focus_monitor(d);
                }
                Op::MoveColumnToMonitor(d) => {
                    let target = m.neighbor(d);
                    let focus = focus_key(&c);
                    match (target, focus) {
                        (None, _) => act(&mut c, Action::MoveColumnToMonitor(d)),
                        (Some(_), None) => {
                            let err = apply_err(&mut c, Command::Action(Action::MoveColumnToMonitor(d)));
                            prop_assert_eq!(err, CoreError::NoFocusedWindow);
                        }
                        (Some(t), Some(k)) => {
                            act(&mut c, Action::MoveColumnToMonitor(d));
                            m.move_column_to_monitor(k, t);
                        }
                    }
                }
                Op::Switch(i) => {
                    act(&mut c, Action::SwitchWorkspace(WorkspaceIndex::new(i)));
                    m.switch(usize::from(i));
                }
                Op::MoveColumnToWorkspace(i) => {
                    let cmd = Command::Action(Action::MoveColumnToWorkspace(WorkspaceIndex::new(i)));
                    match focus_key(&c) {
                        None => {
                            let err = apply_err(&mut c, cmd);
                            prop_assert_eq!(err, CoreError::NoFocusedWindow);
                        }
                        Some(k) => {
                            apply_ok(&mut c, cmd);
                            m.move_column_to_workspace(k, usize::from(i));
                        }
                    }
                }
                Op::MoveWorkspace(down) => {
                    let a = if down { Action::MoveWorkspaceDown } else { Action::MoveWorkspaceUp };
                    act(&mut c, a);
                    m.move_workspace(down);
                }
                Op::FocusColumn(left) => {
                    let d = if left { Direction::Left } else { Direction::Right };
                    act(&mut c, Action::FocusDirection(d));
                }
            }
            prop_assert_eq!(observed(&c), expected(&m));
            let want_active = m.mons.get(m.active).map(|mon| mon.output);
            prop_assert_eq!(active(&c), want_active);
            // フォーカスは、アクティブな出力の表示中のワークスペースにある（空なら None）。
            match m.mons.get(m.active) {
                None => prop_assert_eq!(c.view().focus(), None),
                Some(mon) => {
                    let ws = &mon.ws[mon.active];
                    match focus_key(&c) {
                        None => prop_assert!(ws.windows.is_empty(), "窓があるのにフォーカスが無い"),
                        Some(k) => prop_assert!(ws.windows.contains(&k), "フォーカスが表示中のワークスペースに無い"),
                    }
                }
            }
        }
    }
}
