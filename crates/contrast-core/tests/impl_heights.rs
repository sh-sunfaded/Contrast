//! 窓の高さと center-column（core-spec 14 節）の実装テスト: 仕様テストが触れない端の挙動
//! （設定のプリセット、不正な Fixed、フォーカスなし、Floating、窓の入れ替え、出力の変更、ワークスペースの切り替え）。
//!
//! `check_invariants` は `invariants` feature で公開されるので、そのときだけ動かす。

/// feature の付け忘れで、実装テストが黙って 0 件になるのを防ぐ。
#[cfg(not(feature = "invariants"))]
#[test]
#[allow(
    clippy::assertions_on_constants,
    reason = "feature の付け忘れを、テストの失敗として知らせる"
)]
fn heights_impl_tests_need_the_invariants_feature() {
    assert!(
        cfg!(feature = "invariants"),
        "core の実装テストは --all-features（または --features invariants）で実行する"
    );
}

/// テスト一式を `cfg(test)` モジュールに入れる。
#[cfg(test)]
#[cfg(feature = "invariants")]
mod heights_impl {
    use contrast_core::{
        Action, ColumnWidth, Columns, Command, Core, CoreConfig, CoreError, DefaultPolicy,
        Direction, ExternalKey, LayoutMode, OutputName, Ratio, Rect, WindowHeight, WindowMeta,
        WorkspaceIndex,
    };

    /// テスト対象の Core。
    type TestCore = Core<DefaultPolicy, Columns>;

    /// 矩形を作る。
    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect::new(x, y, w, h).expect("valid rect")
    }

    /// 比を作る。
    fn ratio(n: u32, d: u32) -> Ratio {
        Ratio::new(n, d).expect("valid ratio")
    }

    /// 割合の高さ。
    fn proportion(n: u32, d: u32) -> WindowHeight {
        WindowHeight::Proportion(ratio(n, d))
    }

    /// Action の Command。
    fn act(a: Action) -> Command {
        Command::Action(a)
    }

    /// Command を適用し、不変条件を確かめる。Effect を返す。
    fn apply(c: &mut TestCore, cmd: Command) -> Vec<contrast_core::Effect> {
        let applied = c.apply(cmd).expect("apply");
        c.check_invariants().expect("invariants");
        applied.effects
    }

    /// 出力（幅 900、高さ 600）に `windows` 枚の窓（キー 11 から）を置いた、スクロール列レイアウトの Core。
    fn scrolling_with(config: CoreConfig, windows: u64) -> TestCore {
        let mut c = Core::new(
            config.with_layout(LayoutMode::Scrolling),
            DefaultPolicy::new(),
            Columns::new(),
        );
        apply(
            &mut c,
            Command::OutputAdded {
                key: ExternalKey::new(1),
                name: OutputName::new("out"),
                geometry: rect(0, 0, 900, 600),
            },
        );
        for i in 0..windows {
            apply(
                &mut c,
                Command::WindowMapped {
                    key: ExternalKey::new(11_u64.saturating_add(i)),
                    meta: WindowMeta::default(),
                },
            );
        }
        c
    }

    /// 窓を n 枚、1 つの列に積む。フォーカスは先頭（11）。
    fn stacked(config: CoreConfig, n: u64) -> TestCore {
        let mut c = scrolling_with(config, n);
        apply(&mut c, act(Action::FocusColumnFirst));
        for _ in 1..n {
            apply(&mut c, act(Action::ConsumeWindowIntoColumn));
        }
        c
    }

    /// 窓 `k` の矩形。
    fn rect_of(c: &TestCore, k: u64) -> Option<Rect> {
        let view = c.view();
        let id = view.window_id(ExternalKey::new(k))?;
        view.window(id)?.rect
    }

    /// 窓 `k` の高さ。
    fn height_of(c: &TestCore, k: u64) -> i32 {
        rect_of(c, k).expect("rect").height()
    }

    #[test]
    fn custom_preset_heights_are_used_and_wrap() {
        let config = CoreConfig::new(4).with_preset_window_heights(vec![ratio(1, 4)]);
        let mut c = stacked(config, 2);
        // 今の高さは 300（Auto）。プリセットは 150 だけなので、先頭へ戻る。
        apply(&mut c, act(Action::SwitchPresetWindowHeight));
        assert_eq!((height_of(&c, 11), height_of(&c, 12)), (150, 450));
        // 150 より高いプリセットがなければ、また先頭（同じ高さ）。
        apply(&mut c, act(Action::SwitchPresetWindowHeight));
        assert_eq!(height_of(&c, 11), 150);
    }

    #[test]
    fn an_empty_preset_list_keeps_the_default_presets() {
        let config = CoreConfig::new(4).with_preset_window_heights(Vec::new());
        let mut c = stacked(config, 2);
        apply(&mut c, act(Action::SwitchPresetWindowHeight));
        assert_eq!(height_of(&c, 11), 400);
    }

    #[test]
    fn an_invalid_fixed_height_is_treated_as_one_pixel() {
        let mut c = stacked(CoreConfig::new(4), 2);
        apply(&mut c, act(Action::SetWindowHeight(WindowHeight::Fixed(0))));
        assert_eq!((height_of(&c, 11), height_of(&c, 12)), (1, 599));
        apply(
            &mut c,
            act(Action::SetWindowHeight(WindowHeight::Fixed(-5))),
        );
        assert_eq!(height_of(&c, 11), 1);
    }

    #[test]
    fn a_single_window_column_always_fills_the_height() {
        let mut c = scrolling_with(CoreConfig::new(4), 1);
        for h in [
            proportion(1, 3),
            WindowHeight::Fixed(10),
            WindowHeight::Auto,
        ] {
            apply(&mut c, act(Action::SetWindowHeight(h)));
            assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 450, 600)));
        }
        apply(&mut c, act(Action::SwitchPresetWindowHeight));
        assert_eq!(height_of(&c, 11), 600);
    }

    #[test]
    fn without_a_focused_window_the_actions_fail_and_change_nothing() {
        let mut c = scrolling_with(CoreConfig::new(4), 0);
        for a in [
            Action::SetWindowHeight(proportion(1, 2)),
            Action::SwitchPresetWindowHeight,
            Action::ResetWindowHeight,
            Action::CenterColumn,
        ] {
            let before = c.snapshot();
            assert_eq!(c.apply(act(a)).map(|_| ()), Err(CoreError::NoFocusedWindow));
            assert_eq!(c.snapshot(), before);
        }
    }

    #[test]
    fn a_floating_focus_window_is_not_in_a_column_so_nothing_happens() {
        let mut c = scrolling_with(CoreConfig::new(4), 2);
        apply(&mut c, act(Action::ToggleFloating));
        for a in [
            Action::SetWindowHeight(proportion(1, 2)),
            Action::SwitchPresetWindowHeight,
            Action::ResetWindowHeight,
            Action::CenterColumn,
        ] {
            let before = c.snapshot();
            assert!(apply(&mut c, act(a)).is_empty());
            assert_eq!(c.snapshot(), before);
        }
    }

    #[test]
    fn a_height_follows_its_window_when_the_order_in_the_column_changes() {
        let mut c = stacked(CoreConfig::new(4), 2);
        apply(&mut c, act(Action::SetWindowHeight(proportion(1, 3))));
        assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 450, 200)));
        apply(&mut c, act(Action::MoveWindow(Direction::Down)));
        // 11 は下へ移った。高さ 200 はついていく。
        assert_eq!(rect_of(&c, 12), Some(rect(0, 0, 450, 400)));
        assert_eq!(rect_of(&c, 11), Some(rect(0, 400, 450, 200)));
    }

    #[test]
    fn heights_survive_other_columns_and_switching_workspaces() {
        let mut c = stacked(CoreConfig::new(4), 2);
        apply(&mut c, act(Action::SetWindowHeight(proportion(1, 3))));
        let before = (rect_of(&c, 11), rect_of(&c, 12));
        apply(&mut c, act(Action::SwitchWorkspace(WorkspaceIndex::new(1))));
        apply(&mut c, act(Action::SwitchWorkspace(WorkspaceIndex::new(0))));
        assert_eq!((rect_of(&c, 11), rect_of(&c, 12)), before);
    }

    #[test]
    fn a_window_moved_to_another_workspace_gets_an_auto_height() {
        let mut c = stacked(CoreConfig::new(4), 2);
        apply(&mut c, act(Action::SetWindowHeight(proportion(1, 3))));
        apply(&mut c, act(Action::MoveToWorkspace(WorkspaceIndex::new(1))));
        apply(&mut c, act(Action::SwitchWorkspace(WorkspaceIndex::new(1))));
        assert_eq!(rect_of(&c, 11), Some(rect(0, 0, 450, 600)));
    }

    #[test]
    fn a_window_that_returns_from_floating_gets_an_auto_height() {
        let mut c = stacked(CoreConfig::new(4), 2);
        apply(&mut c, act(Action::SetWindowHeight(proportion(1, 3))));
        apply(&mut c, act(Action::ToggleFloating));
        apply(&mut c, act(Action::ToggleFloating));
        // 11 は新しい列として戻る。高さは Auto（列に 1 枚）で、12 の列は 1 枚になる。
        assert_eq!(height_of(&c, 11), 600);
        assert_eq!(height_of(&c, 12), 600);
    }

    #[test]
    fn proportional_heights_follow_the_output_height() {
        let mut c = stacked(CoreConfig::new(4), 2);
        apply(&mut c, act(Action::SetWindowHeight(proportion(1, 3))));
        apply(
            &mut c,
            Command::OutputChanged {
                key: ExternalKey::new(1),
                geometry: rect(0, 0, 900, 900),
            },
        );
        assert_eq!((height_of(&c, 11), height_of(&c, 12)), (300, 600));
    }

    #[test]
    fn too_many_windows_for_the_height_is_an_error_and_changes_nothing() {
        let mut c = Core::new(
            CoreConfig::new(4).with_layout(LayoutMode::Scrolling),
            DefaultPolicy::new(),
            Columns::new(),
        );
        apply(
            &mut c,
            Command::OutputAdded {
                key: ExternalKey::new(1),
                name: OutputName::new("tiny"),
                geometry: rect(0, 0, 900, 2),
            },
        );
        for k in 11..=13 {
            let _ = c.apply(Command::WindowMapped {
                key: ExternalKey::new(k),
                meta: WindowMeta::default(),
            });
        }
        apply(&mut c, act(Action::FocusColumnFirst));
        apply(&mut c, act(Action::ConsumeWindowIntoColumn));
        // 高さ 2 に 3 枚は積めない。積もうとすると Err で、状態は変わらない。
        let before = c.snapshot();
        assert!(c.apply(act(Action::ConsumeWindowIntoColumn)).is_err());
        assert_eq!(c.snapshot(), before);
    }

    #[test]
    fn centering_a_column_wider_than_the_area_aligns_its_left_edge() {
        let mut c = scrolling_with(CoreConfig::new(4), 2);
        apply(
            &mut c,
            act(Action::SetColumnWidth(
                ColumnWidth::fixed(1200).expect("1 以上"),
            )),
        );
        // 幅 1200 > 作業領域 900: D39 の規則で左端が合う。センタリングで動かない。
        let before = c.snapshot();
        assert!(apply(&mut c, act(Action::CenterColumn)).is_empty());
        assert_eq!(c.snapshot(), before);
    }

    #[test]
    fn centering_is_stable_when_repeated() {
        let mut c = scrolling_with(CoreConfig::new(4), 3);
        apply(&mut c, act(Action::FocusDirection(Direction::Left)));
        apply(&mut c, act(Action::CenterColumn));
        let centered = rect_of(&c, 12);
        assert!(apply(&mut c, act(Action::CenterColumn)).is_empty());
        assert_eq!(rect_of(&c, 12), centered);
        // 同じ列に留まる操作（高さの変更）では、ビューが動かない。
        apply(&mut c, act(Action::SetWindowHeight(proportion(1, 2))));
        assert_eq!(rect_of(&c, 12).map(|r| r.x()), centered.map(|r| r.x()));
    }
}
