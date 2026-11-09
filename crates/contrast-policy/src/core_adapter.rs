//! `port::Policy` を contrast-core の `Policy` として使うアダプタ。
//!
//! コンポジタの `Core` はこれを通して窓の配置をポリシー（今は `StaticPolicy`、あとで CDL 由来）に問い合わせる。
//! ポリシーは外部入力に等しいので、範囲外のワークスペースは `None`（フォーカス出力の表示ワークスペース）
//! として扱い、core のエラーにしない。

use std::rc::Rc;

use contrast_core::{Placement, PlacementContext, WindowMeta, WorkspaceIndex};

use crate::port::Policy;

/// `port::Policy` を共有して持ち、`contrast_core::Policy` を実装する。
pub struct PortPlacement<P: ?Sized> {
    /// 問い合わせ先のポリシー（コンポジタのキー処理などと共有する）。
    port: Rc<P>,
}

impl<P: ?Sized> PortPlacement<P> {
    /// ポリシーを共有して作る。
    pub fn new(port: Rc<P>) -> Self {
        Self { port }
    }
}

impl<P: Policy + ?Sized> contrast_core::Policy for PortPlacement<P> {
    fn place(&self, meta: &WindowMeta, ctx: &PlacementContext<'_>) -> Placement {
        let focused = ctx.focused_workspace().map(WorkspaceIndex::get);
        let decided = self.port.place_window(meta, focused);
        let workspace = decided
            .workspace
            .filter(|n| *n < ctx.workspace_count())
            .map(WorkspaceIndex::new);
        let placement = Placement::new(workspace, decided.state, decided.focus);
        // 窓に決めさせる幅（niri の `default-column-width {}`）: シェルが窓の描いた大きさを `requested_size` に入れて渡す。
        // core の列の幅は隙間を含むので、窓の幅に隙間を足す（niri の列の幅は隙間を含まない）。
        match decided.column_width {
            Some(crate::PlacementWidth::Window) => meta
                .requested_size
                .and_then(|size| {
                    let gap = self.port.layout_settings().gap.max(0);
                    contrast_core::ColumnWidth::fixed(size.width().saturating_add(gap))
                })
                .map_or(placement, |w| placement.with_column_width(w)),
            None => placement,
        }
    }
}

#[cfg(test)]
mod tests {
    use contrast_core::{DefaultPolicy, Policy as _, WindowState};

    use super::*;
    use crate::StaticPolicy;

    fn meta(is_dialog: bool, app_id: Option<&str>) -> WindowMeta {
        let mut m = WindowMeta::default();
        m.is_dialog = is_dialog;
        m.app_id = app_id.map(str::to_string);
        m
    }

    #[test]
    fn static_policy_matches_default_policy() {
        let adapter = PortPlacement::new(Rc::new(StaticPolicy::new("foot")));
        let core_default = DefaultPolicy::new();
        for n_ws in [1u8, 4, 9] {
            let ctx = PlacementContext::new(Some(WorkspaceIndex::new(0)), n_ws, 1);
            for m in [
                meta(false, None),
                meta(true, None),
                meta(false, Some("foot")),
            ] {
                assert_eq!(adapter.place(&m, &ctx), core_default.place(&m, &ctx));
            }
        }
        let dialog = adapter.place(&meta(true, None), &PlacementContext::new(None, 4, 0));
        assert_eq!(dialog.state, WindowState::Floating);
    }

    /// 範囲外のワークスペースを返すポリシー。
    struct OutOfRange;

    impl Policy for OutOfRange {
        fn background(&self) -> [f32; 4] {
            [0.0; 4]
        }
        fn border_width(&self) -> i32 {
            0
        }
        fn focus_border_color(&self) -> [f32; 4] {
            [0.0; 4]
        }
        fn workspace_count(&self) -> u8 {
            4
        }
        fn layout(&self, area: contrast_core::Rect, count: usize) -> Vec<contrast_core::Rect> {
            vec![area; count]
        }
        fn place_window(&self, _meta: &WindowMeta, _ws: Option<u8>) -> crate::WindowPlacement {
            crate::WindowPlacement {
                workspace: Some(7),
                state: WindowState::Tiled,
                focus: false,
                fullscreen: false,
                column_width: None,
            }
        }
        fn resolve_key(&self, _mods: crate::Mods, _keysym: &str) -> Option<crate::Action> {
            None
        }
    }

    #[test]
    fn out_of_range_workspace_becomes_none() {
        let adapter = PortPlacement::new(Rc::new(OutOfRange));
        let p = adapter.place(&meta(false, None), &PlacementContext::new(None, 4, 1));
        assert_eq!(p.workspace, None);
        assert!(!p.focus);
        let p = adapter.place(&meta(false, None), &PlacementContext::new(None, 9, 1));
        assert_eq!(p.workspace, Some(WorkspaceIndex::new(7)));
    }
}
