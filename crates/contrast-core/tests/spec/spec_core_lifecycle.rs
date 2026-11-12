//! core のライフサイクル仕様: 出力と窓の追加・削除・変更と Effect 列（core-spec 3〜7 節と 10 節）。
//! 出力は幅 900 × 高さ 600（原点 (0, 0)）。Columns は窓を map の順に等分し、余りは先頭から 1 pxずつ配る。
use contrast_core::{
    Action, Columns, Command, Core, CoreConfig, CoreError, Effect, EntityId, LayoutError,
    Placement, PlacementContext, Policy, WindowMeta, WindowState,
};

use crate::common::{
    apply_err, apply_ok, dialog, key, kinds, meta, new_core, out, output_added, rect, rect_of,
    setup, state_of, visible, win, window_mapped, ws,
};

#[test]
fn the_first_window_fills_the_output_and_takes_focus() {
    let mut c = setup(900, 0);
    let a = apply_ok(&mut c, window_mapped(11));
    let (w, o) = (win(&c, 11), out(&c, 1));
    assert_eq!(
        a.effects,
        vec![
            Effect::Registered {
                key: key(11),
                id: EntityId::Window(w)
            },
            Effect::Configure {
                window: w,
                rect: rect(0, 0, 900, 600),
                state: WindowState::Tiled
            },
            Effect::SetFocus(Some(w)),
            Effect::Damage(o),
        ]
    );
    assert_eq!(c.view().focus(), Some(w));
}

#[test]
fn windows_share_the_output_equally_in_map_order() {
    let mut c = setup(900, 2);
    let a = apply_ok(&mut c, window_mapped(13));
    let (w1, w2, w3, o) = (win(&c, 11), win(&c, 12), win(&c, 13), out(&c, 1));
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 300, 600)));
    assert_eq!(rect_of(&c, 12), Some(rect(300, 0, 300, 600)));
    assert_eq!(rect_of(&c, 13), Some(rect(600, 0, 300, 600)));
    assert_eq!(c.view().focus(), Some(w3));
    assert_eq!(
        a.effects,
        vec![
            Effect::Registered {
                key: key(13),
                id: EntityId::Window(w3)
            },
            Effect::Configure {
                window: w1,
                rect: rect(0, 0, 300, 600),
                state: WindowState::Tiled
            },
            Effect::Configure {
                window: w2,
                rect: rect(300, 0, 300, 600),
                state: WindowState::Tiled
            },
            Effect::Configure {
                window: w3,
                rect: rect(600, 0, 300, 600),
                state: WindowState::Tiled
            },
            Effect::SetFocus(Some(w3)),
            Effect::Damage(o),
        ]
    );
}

#[test]
fn unknown_keys_are_rejected_without_changing_state() {
    let mut c = setup(900, 1);
    for cmd in [
        Command::OutputRemoved(key(99)),
        Command::OutputChanged {
            key: key(99),
            geometry: rect(0, 0, 500, 500),
        },
        Command::WindowUnmapped(key(99)),
        Command::WindowMetaChanged {
            key: key(99),
            meta: meta(),
        },
        Command::WindowFocusRequested(key(99)),
    ] {
        assert_eq!(apply_err(&mut c, cmd), CoreError::UnknownKey);
    }
}

#[test]
fn duplicate_keys_are_rejected_without_changing_state() {
    let mut c = setup(900, 1);
    assert_eq!(
        apply_err(&mut c, output_added(1, 500)),
        CoreError::DuplicateKey
    );
    assert_eq!(
        apply_err(&mut c, window_mapped(11)),
        CoreError::DuplicateKey
    );
}

#[test]
fn an_output_needs_a_free_workspace() {
    let mut c = new_core(1);
    apply_ok(&mut c, output_added(1, 900));
    assert_eq!(
        apply_err(&mut c, output_added(2, 900)),
        CoreError::NoFreeWorkspace
    );
}

#[test]
fn too_many_windows_for_the_width_is_a_layout_error() {
    let mut c = setup(2, 2);
    assert_eq!(
        apply_err(&mut c, window_mapped(13)),
        CoreError::Layout(LayoutError::AreaTooSmall)
    );
    assert!(c.view().window_id(key(13)).is_none());
}

#[test]
fn shrinking_an_output_below_the_window_count_fails_and_keeps_the_old_geometry() {
    let mut c = setup(3, 3);
    let err = apply_err(
        &mut c,
        Command::OutputChanged {
            key: key(1),
            geometry: rect(0, 0, 2, 600),
        },
    );
    assert_eq!(err, CoreError::Layout(LayoutError::AreaTooSmall));
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 1, 600)));
}

#[test]
fn windows_without_an_output_wait_for_one() {
    let mut c = new_core(4);
    let a = apply_ok(&mut c, window_mapped(11));
    let w = win(&c, 11);
    assert_eq!(
        a.effects,
        vec![Effect::Registered {
            key: key(11),
            id: EntityId::Window(w)
        }]
    );
    assert_eq!(rect_of(&c, 11), None);
    assert!(!visible(&c, 11));
    assert_eq!(c.view().focus(), None);

    let a = apply_ok(&mut c, output_added(1, 900));
    assert_eq!(
        kinds(&a.effects),
        vec!["Registered", "ShowWorkspace", "Configure", "Damage"]
    );
    assert!(matches!(
        &a.effects[2],
        Effect::Configure { window, rect: r, .. } if *window == w && *r == rect(0, 0, 900, 600)
    ));
    assert_eq!(
        c.view().focus(),
        None,
        "OutputAdded ではフォーカスを与えない"
    );
}

/// どんな窓にも同じ配置を返すポリシー。
struct Fixed(Placement);

impl Policy for Fixed {
    fn place(&self, _meta: &WindowMeta, _ctx: &PlacementContext<'_>) -> Placement {
        self.0
    }
}

fn with_policy(p: Placement) -> Core<Fixed, Columns> {
    let mut c = Core::new(CoreConfig::new(4), Fixed(p), Columns::new());
    apply_ok(&mut c, output_added(1, 900));
    c
}

#[test]
fn the_policy_chooses_workspace_and_focus() {
    // 非表示のワークスペースに置く: Registered だけが出て、フォーカスは移らない。
    let mut c = with_policy(Placement::new(Some(ws(1)), WindowState::Tiled, true));
    let a = apply_ok(&mut c, window_mapped(11));
    assert_eq!(kinds(&a.effects), vec!["Registered"]);
    assert_eq!(c.view().focus(), None);
    assert!(!visible(&c, 11));

    // focus = false: 表示中のワークスペースに置かれるが、フォーカスは移らない。
    let mut c = with_policy(Placement::new(None, WindowState::Tiled, false));
    let a = apply_ok(&mut c, window_mapped(11));
    assert_eq!(kinds(&a.effects), vec!["Registered", "Configure", "Damage"]);
    assert_eq!(c.view().focus(), None);

    // 範囲外のワークスペース: エラー。
    let mut c = with_policy(Placement::new(Some(ws(200)), WindowState::Tiled, true));
    assert_eq!(
        apply_err(&mut c, window_mapped(11)),
        CoreError::WorkspaceIndexOutOfRange
    );
}

#[test]
fn a_dialog_opens_floating_and_centered_at_half_size() {
    let mut c = setup(900, 1);
    apply_ok(
        &mut c,
        Command::WindowMapped {
            key: key(12),
            meta: dialog(),
        },
    );
    assert_eq!(state_of(&c, 12), WindowState::Floating);
    assert_eq!(rect_of(&c, 12), Some(rect(225, 150, 450, 300)));
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 900, 600)));
}

#[test]
fn the_second_output_shows_the_next_free_workspace() {
    let mut c = setup(900, 0);
    apply_ok(&mut c, output_added(2, 600));
    assert_eq!(c.view().shown_workspace(out(&c, 1)), Some(ws(0)));
    assert_eq!(c.view().shown_workspace(out(&c, 2)), Some(ws(1)));
}

#[test]
fn removing_an_output_keeps_its_windows_and_moves_focus() {
    // 出力 2 のワークスペース 1 が空のとき: 窓は残って見えなくなり、フォーカスは無くなる。
    let mut c = setup(900, 2);
    apply_ok(&mut c, output_added(2, 600));
    let o1 = out(&c, 1);
    let a = apply_ok(&mut c, Command::OutputRemoved(key(1)));
    assert!(c.view().output_id(key(1)).is_none());
    assert!(!visible(&c, 11) && !visible(&c, 12));
    assert_eq!(
        c.view().workspace_windows(ws(0)).map(|ids| ids.len()),
        Some(2)
    );
    assert_eq!(c.view().focus(), None);
    assert!(a.effects.contains(&Effect::SetFocus(None)));
    assert!(!a.effects.contains(&Effect::Damage(o1)));

    // 別の出力が窓を表示していれば、その先頭の窓へ移る。
    let mut c = setup(900, 2);
    apply_ok(&mut c, output_added(2, 600));
    apply_ok(&mut c, Command::Action(Action::MoveToWorkspace(ws(1))));
    apply_ok(&mut c, Command::OutputRemoved(key(1)));
    assert_eq!(c.view().focus(), Some(win(&c, 12)));
}

#[test]
fn a_removed_output_key_can_be_added_again_with_a_new_id() {
    let mut c = setup(900, 0);
    let first = out(&c, 1);
    apply_ok(&mut c, Command::OutputRemoved(key(1)));
    apply_ok(&mut c, output_added(1, 900));
    let second = out(&c, 1);
    assert_ne!(first, second);
    assert_eq!(c.view().shown_workspace(second), Some(ws(0)));
}

#[test]
fn meta_changes_do_not_move_or_restyle_windows() {
    let mut c = setup(900, 2);
    let before = (rect_of(&c, 11), state_of(&c, 11));
    let a = apply_ok(
        &mut c,
        Command::WindowMetaChanged {
            key: key(11),
            meta: dialog(),
        },
    );
    assert!(a.effects.is_empty());
    assert_eq!((rect_of(&c, 11), state_of(&c, 11)), before);
}

#[test]
fn unmapping_the_focused_window_focuses_a_neighbour() {
    // 中央の窓: 右隣へ。残りの窓は並び直す。
    let mut c = setup(900, 3);
    apply_ok(&mut c, Command::WindowFocusRequested(key(12)));
    let a = apply_ok(&mut c, Command::WindowUnmapped(key(12)));
    let w13 = win(&c, 13);
    assert_eq!(c.view().focus(), Some(w13));
    assert!(a.effects.contains(&Effect::SetFocus(Some(w13))));
    assert!(c.view().window_id(key(12)).is_none());
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 450, 600)));
    assert_eq!(rect_of(&c, 13), Some(rect(450, 0, 450, 600)));

    // 末尾の窓: 左隣へ。
    let mut c = setup(900, 3);
    apply_ok(&mut c, Command::WindowUnmapped(key(13)));
    assert_eq!(c.view().focus(), Some(win(&c, 12)));

    // 最後の 1 枚: フォーカスは無くなる。
    let mut c = setup(900, 1);
    let a = apply_ok(&mut c, Command::WindowUnmapped(key(11)));
    assert_eq!(c.view().focus(), None);
    assert!(a.effects.contains(&Effect::SetFocus(None)));
}

#[test]
fn unmapping_another_window_keeps_focus() {
    let mut c = setup(900, 3);
    let w13 = win(&c, 13);
    let a = apply_ok(&mut c, Command::WindowUnmapped(key(11)));
    assert_eq!(c.view().focus(), Some(w13));
    assert!(!a.effects.iter().any(|e| matches!(e, Effect::SetFocus(_))));
}
