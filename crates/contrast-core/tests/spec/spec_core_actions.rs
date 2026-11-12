//! core の Action 仕様（core-spec 暫定決定 D8〜D14 と 10 節）。
//! 出力は幅 900 × 高さ 600。窓はキー 11 から。`setup(900, n)` のフォーカスは最後に map した窓。
use contrast_core::{Action, Command, CoreError, Direction, Effect, WindowState};

use crate::common::{
    apply_err, apply_noop, apply_ok, dialog, key, kinds, new_core, out, output_added, rect,
    rect_of, setup, state_of, visible, win, ws,
};

fn act(a: Action) -> Command {
    Command::Action(a)
}

#[test]
fn focus_direction_moves_to_the_nearest_window_in_that_direction() {
    let mut c = setup(900, 3);
    apply_ok(&mut c, act(Action::FocusDirection(Direction::Left)));
    assert_eq!(c.view().focus(), Some(win(&c, 12)));
    apply_ok(&mut c, act(Action::FocusDirection(Direction::Left)));
    assert_eq!(c.view().focus(), Some(win(&c, 11)));
    apply_ok(&mut c, act(Action::FocusDirection(Direction::Right)));
    assert_eq!(c.view().focus(), Some(win(&c, 12)));
}

#[test]
fn focus_direction_without_a_candidate_changes_nothing() {
    let mut c = setup(900, 3);
    for d in [Direction::Right, Direction::Up, Direction::Down] {
        apply_noop(&mut c, act(Action::FocusDirection(d)));
    }
    apply_ok(&mut c, Command::WindowFocusRequested(key(11)));
    apply_noop(&mut c, act(Action::FocusDirection(Direction::Left)));
}

#[test]
fn move_window_swaps_with_the_next_tiled_window() {
    let mut c = setup(900, 3);
    apply_ok(&mut c, Command::WindowFocusRequested(key(11)));
    let a = apply_ok(&mut c, act(Action::MoveWindow(Direction::Right)));
    let (w1, w2, o) = (win(&c, 11), win(&c, 12), out(&c, 1));
    assert_eq!(rect_of(&c, 11), Some(rect(300, 0, 300, 600)));
    assert_eq!(rect_of(&c, 12), Some(rect(0, 0, 300, 600)));
    assert_eq!(rect_of(&c, 13), Some(rect(600, 0, 300, 600)));
    assert_eq!(
        c.view().focus(),
        Some(w1),
        "フォーカスは動かした窓についていく"
    );
    assert_eq!(
        a.effects,
        vec![
            Effect::Configure {
                window: w1,
                rect: rect(300, 0, 300, 600),
                state: WindowState::Tiled
            },
            Effect::Configure {
                window: w2,
                rect: rect(0, 0, 300, 600),
                state: WindowState::Tiled
            },
            Effect::Damage(o),
        ]
    );
}

#[test]
fn move_window_up_is_previous_and_down_is_next() {
    let mut c = setup(900, 2);
    apply_ok(&mut c, act(Action::MoveWindow(Direction::Up)));
    assert_eq!(rect_of(&c, 12), Some(rect(0, 0, 450, 600)));
    assert_eq!(rect_of(&c, 11), Some(rect(450, 0, 450, 600)));
    apply_ok(&mut c, act(Action::MoveWindow(Direction::Down)));
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 450, 600)));
    assert_eq!(rect_of(&c, 12), Some(rect(450, 0, 450, 600)));
}

#[test]
fn move_window_at_the_edge_or_for_a_floating_window_changes_nothing() {
    let mut c = setup(900, 3);
    apply_noop(&mut c, act(Action::MoveWindow(Direction::Right)));
    apply_ok(&mut c, Command::WindowFocusRequested(key(11)));
    apply_noop(&mut c, act(Action::MoveWindow(Direction::Left)));

    let mut c = setup(900, 1);
    apply_ok(
        &mut c,
        Command::WindowMapped {
            key: key(12),
            meta: dialog(),
        },
    );
    apply_noop(&mut c, act(Action::MoveWindow(Direction::Right)));
}

#[test]
fn switching_workspace_hides_windows_and_restores_focus_without_configure() {
    let mut c = setup(900, 3);
    let a = apply_ok(&mut c, act(Action::SwitchWorkspace(ws(1))));
    assert_eq!(
        kinds(&a.effects),
        vec!["ShowWorkspace", "SetFocus", "Damage"]
    );
    assert_eq!(c.view().focus(), None);
    assert!(!visible(&c, 11) && !visible(&c, 12) && !visible(&c, 13));

    // 戻る: 窓の矩形と状態は変わらないので、Configure は出ない。
    let a = apply_ok(&mut c, act(Action::SwitchWorkspace(ws(0))));
    assert_eq!(
        kinds(&a.effects),
        vec!["ShowWorkspace", "SetFocus", "Damage"]
    );
    assert_eq!(c.view().focus(), Some(win(&c, 13)));
    assert!(visible(&c, 11) && visible(&c, 12) && visible(&c, 13));
}

#[test]
fn switching_workspace_edge_cases() {
    let mut c = setup(900, 1);
    apply_noop(&mut c, act(Action::SwitchWorkspace(ws(0))));
    assert_eq!(
        apply_err(&mut c, act(Action::SwitchWorkspace(ws(4)))),
        CoreError::WorkspaceIndexOutOfRange
    );
    // 出力が 1 台もないときは、何も起きない。
    let mut c = new_core(4);
    apply_noop(&mut c, act(Action::SwitchWorkspace(ws(1))));
}

#[test]
fn switching_to_a_workspace_shown_on_another_output_swaps_the_two() {
    let mut c = setup(900, 1);
    apply_ok(&mut c, output_added(2, 600));
    let (o1, o2) = (out(&c, 1), out(&c, 2));
    apply_ok(&mut c, act(Action::SwitchWorkspace(ws(1))));
    assert_eq!(c.view().shown_workspace(o1), Some(ws(1)));
    assert_eq!(c.view().shown_workspace(o2), Some(ws(0)));
}

#[test]
fn moving_to_a_workspace_leaves_focus_behind() {
    let mut c = setup(900, 3);
    let w13 = win(&c, 13);
    let a = apply_ok(&mut c, act(Action::MoveToWorkspace(ws(1))));
    assert_eq!(c.view().window(w13).map(|w| w.workspace), Some(ws(1)));
    assert!(!visible(&c, 13));
    assert_eq!(
        c.view().focus(),
        Some(win(&c, 11)),
        "元のワークスペースの先頭へ"
    );
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 450, 600)));
    assert_eq!(rect_of(&c, 12), Some(rect(450, 0, 450, 600)));
    assert!(
        !a.effects
            .iter()
            .any(|e| matches!(e, Effect::Configure { window, .. } if *window == w13))
    );
}

#[test]
fn moving_to_a_workspace_edge_cases() {
    let mut c = setup(900, 1);
    apply_noop(&mut c, act(Action::MoveToWorkspace(ws(0))));
    assert_eq!(
        apply_err(&mut c, act(Action::MoveToWorkspace(ws(9)))),
        CoreError::WorkspaceIndexOutOfRange
    );
    // フォーカスがないときも、先に範囲を検査する。
    let mut c = setup(900, 0);
    assert_eq!(
        apply_err(&mut c, act(Action::MoveToWorkspace(ws(9)))),
        CoreError::WorkspaceIndexOutOfRange
    );
    assert_eq!(
        apply_err(&mut c, act(Action::MoveToWorkspace(ws(1)))),
        CoreError::NoFocusedWindow
    );
}

#[test]
fn actions_that_need_a_window_fail_without_focus() {
    let mut c = setup(900, 0);
    for a in [
        Action::FocusDirection(Direction::Left),
        Action::MoveWindow(Direction::Left),
        Action::ToggleFloating,
        Action::ToggleFullscreen,
        Action::CloseFocused,
    ] {
        assert_eq!(apply_err(&mut c, act(a)), CoreError::NoFocusedWindow);
    }
}

#[test]
fn close_focused_only_requests_the_close() {
    let mut c = setup(900, 2);
    let w2 = win(&c, 12);
    let before = c.snapshot();
    let a = apply_ok(&mut c, act(Action::CloseFocused));
    assert_eq!(a.effects, vec![Effect::RequestClose(w2)]);
    assert_eq!(
        c.snapshot(),
        before,
        "窓の削除は WindowUnmapped が来たときに行う"
    );
}

#[test]
fn toggling_floating_centers_the_window_at_half_size_and_back() {
    let mut c = setup(900, 2);
    apply_ok(&mut c, act(Action::ToggleFloating));
    assert_eq!(state_of(&c, 12), WindowState::Floating);
    assert_eq!(rect_of(&c, 12), Some(rect(225, 150, 450, 300)));
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 900, 600)));
    apply_ok(&mut c, act(Action::ToggleFloating));
    assert_eq!(state_of(&c, 12), WindowState::Tiled);
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 450, 600)));
    assert_eq!(rect_of(&c, 12), Some(rect(450, 0, 450, 600)));
}

#[test]
fn toggling_floating_on_a_fullscreen_window_changes_nothing() {
    let mut c = setup(900, 1);
    apply_ok(&mut c, act(Action::ToggleFullscreen));
    apply_noop(&mut c, act(Action::ToggleFloating));
}

#[test]
fn only_one_window_per_workspace_is_fullscreen() {
    let mut c = setup(900, 2);
    apply_ok(&mut c, act(Action::ToggleFullscreen));
    assert_eq!(state_of(&c, 12), WindowState::Fullscreen);
    assert_eq!(rect_of(&c, 12), Some(rect(0, 0, 900, 600)));
    assert_eq!(
        rect_of(&c, 11),
        Some(rect(0, 0, 900, 600)),
        "残りの Tiled 窓は全幅を使う"
    );

    // 2 枚目を全画面にすると、1 枚目は元の状態に戻る。
    apply_ok(&mut c, Command::WindowFocusRequested(key(11)));
    apply_ok(&mut c, act(Action::ToggleFullscreen));
    assert_eq!(state_of(&c, 11), WindowState::Fullscreen);
    assert_eq!(state_of(&c, 12), WindowState::Tiled);

    // 解除すると、2 枚が並ぶ。
    apply_ok(&mut c, act(Action::ToggleFullscreen));
    assert_eq!(state_of(&c, 11), WindowState::Tiled);
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 450, 600)));
    assert_eq!(rect_of(&c, 12), Some(rect(450, 0, 450, 600)));
}

#[test]
fn leaving_fullscreen_restores_a_floating_window() {
    let mut c = setup(900, 1);
    apply_ok(&mut c, act(Action::ToggleFloating));
    apply_ok(&mut c, act(Action::ToggleFullscreen));
    assert_eq!(state_of(&c, 11), WindowState::Fullscreen);
    apply_ok(&mut c, act(Action::ToggleFullscreen));
    assert_eq!(state_of(&c, 11), WindowState::Floating);
}

#[test]
fn a_focus_request_moves_focus_only_to_a_visible_window() {
    let mut c = setup(900, 2);
    let (w1, o) = (win(&c, 11), out(&c, 1));
    let a = apply_ok(&mut c, Command::WindowFocusRequested(key(11)));
    assert_eq!(
        a.effects,
        vec![Effect::SetFocus(Some(w1)), Effect::Damage(o)]
    );

    // 非表示のワークスペースの窓には、フォーカスを移さない。
    apply_ok(&mut c, act(Action::MoveToWorkspace(ws(1))));
    apply_noop(&mut c, Command::WindowFocusRequested(key(11)));
}
