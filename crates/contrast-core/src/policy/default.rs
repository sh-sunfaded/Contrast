//! 既定ポリシー: ダイアログは Floating、それ以外は Tiled。配置先はフォーカス出力、フォーカスを与える。

use crate::domain::window::{WindowMeta, WindowState};
use crate::policy::{Placement, PlacementContext, Policy};

/// 既定のポリシー。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DefaultPolicy;

impl DefaultPolicy {
    /// `DefaultPolicy` を作る。
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Policy for DefaultPolicy {
    fn place(&self, meta: &WindowMeta, _ctx: &PlacementContext<'_>) -> Placement {
        let state = if meta.is_dialog {
            WindowState::Floating
        } else {
            WindowState::Tiled
        };
        Placement::new(None, state, true)
    }
}
