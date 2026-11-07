//! `StateView`: シェル向けの読み取り専用の口。状態は変えない。

use alloc::vec::Vec;

use crate::domain::geometry::Rect;
use crate::domain::id::{ExternalKey, OutputId, WindowId, WorkspaceId, WorkspaceIndex};
use crate::domain::state::State;
use crate::domain::window::WindowState;

/// 窓 1 枚の読み取り結果。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowView {
    /// ID。
    pub id: WindowId,
    /// 最後に計算した矩形（一度も表示されていなければ `None`）。
    pub rect: Option<Rect>,
    /// 配置状態。
    pub state: WindowState,
    /// 属するワークスペースの、全体の並びでの位置（D73）。
    pub workspace: WorkspaceIndex,
    /// 属するワークスペースの id（D82）。
    pub workspace_id: WorkspaceId,
    /// 表示中のワークスペースにあれば true。
    pub visible: bool,
}

/// 読み取り専用の状態。
#[derive(Debug, Clone, Copy)]
pub struct StateView<'a> {
    /// 読む対象。
    state: &'a State,
}

impl<'a> StateView<'a> {
    /// `state` を読む口を作る。
    pub(crate) const fn new(state: &'a State) -> Self {
        Self { state }
    }

    /// フォーカス中の窓。
    #[must_use]
    pub fn focus(&self) -> Option<WindowId> {
        self.state.focus
    }

    /// ワークスペースの数。
    #[must_use]
    pub fn workspace_count(&self) -> u8 {
        self.state.workspace_count()
    }

    /// 窓の `ExternalKey` から ID を引く。
    #[must_use]
    pub fn window_id(&self, key: ExternalKey) -> Option<WindowId> {
        self.state.window_keys.id_of(key)
    }

    /// 窓の ID から `ExternalKey` を引く。
    #[must_use]
    pub fn window_key(&self, id: WindowId) -> Option<ExternalKey> {
        self.state.window_keys.key_of(id)
    }

    /// 出力の `ExternalKey` から ID を引く。
    #[must_use]
    pub fn output_id(&self, key: ExternalKey) -> Option<OutputId> {
        self.state.output_keys.id_of(key)
    }

    /// 出力が表示しているワークスペースの、全体の並びでの位置（D73）。
    #[must_use]
    pub fn shown_workspace(&self, output: OutputId) -> Option<WorkspaceIndex> {
        self.state
            .outputs
            .get(&output)
            .and_then(|o| self.state.index_of(o.shown))
    }

    /// 全体の並びで位置 `index` のワークスペースの id（D63・D73。id は位置が変わっても変わらない通し番号）。
    #[must_use]
    pub fn workspace_id(&self, index: WorkspaceIndex) -> Option<WorkspaceId> {
        self.state.workspace_at(index).map(|ws| ws.id)
    }

    /// 全体の並びで位置 `index` のワークスペースの窓の順序列（D73）。
    #[must_use]
    pub fn workspace_windows(&self, index: WorkspaceIndex) -> Option<&'a [WindowId]> {
        self.state
            .workspace_at(index)
            .map(|ws| ws.windows.as_slice())
    }

    /// アクティブな出力（D82）。`Dynamic` はアクティブな出力（D75）、`Fixed` は D5 のフォーカス出力。
    #[must_use]
    pub fn active_output(&self) -> Option<OutputId> {
        self.state.focused_output()
    }

    /// 出力 `output` のワークスペースの列（D82）。`Dynamic` はその出力の列、`Fixed` は全体の並び。
    /// 出力が無ければ空。
    #[must_use]
    pub fn output_workspaces(&self, output: OutputId) -> Vec<WorkspaceId> {
        if !self.state.outputs.contains_key(&output) {
            return Vec::new();
        }
        self.state.list_of(Some(output))
    }

    /// id のワークスペースの窓の順序列（D82）。
    #[must_use]
    pub fn workspace_windows_by_id(&self, id: WorkspaceId) -> Option<&'a [WindowId]> {
        self.state.workspace(id).map(|ws| ws.windows.as_slice())
    }

    /// ワークスペース `id` の出力（D82）。`Dynamic` は持ち主、`Fixed` は表示している出力。
    #[must_use]
    pub fn workspace_output(&self, id: WorkspaceId) -> Option<OutputId> {
        if self.state.is_dynamic() {
            self.state.workspace(id).and_then(|ws| ws.output)
        } else {
            self.state.output_showing(id)
        }
    }

    /// 窓 1 枚の情報。
    #[must_use]
    pub fn window(&self, id: WindowId) -> Option<WindowView> {
        let w = self.state.windows.get(&id)?;
        Some(WindowView {
            id,
            rect: w.rect,
            state: w.state,
            workspace: self
                .state
                .index_of(w.workspace)
                .unwrap_or(WorkspaceIndex::new(0)),
            workspace_id: w.workspace,
            visible: self.state.is_window_visible(id),
        })
    }

    /// 表示中の窓。出力の OutputId 昇順、各ワークスペースの順序列順。
    #[must_use]
    pub fn visible_windows(&self) -> Vec<WindowView> {
        self.state
            .outputs
            .values()
            .filter_map(|o| self.state.workspace(o.shown))
            .flat_map(|ws| ws.windows.iter())
            .filter_map(|id| self.window(*id))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::state::State;

    use super::StateView;

    #[test]
    fn workspace_count_is_the_number_of_workspaces() {
        for n in [1_u8, 4, 9, 32] {
            let state = State::new(n);
            assert_eq!(StateView::new(&state).workspace_count(), n);
        }
    }
}
