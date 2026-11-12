//! 任意の Command 列に対する性質（core-spec 4 節の I1〜I9 と 5 節の Effect の意味）。
//! 出力はキー 1〜3、窓はキー 11〜16。幅 1〜3 は AreaTooSmall のエラー経路を通すための値。
use contrast_core::{Action, Command, Direction, Effect, OutputName, WindowId, WindowState};
use proptest::prelude::*;

use crate::common::{TestCore, assert_effect_order, dialog, key, meta, new_core, rect, ws};

fn width() -> impl Strategy<Value = i32> {
    prop::sample::select(vec![1, 2, 3, 100, 900])
}

fn direction() -> impl Strategy<Value = Direction> {
    prop::sample::select(vec![
        Direction::Left,
        Direction::Right,
        Direction::Up,
        Direction::Down,
    ])
}

fn action() -> impl Strategy<Value = Action> {
    prop_oneof![
        direction().prop_map(Action::FocusDirection),
        direction().prop_map(Action::MoveWindow),
        (0u8..=4).prop_map(|n| Action::SwitchWorkspace(ws(n))),
        (0u8..=4).prop_map(|n| Action::MoveToWorkspace(ws(n))),
        Just(Action::ToggleFloating),
        Just(Action::ToggleFullscreen),
        Just(Action::CloseFocused),
    ]
}

fn command() -> impl Strategy<Value = Command> {
    prop_oneof![
        2 => (1u64..=3, width()).prop_map(|(k, w)| Command::OutputAdded {
            key: key(k),
            name: OutputName::new("out"),
            geometry: rect(0, 0, w, 400),
        }),
        1 => (1u64..=3).prop_map(|k| Command::OutputRemoved(key(k))),
        1 => (1u64..=3, width()).prop_map(|(k, w)| Command::OutputChanged {
            key: key(k),
            geometry: rect(0, 0, w, 400),
        }),
        4 => (11u64..=16, any::<bool>()).prop_map(|(k, d)| Command::WindowMapped {
            key: key(k),
            meta: if d { dialog() } else { meta() },
        }),
        2 => (11u64..=16).prop_map(|k| Command::WindowUnmapped(key(k))),
        1 => (11u64..=16).prop_map(|k| Command::WindowMetaChanged {
            key: key(k),
            meta: meta(),
        }),
        2 => (11u64..=16).prop_map(|k| Command::WindowFocusRequested(key(k))),
        6 => action().prop_map(Command::Action),
    ]
}

fn commands() -> impl Strategy<Value = Vec<Command>> {
    prop::collection::vec(command(), 0..60)
}

/// Configure と SetFocus が、適用後の状態と一致する。SetFocus は変わったときだけ出る。
fn assert_effects_match_state(c: &TestCore, focus_before: Option<WindowId>, effects: &[Effect]) {
    let view = c.view();
    let mut set_focus = None;
    for e in effects {
        match e {
            Effect::Configure {
                window,
                rect,
                state,
            } => {
                let w = view.window(*window).expect("Configure の窓は存在する");
                assert!(w.visible, "Configure の窓は表示中: {effects:?}");
                assert_eq!(w.rect, Some(*rect));
                assert_eq!(w.state, *state);
            }
            Effect::SetFocus(f) => {
                assert_ne!(*f, focus_before, "SetFocus は変わったときだけ: {effects:?}");
                set_focus = Some(*f);
            }
            _ => {}
        }
    }
    match set_focus {
        Some(f) => assert_eq!(view.focus(), f),
        None => assert_eq!(
            view.focus(),
            focus_before,
            "SetFocus なしでフォーカスが変わった"
        ),
    }
}

/// I9: 同じワークスペースの Fullscreen の窓は高々 1 枚。
fn assert_one_fullscreen_per_workspace(c: &TestCore) {
    let view = c.view();
    for i in 0..view.workspace_count() {
        let fullscreen = view.workspace_windows(ws(i)).map_or(0, |ids| {
            ids.iter()
                .filter(|id| {
                    view.window(**id)
                        .is_some_and(|w| w.state == WindowState::Fullscreen)
                })
                .count()
        });
        assert!(
            fullscreen <= 1,
            "ワークスペース {i} に Fullscreen が {fullscreen} 枚"
        );
    }
}

proptest! {
    /// 毎ステップで I1〜I6・I9、Effect の順序と意味が保たれ、Err なら状態が変わらない（I7）。
    #[test]
    fn every_step_keeps_the_invariants_and_describes_its_changes(cmds in commands()) {
        let mut c = new_core(4);
        for cmd in cmds {
            let before = c.snapshot();
            let focus_before = c.view().focus();
            match c.apply(cmd) {
                Ok(applied) => {
                    assert_effect_order(&applied.effects);
                    assert_effects_match_state(&c, focus_before, &applied.effects);
                }
                Err(_) => prop_assert_eq!(c.snapshot(), before),
            }
            prop_assert_eq!(c.check_invariants(), Ok(()));
            assert_one_fullscreen_per_workspace(&c);
        }
    }

    /// I8: 同じ Command 列は、新しい Core に流しても同じ結果と同じ状態になる。
    #[test]
    fn the_same_commands_give_the_same_results(cmds in commands()) {
        let (mut a, mut b) = (new_core(4), new_core(4));
        for cmd in cmds {
            prop_assert_eq!(a.apply(cmd.clone()), b.apply(cmd));
            prop_assert_eq!(a.snapshot(), b.snapshot());
        }
    }
}
