//! スクロール列レイアウトの実装テスト: 仕様テストが触れない端の挙動（設定、Floating への出入り、出力の変更、複数ワークスペース）。
//!
//! `check_invariants` は `invariants` feature で公開されるので、そのときだけ動かす。

/// feature の付け忘れで、実装テストが黙って 0 件になるのを防ぐ。
#[cfg(not(feature = "invariants"))]
#[test]
#[allow(
    clippy::assertions_on_constants,
    reason = "feature の付け忘れを、テストの失敗として知らせる"
)]
fn scrolling_impl_tests_need_the_invariants_feature() {
    assert!(
        cfg!(feature = "invariants"),
        "core の実装テストは --all-features（または --features invariants）で実行する"
    );
}

/// テスト一式を `cfg(test)` モジュールに入れる。
#[cfg(test)]
#[cfg(feature = "invariants")]
mod scrolling_impl {
    use contrast_core::{
        Action, ColumnWidth, Columns, Command, Core, CoreConfig, DefaultPolicy, Direction,
        ExternalKey, LayoutMode, OutputName, Ratio, Rect, WindowMeta, WindowState, WorkspaceIndex,
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

    /// 設定を指定した Scrolling の Core（出力 1 台 900x600、窓 `windows` 枚）。
    fn core_with(config: CoreConfig, windows: u64) -> TestCore {
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

    /// apply して不変条件を確かめる。
    fn apply(core: &mut TestCore, cmd: Command) {
        core.apply(cmd).expect("apply ok");
        core.check_invariants().expect("invariants hold");
    }

    /// 窓 `k` の矩形。
    fn rect_of(c: &TestCore, k: u64) -> Rect {
        let id = c.view().window_id(ExternalKey::new(k)).expect("window");
        c.view().window(id).and_then(|w| w.rect).expect("rect")
    }

    /// 窓 `k` の状態。
    fn state_of(c: &TestCore, k: u64) -> WindowState {
        let id = c.view().window_id(ExternalKey::new(k)).expect("window");
        c.view().window(id).expect("window").state
    }

    #[test]
    fn default_column_width_and_presets_can_be_configured() {
        let config = CoreConfig::new(4)
            .with_default_column_width(ratio(1, 3))
            .with_preset_column_widths(vec![ratio(1, 4), ratio(3, 4)]);
        let mut c = core_with(config, 1);
        assert_eq!(rect_of(&c, 11).width(), 300);
        // 300 より広い最初のプリセットは 3/4（675）。それより広いものはないので、次は先頭の 1/4（225）に戻る。
        for want in [675, 225, 675] {
            apply(&mut c, Command::Action(Action::SwitchPresetColumnWidth));
            assert_eq!(rect_of(&c, 11).width(), want);
        }
    }

    #[test]
    fn an_empty_preset_list_keeps_the_defaults() {
        let config = CoreConfig::new(4).with_preset_column_widths(Vec::new());
        let mut c = core_with(config, 1);
        apply(&mut c, Command::Action(Action::SwitchPresetColumnWidth));
        assert_eq!(rect_of(&c, 11).width(), 600);
    }

    #[test]
    fn a_proportion_column_follows_the_output_size() {
        let mut c = core_with(CoreConfig::new(4), 2);
        apply(
            &mut c,
            Command::OutputChanged {
                key: ExternalKey::new(1),
                geometry: rect(0, 0, 600, 600),
            },
        );
        assert_eq!(rect_of(&c, 11).width(), 300);
        assert_eq!(rect_of(&c, 12).x(), 300);
    }

    #[test]
    fn a_fixed_width_of_zero_built_by_hand_is_treated_as_one() {
        let mut c = core_with(CoreConfig::new(4), 1);
        apply(
            &mut c,
            Command::Action(Action::SetColumnWidth(ColumnWidth::Fixed(0))),
        );
        assert_eq!(rect_of(&c, 11).width(), 1);
    }

    #[test]
    fn setting_a_width_forgets_the_maximized_width() {
        let mut c = core_with(CoreConfig::new(4), 1);
        apply(&mut c, Command::Action(Action::MaximizeColumn));
        apply(
            &mut c,
            Command::Action(Action::SetColumnWidth(ColumnWidth::Proportion(ratio(1, 3)))),
        );
        apply(&mut c, Command::Action(Action::MaximizeColumn));
        assert_eq!(rect_of(&c, 11).width(), 900);
        apply(&mut c, Command::Action(Action::MaximizeColumn));
        assert_eq!(rect_of(&c, 11).width(), 300);
    }

    #[test]
    fn a_window_returning_from_floating_opens_a_column_right_of_the_focused_one() {
        let mut c = core_with(CoreConfig::new(4), 3);
        apply(&mut c, Command::Action(Action::ToggleFloating));
        assert_eq!(state_of(&c, 13), WindowState::Floating);
        apply(&mut c, Command::Action(Action::FocusColumnFirst));
        apply(
            &mut c,
            Command::Action(Action::FocusDirection(Direction::Right)),
        );
        // 12 がフォーカス。13 を Tiled に戻すには、13 にフォーカスを戻す必要がある。
        let id = c.view().window_id(ExternalKey::new(13)).expect("window");
        let key = c.view().window_key(id).expect("key");
        apply(&mut c, Command::WindowFocusRequested(key));
        apply(&mut c, Command::Action(Action::ToggleFloating));
        assert_eq!(state_of(&c, 13), WindowState::Tiled);
        // 並びは 11・12・13。13 は 12 の右隣（フォーカス履歴で列にある最後の窓は 12）。
        assert!(rect_of(&c, 12).x() < rect_of(&c, 13).x());
        assert!(rect_of(&c, 11).x() < rect_of(&c, 12).x());
    }

    #[test]
    fn moving_a_window_to_another_workspace_removes_its_column() {
        let mut c = core_with(CoreConfig::new(4), 3);
        apply(
            &mut c,
            Command::Action(Action::MoveToWorkspace(WorkspaceIndex::new(1))),
        );
        // 13 は別のワークスペースへ。11・12 の列は残る。
        assert_eq!(rect_of(&c, 11).x(), 0);
        assert_eq!(rect_of(&c, 12).x(), 450);
        apply(
            &mut c,
            Command::Action(Action::SwitchWorkspace(WorkspaceIndex::new(1))),
        );
        assert_eq!(rect_of(&c, 13), rect(0, 0, 450, 600));
    }

    #[test]
    fn the_view_of_a_workspace_is_kept_when_switching_away_and_back() {
        let mut c = core_with(CoreConfig::new(4), 3);
        assert_eq!(rect_of(&c, 11).x(), -450);
        apply(
            &mut c,
            Command::Action(Action::SwitchWorkspace(WorkspaceIndex::new(1))),
        );
        apply(
            &mut c,
            Command::Action(Action::SwitchWorkspace(WorkspaceIndex::new(0))),
        );
        assert_eq!(rect_of(&c, 11).x(), -450);
        assert_eq!(rect_of(&c, 13).x(), 450);
    }

    #[test]
    fn fullscreen_takes_a_window_out_of_its_column_and_back_in() {
        let mut c = core_with(CoreConfig::new(4), 2);
        apply(&mut c, Command::Action(Action::ToggleFullscreen));
        assert_eq!(state_of(&c, 12), WindowState::Fullscreen);
        assert_eq!(rect_of(&c, 12), rect(0, 0, 900, 600));
        assert_eq!(rect_of(&c, 11), rect(0, 0, 450, 600));
        apply(&mut c, Command::Action(Action::ToggleFullscreen));
        assert_eq!(state_of(&c, 12), WindowState::Tiled);
        assert_eq!(rect_of(&c, 12), rect(450, 0, 450, 600));
    }

    #[test]
    fn too_many_windows_in_a_column_for_the_height_is_rejected_without_a_change() {
        let mut c = core_with(CoreConfig::new(4), 0);
        apply(
            &mut c,
            Command::OutputChanged {
                key: ExternalKey::new(1),
                geometry: rect(0, 0, 900, 2),
            },
        );
        for k in 11..=13 {
            apply(
                &mut c,
                Command::WindowMapped {
                    key: ExternalKey::new(k),
                    meta: WindowMeta::default(),
                },
            );
        }
        apply(&mut c, Command::Action(Action::FocusColumnFirst));
        apply(&mut c, Command::Action(Action::ConsumeWindowIntoColumn));
        // 高さ 2 に 3 枚は積めない: 3 枚目を取り込む操作は Err で、状態は変わらない。
        let before = c.snapshot();
        assert!(
            c.apply(Command::Action(Action::ConsumeWindowIntoColumn))
                .is_err()
        );
        assert_eq!(c.snapshot(), before);
    }

    /// 窓 `k` にフォーカスがあるか。
    fn focus_is(c: &TestCore, k: u64) -> bool {
        c.view().focus() == c.view().window_id(ExternalKey::new(k))
    }

    fn act(c: &mut TestCore, a: Action) {
        apply(c, Command::Action(a));
    }

    #[test]
    fn closing_the_focused_column_goes_right_even_if_the_window_order_says_otherwise() {
        // 列の並びは 12・11・13、窓の順序列は 11・12・13。11 を閉じると、D41 では右の列の 13 へ。
        let mut c = core_with(CoreConfig::new(4), 3);
        act(&mut c, Action::FocusColumnFirst);
        act(&mut c, Action::MoveWindow(Direction::Right));
        assert!(focus_is(&c, 11));
        apply(&mut c, Command::WindowUnmapped(ExternalKey::new(11)));
        assert!(focus_is(&c, 13));
    }

    #[test]
    fn closing_a_window_in_a_stack_goes_to_the_one_below_by_column_order() {
        // 列 [12, 11, 13]（11 は真ん中）。11 を閉じると、下の 13 へ（順序列では 12 が隣）。
        let mut c = core_with(CoreConfig::new(4), 3);
        act(&mut c, Action::FocusColumnFirst);
        act(&mut c, Action::ConsumeWindowIntoColumn);
        act(&mut c, Action::ConsumeWindowIntoColumn);
        act(&mut c, Action::MoveWindow(Direction::Down));
        assert!(focus_is(&c, 11));
        assert_eq!(rect_of(&c, 12).y(), 0);
        apply(&mut c, Command::WindowUnmapped(ExternalKey::new(11)));
        assert!(focus_is(&c, 13));
    }

    #[test]
    fn focusing_left_returns_to_the_window_last_focused_in_that_column() {
        let mut c = core_with(CoreConfig::new(4), 3);
        act(&mut c, Action::FocusColumnFirst);
        act(&mut c, Action::ConsumeWindowIntoColumn); // 列 [11, 12] と 13
        act(&mut c, Action::FocusDirection(Direction::Down));
        assert!(focus_is(&c, 12));
        act(&mut c, Action::FocusDirection(Direction::Right));
        assert!(focus_is(&c, 13));
        // 幾何の規則なら（中心間の距離が同じで）11 になるが、列の規則では最後にフォーカスした 12。
        act(&mut c, Action::FocusDirection(Direction::Left));
        assert!(focus_is(&c, 12));
    }

    #[test]
    fn moving_a_window_down_at_the_bottom_of_a_stack_changes_nothing() {
        let mut c = core_with(CoreConfig::new(4), 2);
        act(&mut c, Action::FocusColumnFirst);
        act(&mut c, Action::ConsumeWindowIntoColumn); // 列 [11, 12]、フォーカス 11
        act(&mut c, Action::FocusDirection(Direction::Down));
        let before = c.snapshot();
        let applied = c
            .apply(Command::Action(Action::MoveWindow(Direction::Down)))
            .expect("apply ok");
        assert!(applied.effects.is_empty());
        assert_eq!(c.snapshot(), before);
        act(&mut c, Action::MoveWindow(Direction::Up));
        assert_eq!(rect_of(&c, 12).y(), 0);
        assert_eq!(rect_of(&c, 11).y(), 300);
        let before = c.snapshot();
        c.apply(Command::Action(Action::MoveWindow(Direction::Up)))
            .expect("apply ok");
        assert_eq!(c.snapshot(), before);
    }
}
