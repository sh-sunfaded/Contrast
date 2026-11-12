//! 作業領域の余白（Insets）の仕様（core-spec 11 節、D30〜D34）。
//! 出力は幅 900 × 高さ 600（原点 (0, 0)）。余白は (top, right, bottom, left) の順。
use contrast_core::{Action, Command, CoreError, Insets, LayoutError};
use proptest::prelude::*;

use crate::common::{
    apply_err, apply_noop, apply_ok, assert_effect_order, dialog, key, kinds, meta, new_core,
    output_added, rect, rect_of, setup, ws,
};

fn insets(top: i32, right: i32, bottom: i32, left: i32) -> Insets {
    Insets::new(top, right, bottom, left).expect("負でない余白")
}

fn set_insets(k: u64, i: Insets) -> Command {
    Command::OutputInsetsChanged {
        key: key(k),
        insets: i,
    }
}

#[test]
fn insets_must_not_be_negative() {
    assert!(Insets::new(-1, 0, 0, 0).is_none());
    assert!(Insets::new(0, -1, 0, 0).is_none());
    assert!(Insets::new(0, 0, -1, 0).is_none());
    assert!(Insets::new(0, 0, 0, -1).is_none());
    let i = insets(1, 2, 3, 4);
    assert_eq!((i.top(), i.right(), i.bottom(), i.left()), (1, 2, 3, 4));
    let zero = Insets::default();
    assert_eq!(
        (zero.top(), zero.right(), zero.bottom(), zero.left()),
        (0, 0, 0, 0)
    );
}

#[test]
fn a_top_inset_shrinks_the_work_area_of_tiled_windows() {
    let mut c = setup(900, 2);
    let a = apply_ok(&mut c, set_insets(1, insets(30, 0, 0, 0)));
    assert_eq!(rect_of(&c, 11), Some(rect(0, 30, 450, 570)));
    assert_eq!(rect_of(&c, 12), Some(rect(450, 30, 450, 570)));
    assert_eq!(kinds(&a.effects), vec!["Configure", "Configure", "Damage"]);
}

#[test]
fn side_insets_move_and_narrow_the_columns() {
    let mut c = setup(900, 2);
    apply_ok(&mut c, set_insets(1, insets(0, 50, 0, 100)));
    // 作業領域は x = 100、幅 750、高さ 600。
    assert_eq!(rect_of(&c, 11), Some(rect(100, 0, 375, 600)));
    assert_eq!(rect_of(&c, 12), Some(rect(475, 0, 375, 600)));
}

#[test]
fn removing_the_insets_restores_the_full_output() {
    let mut c = setup(900, 1);
    apply_ok(&mut c, set_insets(1, insets(30, 0, 40, 0)));
    assert_eq!(rect_of(&c, 11), Some(rect(0, 30, 900, 530)));
    apply_ok(&mut c, set_insets(1, Insets::default()));
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 900, 600)));
}

#[test]
fn setting_the_same_insets_changes_nothing() {
    let mut c = setup(900, 2);
    apply_noop(&mut c, set_insets(1, Insets::default()));
    apply_ok(&mut c, set_insets(1, insets(30, 0, 0, 0)));
    apply_noop(&mut c, set_insets(1, insets(30, 0, 0, 0)));
}

#[test]
fn insets_that_leave_no_area_are_an_error() {
    for windows in [0, 1] {
        let mut c = setup(900, windows);
        let too_small = CoreError::Layout(LayoutError::AreaTooSmall);
        assert_eq!(
            apply_err(&mut c, set_insets(1, insets(300, 0, 300, 0))),
            too_small
        );
        assert_eq!(
            apply_err(&mut c, set_insets(1, insets(0, 450, 0, 450))),
            too_small
        );
    }
    // ちょうど 1 px 残る余白は受け付ける。
    let mut c = setup(900, 1);
    apply_ok(&mut c, set_insets(1, insets(300, 0, 299, 0)));
    assert_eq!(rect_of(&c, 11), Some(rect(0, 300, 900, 1)));
}

#[test]
fn insets_for_an_unknown_output_are_rejected() {
    let mut c = setup(900, 1);
    assert_eq!(
        apply_err(&mut c, set_insets(99, insets(1, 0, 0, 0))),
        CoreError::UnknownKey
    );
}

#[test]
fn floating_windows_are_placed_in_the_work_area() {
    let mut c = setup(900, 0);
    apply_ok(&mut c, set_insets(1, insets(100, 0, 0, 0)));
    apply_ok(
        &mut c,
        Command::WindowMapped {
            key: key(11),
            meta: dialog(),
        },
    );
    // 作業領域は (0, 100, 900, 500)。半分の 450 × 250 を中央に置く。
    assert_eq!(rect_of(&c, 11), Some(rect(225, 225, 450, 250)));
}

#[test]
fn fullscreen_ignores_the_insets() {
    let mut c = setup(900, 1);
    apply_ok(&mut c, set_insets(1, insets(30, 0, 0, 0)));
    apply_ok(&mut c, Command::Action(Action::ToggleFullscreen));
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 900, 600)));
}

#[test]
fn output_changes_keep_the_insets() {
    let mut c = setup(900, 1);
    apply_ok(&mut c, set_insets(1, insets(30, 0, 0, 0)));
    apply_ok(
        &mut c,
        Command::OutputChanged {
            key: key(1),
            geometry: rect(0, 0, 1000, 700),
        },
    );
    assert_eq!(rect_of(&c, 11), Some(rect(0, 30, 1000, 670)));
    // 余白が残る高さを下回る矩形は、エラー（状態は変わらない）。
    assert_eq!(
        apply_err(
            &mut c,
            Command::OutputChanged {
                key: key(1),
                geometry: rect(0, 0, 1000, 30),
            }
        ),
        CoreError::Layout(LayoutError::AreaTooSmall)
    );
}

#[test]
fn insets_belong_to_one_output() {
    let mut c = setup(900, 1);
    apply_ok(&mut c, output_added(2, 600));
    apply_ok(&mut c, set_insets(1, insets(30, 0, 0, 0)));
    // 窓を、余白のない出力 2（ワークスペース 1）へ移す。
    apply_ok(&mut c, Command::Action(Action::MoveToWorkspace(ws(1))));
    assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 600, 600)));
}

fn op() -> impl Strategy<Value = Command> {
    prop_oneof![
        3 => (11u64..=14, any::<bool>()).prop_map(|(k, d)| Command::WindowMapped {
            key: key(k),
            meta: if d { dialog() } else { meta() },
        }),
        2 => (11u64..=14).prop_map(|k| Command::WindowUnmapped(key(k))),
        4 => (0i32..=700, 0i32..=500, 0i32..=700, 0i32..=500)
            .prop_map(|(t, r, b, l)| set_insets(1, insets(t, r, b, l))),
        1 => prop::sample::select(vec![1, 2, 3, 100, 900]).prop_map(|w| Command::OutputChanged {
            key: key(1),
            geometry: rect(0, 0, w, 600),
        }),
        1 => Just(Command::Action(Action::ToggleFullscreen)),
        1 => Just(Command::Action(Action::ToggleFloating)),
    ]
}

proptest! {
    /// 余白・窓・出力の変更をどう混ぜても、不変条件（I5 を含む）が保たれ、Err のとき状態が変わらない。
    #[test]
    fn insets_changes_keep_the_invariants(ops in prop::collection::vec(op(), 0..50)) {
        let mut c = new_core(4);
        c.apply(output_added(1, 900)).expect("出力を足す");
        for cmd in ops {
            let before = c.snapshot();
            match c.apply(cmd) {
                Ok(applied) => assert_effect_order(&applied.effects),
                Err(_) => prop_assert_eq!(c.snapshot(), before),
            }
            prop_assert_eq!(c.check_invariants(), Ok(()));
        }
    }
}
