//! 差分ミュータント（`cargo mutants --in-diff`）が見逃した箇所を殺すテスト。
//!
//! 公開 API だけを使い、各テストは「その変異で結果が変わる最小の入力」を組む。
//! 対象: 全画面の降格（D9）の呼び分け（`move_to_workspace`・`move_column_to_workspace`・
//! `move_column_to_monitor`）、`StateView::workspace_output`、`output_removed` の元の出力の探索、
//! `view_gesture_end` の DnD・フォーカス移動の条件。
//!
//! `check_invariants` は `invariants` feature で公開されるので、そのときだけ動かす。

/// feature の付け忘れで、実装テストが黙って 0 件になるのを防ぐ。
#[cfg(not(feature = "invariants"))]
#[test]
#[allow(
    clippy::assertions_on_constants,
    reason = "feature の付け忘れを、テストの失敗として知らせる"
)]
fn mutant_killers_need_the_invariants_feature() {
    assert!(
        cfg!(feature = "invariants"),
        "core の実装テストは --all-features（または --features invariants）で実行する"
    );
}

/// テスト一式を `cfg(test)` モジュールに入れる。
#[cfg(test)]
#[cfg(feature = "invariants")]
mod mutant_killers {
    use contrast_core::{
        Action, ColumnWidth, Columns, Command, Core, CoreConfig, DefaultPolicy, Direction,
        ExternalKey, LayoutMode, OutputName, Rect, WindowMeta, WindowState, WorkspaceIndex,
        WorkspaceMode,
    };

    /// テスト対象の Core。
    type TestCore = Core<DefaultPolicy, Columns>;

    /// 窓の最初のキー。
    const FIRST: u64 = 11;

    /// 矩形を作る。
    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect::new(x, y, w, h).expect("valid rect")
    }

    /// apply して不変条件を確かめる。
    fn apply(c: &mut TestCore, cmd: Command) {
        c.apply(cmd).expect("apply ok");
        c.check_invariants().expect("invariants hold");
    }

    /// Action を適用する。
    fn act(c: &mut TestCore, a: Action) {
        apply(c, Command::Action(a));
    }

    /// 出力を足す（900x600。`x` は左端）。
    fn add_output(c: &mut TestCore, key: u64, name: &str, x: i32) {
        apply(
            c,
            Command::OutputAdded {
                key: ExternalKey::new(key),
                name: OutputName::new(name),
                geometry: rect(x, 0, 900, 600),
            },
        );
    }

    /// 窓を開く。
    fn open(c: &mut TestCore, k: u64) {
        apply(
            c,
            Command::WindowMapped {
                key: ExternalKey::new(k),
                meta: WindowMeta::default(),
            },
        );
    }

    /// 窓を開いて、列の幅を `w` px にする（開いた窓がフォーカスされている前提）。
    fn open_wide(c: &mut TestCore, k: u64, w: i32) {
        open(c, k);
        act(
            c,
            Action::SetColumnWidth(ColumnWidth::fixed(w).expect("width")),
        );
    }

    /// 出力 1 台（key 1）の Core。
    fn core_with_output(mode: LayoutMode, workspaces: WorkspaceMode) -> TestCore {
        let mut c = Core::new(
            CoreConfig::new(4)
                .with_layout(mode)
                .with_workspace_mode(workspaces),
            DefaultPolicy::new(),
            Columns::new(),
        );
        add_output(&mut c, 1, "out", 0);
        c
    }

    /// 窓 `k` の状態。
    fn state_of(c: &TestCore, k: u64) -> WindowState {
        let id = c.view().window_id(ExternalKey::new(k)).expect("window");
        c.view().window(id).expect("window").state
    }

    /// フォーカス窓のキー（窓のキーは 11 以上 100 未満）。
    fn focus_key(c: &TestCore) -> Option<u64> {
        let want = c.view().window_key(c.view().focus()?)?;
        (FIRST..100).find(|k| ExternalKey::new(*k) == want)
    }

    /// 窓 `k` の左端。
    fn x_of(c: &TestCore, k: u64) -> i32 {
        let id = c.view().window_id(ExternalKey::new(k)).expect("window");
        c.view().window(id).and_then(|w| w.rect).expect("rect").x()
    }

    fn ws(n: u8) -> WorkspaceIndex {
        WorkspaceIndex::new(n)
    }

    // ---- 全画面の降格（D9）の呼び分け ------------------------------------------------

    /// ワークスペース 1 に全画面の窓 13、ワークスペース 0 に窓 11・12（フォーカスは 12）の Core（`Fixed`）。
    fn fullscreen_on_workspace_1(mode: LayoutMode) -> TestCore {
        let mut c = core_with_output(mode, WorkspaceMode::Fixed);
        open(&mut c, 11);
        open(&mut c, 12);
        act(&mut c, Action::SwitchWorkspace(ws(1)));
        open(&mut c, 13);
        act(&mut c, Action::ToggleFullscreen);
        assert_eq!(state_of(&c, 13), WindowState::Fullscreen);
        act(&mut c, Action::SwitchWorkspace(ws(0)));
        assert_eq!(focus_key(&c), Some(12));
        c
    }

    #[test]
    fn move_to_workspace_keeps_the_targets_fullscreen_for_a_tiled_window() {
        let mut c = fullscreen_on_workspace_1(LayoutMode::Equal);
        act(&mut c, Action::MoveToWorkspace(ws(1)));
        // 全画面でない窓を移しても、移し先の全画面の窓は降格しない。
        assert_eq!(state_of(&c, 12), WindowState::Tiled);
        assert_eq!(state_of(&c, 13), WindowState::Fullscreen);
    }

    #[test]
    fn move_to_workspace_demotes_the_targets_fullscreen_for_a_fullscreen_window() {
        let mut c = fullscreen_on_workspace_1(LayoutMode::Equal);
        act(&mut c, Action::ToggleFullscreen);
        assert_eq!(state_of(&c, 12), WindowState::Fullscreen);
        act(&mut c, Action::MoveToWorkspace(ws(1)));
        // 全画面の窓を移すと、移し先にあった全画面の窓は降格する（D9）。
        assert_eq!(state_of(&c, 12), WindowState::Fullscreen);
        assert_eq!(state_of(&c, 13), WindowState::Tiled);
    }

    #[test]
    fn move_column_to_workspace_keeps_the_targets_fullscreen_for_a_tiled_column() {
        let mut c = fullscreen_on_workspace_1(LayoutMode::Scrolling);
        act(&mut c, Action::MoveColumnToWorkspace(ws(1)));
        assert_eq!(state_of(&c, 12), WindowState::Tiled);
        assert_eq!(state_of(&c, 13), WindowState::Fullscreen);
    }

    #[test]
    fn move_column_to_workspace_demotes_the_targets_fullscreen_for_a_fullscreen_window() {
        let mut c = fullscreen_on_workspace_1(LayoutMode::Scrolling);
        act(&mut c, Action::ToggleFullscreen);
        act(&mut c, Action::MoveColumnToWorkspace(ws(1)));
        assert_eq!(state_of(&c, 12), WindowState::Fullscreen);
        assert_eq!(state_of(&c, 13), WindowState::Tiled);
    }

    /// 出力 A（key 1）に窓 11・12（フォーカスは 12）、右隣の出力 B（key 2）に全画面の窓 13 の Core（`Dynamic`・`Scrolling`）。
    fn fullscreen_on_the_right_monitor() -> TestCore {
        let mut c = core_with_output(LayoutMode::Scrolling, WorkspaceMode::Dynamic);
        add_output(&mut c, 2, "right", 900);
        open(&mut c, 11);
        open(&mut c, 12);
        apply(&mut c, Command::OutputFocusRequested(ExternalKey::new(2)));
        open(&mut c, 13);
        act(&mut c, Action::ToggleFullscreen);
        assert_eq!(state_of(&c, 13), WindowState::Fullscreen);
        apply(&mut c, Command::OutputFocusRequested(ExternalKey::new(1)));
        assert_eq!(focus_key(&c), Some(12));
        c
    }

    #[test]
    fn move_column_to_monitor_keeps_the_targets_fullscreen_for_a_tiled_column() {
        let mut c = fullscreen_on_the_right_monitor();
        act(&mut c, Action::MoveColumnToMonitor(Direction::Right));
        assert_eq!(state_of(&c, 12), WindowState::Tiled);
        assert_eq!(state_of(&c, 13), WindowState::Fullscreen);
    }

    #[test]
    fn move_column_to_monitor_demotes_the_targets_fullscreen_for_a_fullscreen_window() {
        let mut c = fullscreen_on_the_right_monitor();
        act(&mut c, Action::ToggleFullscreen);
        act(&mut c, Action::MoveColumnToMonitor(Direction::Right));
        assert_eq!(state_of(&c, 12), WindowState::Fullscreen);
        assert_eq!(state_of(&c, 13), WindowState::Tiled);
    }

    // ---- StateView::workspace_output ------------------------------------------------

    #[test]
    fn workspace_output_is_the_shown_output_in_fixed_mode() {
        let c = core_with_output(LayoutMode::Equal, WorkspaceMode::Fixed);
        let out = c.view().output_id(ExternalKey::new(1)).expect("output");
        let shown = c.view().workspace_id(ws(0)).expect("workspace 0");
        let hidden = c.view().workspace_id(ws(1)).expect("workspace 1");
        assert_eq!(c.view().workspace_output(shown), Some(out));
        assert_eq!(c.view().workspace_output(hidden), None);
    }

    #[test]
    fn workspace_output_is_the_owner_in_dynamic_mode() {
        let mut c = core_with_output(LayoutMode::Scrolling, WorkspaceMode::Dynamic);
        add_output(&mut c, 2, "right", 900);
        let a = c.view().output_id(ExternalKey::new(1)).expect("a");
        let b = c.view().output_id(ExternalKey::new(2)).expect("b");
        for (owner, other) in [(a, b), (b, a)] {
            let list = c.view().output_workspaces(owner);
            assert!(!list.is_empty());
            for w in list {
                assert_eq!(c.view().workspace_output(w), Some(owner));
                assert_ne!(c.view().workspace_output(w), Some(other));
            }
        }
    }

    // ---- output_removed: 元の出力の探索（D77） ----------------------------------------

    #[test]
    fn removed_outputs_workspace_goes_to_the_output_with_its_original_name() {
        // 出力 P（主）と、同じ名前 "B" の出力 2 台。窓のあるワークスペースの元の出力は "B"。
        let mut c = core_with_output(LayoutMode::Scrolling, WorkspaceMode::Dynamic);
        add_output(&mut c, 2, "B", 900);
        add_output(&mut c, 3, "B", 1800);
        let p = c.view().output_id(ExternalKey::new(1)).expect("p");
        let b1 = c.view().output_id(ExternalKey::new(2)).expect("b1");
        let b2 = c.view().output_id(ExternalKey::new(3)).expect("b2");
        apply(&mut c, Command::OutputFocusRequested(ExternalKey::new(2)));
        open(&mut c, 11);
        let with_window = c
            .view()
            .output_workspaces(b1)
            .into_iter()
            .find(|w| {
                c.view()
                    .workspace_windows_by_id(*w)
                    .is_some_and(|l| !l.is_empty())
            })
            .expect("workspace with the window");
        apply(&mut c, Command::OutputRemoved(ExternalKey::new(2)));
        // 元の出力の名前と同じ出力（b2）へ。主の出力（p）ではない。
        assert_eq!(c.view().workspace_output(with_window), Some(b2));
        assert!(c.view().output_workspaces(b2).contains(&with_window));
        assert!(!c.view().output_workspaces(p).contains(&with_window));
    }

    // ---- view_gesture_end ------------------------------------------------------------

    /// 3 列（幅 600）。出力は 900 幅なので、ビューは右端（900）で、フォーカスは最後の列（窓 13）。
    fn three_columns() -> TestCore {
        let mut c = core_with_output(LayoutMode::Scrolling, WorkspaceMode::Fixed);
        for k in 11..=13 {
            open_wide(&mut c, k, 600);
        }
        assert_eq!(focus_key(&c), Some(13));
        c
    }

    fn begin(c: &mut TestCore, output: u64, dnd: bool) {
        apply(
            c,
            Command::ViewGestureBegin {
                output: ExternalKey::new(output),
                dnd,
            },
        );
    }

    fn update(c: &mut TestCore, offset: i32) {
        apply(c, Command::ViewGestureUpdate { offset });
    }

    fn end(c: &mut TestCore, projected_offset: i32) {
        apply(c, Command::ViewGestureEnd { projected_offset });
    }

    #[test]
    fn a_dnd_gesture_that_moved_snaps_like_a_normal_one() {
        let mut c = three_columns();
        begin(&mut c, 1, true);
        update(&mut c, -300);
        end(&mut c, -900);
        // 動いた DnD は吸着する: 左端の列にフォーカスが移り、ビューは 0。
        assert_eq!(focus_key(&c), Some(11));
        assert_eq!(x_of(&c, 11), 0);
    }

    #[test]
    fn a_dnd_gesture_that_did_not_move_keeps_the_view() {
        let mut c = three_columns();
        let before = x_of(&c, 13);
        begin(&mut c, 1, true);
        end(&mut c, -900);
        // 動かなかった DnD は、投影の値を使わず、ビューもフォーカスも変えない。
        assert_eq!(focus_key(&c), Some(13));
        assert_eq!(x_of(&c, 13), before);
    }

    #[test]
    fn a_gesture_on_a_workspace_of_another_output_does_not_move_the_focus() {
        // 出力 A（key 1）は空、出力 B（key 2）に 3 列。アクティブな出力は A で、フォーカスは None
        // （`Fixed` では、フォーカス出力が窓のある出力になるので、`Dynamic` で組む）。
        let mut c = core_with_output(LayoutMode::Scrolling, WorkspaceMode::Dynamic);
        add_output(&mut c, 2, "right", 900);
        apply(&mut c, Command::OutputFocusRequested(ExternalKey::new(2)));
        for k in 11..=13 {
            open_wide(&mut c, k, 600);
        }
        apply(&mut c, Command::OutputFocusRequested(ExternalKey::new(1)));
        assert_eq!(c.view().focus(), None);
        let focus_before = c.view().focus();
        // 吸着先は左端の列（窓 11）。フォーカスの移り先がフォーカス窓と違うので、移れば分かる。
        begin(&mut c, 2, false);
        update(&mut c, -600);
        end(&mut c, -900);
        // アクティブでない出力のジェスチャーは、core のフォーカスを移さない。
        assert_eq!(c.view().focus(), focus_before);
    }

    #[test]
    fn a_gesture_does_not_take_the_focus_from_a_floating_window() {
        let mut c = three_columns();
        // フォーカス窓 13 を Floating にする（窓 11・12 は Tiled のまま）。
        act(&mut c, Action::ToggleFloating);
        assert_eq!(state_of(&c, 13), WindowState::Floating);
        assert_eq!(focus_key(&c), Some(13));
        begin(&mut c, 1, false);
        update(&mut c, 300);
        end(&mut c, -900);
        // フォーカスが Tiled の窓でないときは、core のフォーカスを移さない。
        assert_eq!(focus_key(&c), Some(13));
    }
}
