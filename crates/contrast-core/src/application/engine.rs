//! `Core` 本体と `CoreConfig`。
//!
//! 守る不変条件:
//! - I7 `apply` は複製した State だけを変え、Err なら元の State を一切変えない。
//! - I8 `apply` は State と Command だけから決まる（時刻・乱数・反復順の不定なコレクションを使わない）。
//! - I1〜I6・I9 は各 handler と `arrange` が保ち、`check_invariants` で検査できる。

use alloc::vec::Vec;

use crate::application::command::Command;
use crate::application::effect::Applied;
use crate::application::view::StateView;
use crate::application::{arrange, diff, handlers, scrolling};
use crate::domain::column::{LayoutMode, LayoutSettings, MAX_PRESETS, Ratio, WorkspaceMode};
use crate::domain::state::State;
use crate::error::CoreError;
use crate::layout::Layout;
use crate::policy::Policy;

/// `Core` の設定。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreConfig {
    /// ワークスペースの数（1〜32。範囲外は `Core::new` が丸める）。
    pub workspace_count: u8,
    /// 窓の並べ方（D35。既定 `Equal`）。
    layout: LayoutMode,
    /// ワークスペースの方式（D62。既定 `Fixed`）。
    workspaces: WorkspaceMode,
    /// 新しい列の幅（D35。既定 1/2）。
    default_column_width: Ratio,
    /// プリセットの列幅（先頭から詰める。残りは `None`）。
    preset_column_widths: [Option<Ratio>; MAX_PRESETS],
    /// プリセットの窓の高さ（D49。先頭から詰める。残りは `None`）。
    preset_window_heights: [Option<Ratio>; MAX_PRESETS],
}

impl CoreConfig {
    /// ワークスペース数を指定して作る。
    #[must_use]
    pub const fn new(workspace_count: u8) -> Self {
        Self {
            workspace_count,
            layout: LayoutSettings::DEFAULT.mode,
            workspaces: LayoutSettings::DEFAULT.workspaces,
            default_column_width: LayoutSettings::DEFAULT.default_width,
            preset_column_widths: LayoutSettings::DEFAULT.presets,
            preset_window_heights: LayoutSettings::DEFAULT.window_presets,
        }
    }

    /// ワークスペースの方式を指定する（D62）。`Dynamic` のとき `workspace_count` は無視される。
    #[must_use]
    pub const fn with_workspace_mode(mut self, workspaces: WorkspaceMode) -> Self {
        self.workspaces = workspaces;
        self
    }

    /// 窓の並べ方を指定する（D35）。
    #[must_use]
    pub const fn with_layout(mut self, layout: LayoutMode) -> Self {
        self.layout = layout;
        self
    }

    /// 新しい列の幅を指定する（D35）。
    #[must_use]
    pub const fn with_default_column_width(mut self, width: Ratio) -> Self {
        self.default_column_width = width;
        self
    }

    /// プリセットの列幅を指定する（D35）。空なら既定のまま。`MAX_PRESETS`（8）を超える分は使わない。
    #[must_use]
    pub fn with_preset_column_widths(mut self, presets: Vec<Ratio>) -> Self {
        if presets.is_empty() {
            return self;
        }
        let mut slots = [None; MAX_PRESETS];
        for (slot, ratio) in slots.iter_mut().zip(presets) {
            *slot = Some(ratio);
        }
        self.preset_column_widths = slots;
        self
    }

    /// プリセットの窓の高さを指定する（D49）。空なら既定のまま。`MAX_PRESETS`（8）を超える分は使わない。
    #[must_use]
    pub fn with_preset_window_heights(mut self, presets: Vec<Ratio>) -> Self {
        if presets.is_empty() {
            return self;
        }
        let mut slots = [None; MAX_PRESETS];
        for (slot, ratio) in slots.iter_mut().zip(presets) {
            *slot = Some(ratio);
        }
        self.preset_window_heights = slots;
        self
    }

    /// `State` が持つ設定にする。
    fn layout_settings(&self) -> LayoutSettings {
        LayoutSettings {
            workspaces: self.workspaces,
            mode: self.layout,
            default_width: self.default_column_width,
            presets: self.preset_column_widths,
            window_presets: self.preset_window_heights,
        }
    }
}

impl Default for CoreConfig {
    /// 既定はワークスペース 9 個。
    fn default() -> Self {
        Self::new(9)
    }
}

/// Contrast の決定的な状態遷移エンジン。
#[derive(Debug, Clone)]
pub struct Core<P: Policy, L: Layout> {
    /// 現在の状態。
    state: State,
    /// 新しい窓の配置の判断。
    policy: P,
    /// Tiled 窓の配置計算。
    layout: L,
}

impl<P: Policy, L: Layout> Core<P, L> {
    /// 空の状態（出力・窓なし、ワークスペース `workspace_count` 個）で作る。
    #[must_use]
    pub fn new(config: CoreConfig, policy: P, layout: L) -> Self {
        // `Dynamic` は空のワークスペース 1 個から始まる（D62）。
        let count = match config.workspaces {
            WorkspaceMode::Dynamic => 1,
            _ => config.workspace_count,
        };
        let mut state = State::new(count);
        state.layout = config.layout_settings();
        Self {
            state,
            policy,
            layout,
        }
    }

    /// Command を 1 つ適用し、シェルが実行すべき Effect 列を返す。
    ///
    /// # Errors
    /// 仕様 6 節の `CoreError`。Err のとき状態は変わらない（I7）。
    pub fn apply(&mut self, command: Command) -> Result<Applied, CoreError> {
        let mut next = self.state.clone();
        let from_handler = handlers::dispatch(&mut next, &self.policy, command)?;
        // D64: `Dynamic` では、末尾のワークスペースを常に空にする。
        next.ensure_trailing_empty();
        if next.layout.mode == LayoutMode::Scrolling {
            scrolling::sync(&mut next);
        }
        arrange::arrange(&mut next, &self.layout)?;
        let effects = diff::effects(&self.state, &next, from_handler);
        self.state = next;
        Ok(Applied { effects })
    }

    /// 読み取り専用の状態。
    #[must_use]
    pub const fn view(&self) -> StateView<'_> {
        StateView::new(&self.state)
    }

    /// 不変条件 I1〜I6・I9 を検査する。
    ///
    /// # Errors
    /// 最初に見つかった違反。
    #[cfg(any(test, feature = "invariants"))]
    pub fn check_invariants(&self) -> Result<(), crate::invariant::InvariantViolation> {
        crate::invariant::check(&self.state)
    }

    /// 状態の複製（I7 の比較用）。
    #[cfg(any(test, feature = "invariants"))]
    #[must_use]
    pub fn snapshot(&self) -> crate::invariant::StateSnapshot {
        crate::invariant::StateSnapshot::new(self.state.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::id::{IdKind, WindowId};
    use crate::invariant::InvariantViolation;
    use crate::layout::Columns;
    use crate::policy::DefaultPolicy;

    #[test]
    fn preset_window_heights_are_configurable_and_an_empty_list_keeps_the_default() {
        let default = CoreConfig::new(2);
        assert_eq!(default.with_preset_window_heights(Vec::new()), default);
        let Some(quarter) = Ratio::new(1, 4) else {
            return;
        };
        let custom = default.with_preset_window_heights(alloc::vec![quarter]);
        assert_eq!(
            custom.layout_settings().window_presets.first(),
            Some(&Some(quarter))
        );
        assert_eq!(
            custom
                .layout_settings()
                .window_presets
                .iter()
                .flatten()
                .count(),
            1
        );
        // 列幅のプリセットには影響しない。
        assert_eq!(
            custom.layout_settings().presets,
            default.layout_settings().presets
        );
    }

    #[test]
    fn check_invariants_reports_a_broken_state() {
        let mut core = Core::new(CoreConfig::new(2), DefaultPolicy::new(), Columns::new());
        assert_eq!(core.check_invariants(), Ok(()));
        let ghost = WindowId::from_raw(99);
        core.state.focus = Some(ghost);
        assert_eq!(core.check_invariants(), Err(InvariantViolation::I4(ghost)));
    }
}
