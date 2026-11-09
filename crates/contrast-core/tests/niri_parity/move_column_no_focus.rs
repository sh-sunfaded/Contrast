//! 移すだけの move-column-to-workspace（`docs/core-spec-p13.md` の 19 節、D93〜D95）。実装より先に書いた。

use contrast_core::{Action, Command, CoreError, ExternalKey, LayoutMode, WorkspaceIndex, WorkspaceMode};

use crate::common::{
    act, apply_err, columns, core, core_in, dynamic, focus_key, open, shown, workspaces,
};

fn ws(n: u8) -> WorkspaceIndex {
    WorkspaceIndex::new(n)
}

#[test]
fn the_column_moves_but_the_shown_workspace_and_focus_stay() {
    let mut c = dynamic(3); // 列 11 | 12 | 13、フォーカス 13
    act(&mut c, Action::FocusDirection(contrast_core::Direction::Left)); // 12
    act(&mut c, Action::MoveColumnToWorkspaceNoFocus(ws(1)));
    assert_eq!(shown(&c), 0, "切り替えない");
    // 右の列（13）にフォーカス。
    assert_eq!(focus_key(&c), Some(13));
    assert_eq!(workspaces(&c), [(1, vec![11, 13]), (2, vec![12]), (3, vec![])]);
}

#[test]
fn the_last_column_hands_focus_to_the_left() {
    let mut c = dynamic(2); // 11 | 12、フォーカス 12
    act(&mut c, Action::MoveColumnToWorkspaceNoFocus(ws(1)));
    assert_eq!(focus_key(&c), Some(11));
    act(&mut c, Action::MoveColumnToWorkspaceNoFocus(ws(1)));
    assert_eq!(focus_key(&c), None, "列が残っていない");
    assert_eq!(shown(&c), 0);
    assert_eq!(workspaces(&c), [(1, vec![]), (2, vec![11, 12]), (3, vec![])]);
}

#[test]
fn the_column_goes_right_of_the_targets_last_focused_column() {
    let mut c = dynamic(0);
    open(&mut c, 11);
    act(&mut c, Action::SwitchWorkspace(ws(1)));
    open(&mut c, 12);
    open(&mut c, 13);
    act(&mut c, Action::FocusColumnFirst); // ws2 で 12 を最後にフォーカス
    act(&mut c, Action::SwitchWorkspace(ws(0)));
    act(&mut c, Action::MoveColumnToWorkspaceNoFocus(ws(1)));
    act(&mut c, Action::SwitchWorkspace(ws(1)));
    let order: Vec<Vec<u64>> = columns(&c, &[11, 12, 13]).into_iter().map(|(k, _)| k).collect();
    assert_eq!(order, [vec![12], vec![11], vec![13]]);
    // 対象のフォーカス履歴は変わらない（切り替えると 12）。
    assert_eq!(focus_key(&c), Some(12));
}

#[test]
fn edges_follow_move_column_to_workspace() {
    let mut eq = core(LayoutMode::Equal, 2);
    let before = eq.snapshot();
    act(&mut eq, Action::MoveColumnToWorkspaceNoFocus(ws(1)));
    assert_eq!(eq.snapshot(), before, "Equal では何もしない");
    let mut fixed = core_in(WorkspaceMode::Fixed, LayoutMode::Scrolling, 1);
    let err = apply_err(&mut fixed, Command::Action(Action::MoveColumnToWorkspaceNoFocus(ws(20))));
    assert_eq!(err, CoreError::WorkspaceIndexOutOfRange);
    let before = fixed.snapshot();
    act(&mut fixed, Action::MoveColumnToWorkspaceNoFocus(ws(0)));
    assert_eq!(fixed.snapshot(), before, "今のワークスペース");
    apply_err(&mut fixed, Command::WindowUnmapped(ExternalKey::new(99)));
    let mut empty = dynamic(0);
    let err = apply_err(&mut empty, Command::Action(Action::MoveColumnToWorkspaceNoFocus(ws(1))));
    assert_eq!(err, CoreError::NoFocusedWindow);
}

#[test]
fn a_floating_window_moves_alone_and_focus_goes_to_the_first_window() {
    let mut c = dynamic(2);
    act(&mut c, Action::ToggleFloating); // 12 は浮動
    act(&mut c, Action::MoveColumnToWorkspaceNoFocus(ws(5)));
    assert_eq!(shown(&c), 0);
    assert_eq!(focus_key(&c), Some(11));
    assert_eq!(workspaces(&c), [(1, vec![11]), (2, vec![12]), (3, vec![])]);
}
