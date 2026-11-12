//! 窓を足すときの配置の実装テスト: 全画面の窓との関係（D9・D26）。
//!
//! `check_invariants` は `invariants` feature で公開されるので、そのときだけ動かす。

/// feature の付け忘れで、実装テストが黙って 0 件になるのを防ぐ。
#[cfg(not(feature = "invariants"))]
#[test]
#[allow(
    clippy::assertions_on_constants,
    reason = "feature の付け忘れを、テストの失敗として知らせる"
)]
fn placement_impl_tests_need_the_invariants_feature() {
    assert!(
        cfg!(feature = "invariants"),
        "core の実装テストは --all-features（または --features invariants）で実行する"
    );
}

/// テスト一式を `cfg(test)` モジュールに入れる。
#[cfg(test)]
#[cfg(feature = "invariants")]
mod placement_impl {
    use contrast_core::{
        Action, Columns, Command, Core, CoreConfig, DefaultPolicy, ExternalKey, OutputName,
        Placement, PlacementContext, Policy, Rect, WindowMeta, WindowState,
    };

    /// 新しい窓を全画面で置くポリシー。
    struct FullscreenPolicy;

    impl Policy for FullscreenPolicy {
        fn place(&self, _meta: &WindowMeta, _ctx: &PlacementContext<'_>) -> Placement {
            Placement::new(None, WindowState::Fullscreen, true)
        }
    }

    /// 矩形を作る。
    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect::new(x, y, w, h).expect("valid rect")
    }

    /// 出力 1 台（900x600）を足した Core。
    fn core_with_output<P: Policy>(policy: P) -> Core<P, Columns> {
        let mut c = Core::new(CoreConfig::new(4), policy, Columns::new());
        c.apply(Command::OutputAdded {
            key: ExternalKey::new(1),
            name: OutputName::new("out"),
            geometry: rect(0, 0, 900, 600),
        })
        .expect("output added");
        c
    }

    /// 窓 `k` を足す。
    fn map<P: Policy>(c: &mut Core<P, Columns>, k: u64) {
        c.apply(Command::WindowMapped {
            key: ExternalKey::new(k),
            meta: WindowMeta::default(),
        })
        .expect("window mapped");
        c.check_invariants().expect("invariants hold");
    }

    /// 窓 `k` の状態。
    fn state_of<P: Policy>(c: &Core<P, Columns>, k: u64) -> WindowState {
        let id = c.view().window_id(ExternalKey::new(k)).expect("window");
        c.view().window(id).expect("window").state
    }

    #[test]
    fn a_new_tiled_window_does_not_end_an_existing_fullscreen() {
        let mut c = core_with_output(DefaultPolicy::new());
        map(&mut c, 11);
        c.apply(Command::Action(Action::ToggleFullscreen))
            .expect("fullscreen");
        map(&mut c, 12);
        assert_eq!(state_of(&c, 11), WindowState::Fullscreen);
        assert_eq!(state_of(&c, 12), WindowState::Tiled);
    }

    #[test]
    fn a_new_fullscreen_window_ends_the_existing_fullscreen() {
        let mut c = core_with_output(FullscreenPolicy);
        map(&mut c, 11);
        assert_eq!(state_of(&c, 11), WindowState::Fullscreen);
        map(&mut c, 12);
        assert_eq!(state_of(&c, 12), WindowState::Fullscreen);
        assert_eq!(state_of(&c, 11), WindowState::Tiled);
    }
}
