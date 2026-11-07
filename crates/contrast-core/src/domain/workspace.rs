//! ワークスペース: 窓の順序列とフォーカス履歴。
//!
//! 守る不変条件:
//! - I2 順序列に重複がない（`push_window` は重複を足さない）。列中の ID が存在する窓を指すことは
//!   `State` の操作が保証し、`invariant.rs` が検査する。
//! - フォーカス履歴は順序列の部分集合で、重複がない（最新が末尾）。
//! - I13（`Dynamic`）持ち主の出力（`output`）は、出力が 1 台以上なら存在する出力を指す（`State` の操作が保証する）。

use alloc::vec::Vec;

use crate::domain::column::Column;
use crate::domain::id::{OutputId, WindowId, WorkspaceId};
use crate::domain::output::OutputName;

/// ワークスペース 1 つ。`Fixed` では全出力で共有され（D1）、`Dynamic` では出力ごとの列に属する（D74）。
/// 位置は持たない（`State.workspaces` の並びが位置。窓と出力は id で参照する。D73）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    /// ID。
    pub id: WorkspaceId,
    /// 持ち主の出力（`Dynamic` のみ。出力が 0 台のとき・`Fixed` では `None`。D74）。
    pub output: Option<OutputId>,
    /// 元の出力の名前（`Dynamic` のみ。niri の `original_outputs`。D74）。
    pub original_output: Option<OutputName>,
    /// 窓の順序列（タイル配置の並び順）。
    pub windows: Vec<WindowId>,
    /// フォーカス履歴。末尾が最新。
    pub focus_history: Vec<WindowId>,
    /// 列の並び（左から右）。`Scrolling` のときだけ使い、`Equal` では空のまま（D36）。
    pub columns: Vec<Column>,
    /// ビューの左端（作業領域の左端から数えた x、0 以上。`Scrolling` のみ、D39）。
    pub view_x: i32,
}

impl Workspace {
    /// 持ち主の無い空のワークスペースを作る。
    #[must_use]
    pub const fn new(id: WorkspaceId) -> Self {
        Self {
            id,
            output: None,
            original_output: None,
            windows: Vec::new(),
            focus_history: Vec::new(),
            columns: Vec::new(),
            view_x: 0,
        }
    }

    /// 順序列の末尾に窓を足す。既にあれば何もしない（I2）。
    pub fn push_window(&mut self, id: WindowId) {
        if !self.windows.contains(&id) {
            self.windows.push(id);
        }
    }

    /// 順序列と履歴から窓を除き、順序列で占めていた位置を返す。
    pub fn remove_window(&mut self, id: WindowId) -> Option<usize> {
        self.focus_history.retain(|w| *w != id);
        self.remove_from_columns(id);
        let position = self.windows.iter().position(|w| *w == id)?;
        self.windows.retain(|w| *w != id);
        Some(position)
    }

    /// 窓 `id` がある列の位置 `(列, 列の中の行)`。どの列にもなければ `None`。
    #[must_use]
    pub fn column_of(&self, id: WindowId) -> Option<(usize, usize)> {
        self.columns.iter().enumerate().find_map(|(c, col)| {
            col.windows
                .iter()
                .position(|w| *w == id)
                .map(|row| (c, row))
        })
    }

    /// 窓 `id` を列から除く。空になった列も除く。
    pub fn remove_from_columns(&mut self, id: WindowId) {
        for col in &mut self.columns {
            col.windows.retain(|w| *w != id);
        }
        self.columns.retain(|col| !col.windows.is_empty());
    }

    /// 列 `from` を位置 `to` へ移す（間の列は 1 つずつずれる）。範囲外なら何もしない。
    pub fn move_column(&mut self, from: usize, to: usize) {
        if from >= self.columns.len() || to >= self.columns.len() || from == to {
            return;
        }
        let column = self.columns.remove(from);
        self.columns.insert(to, column);
    }

    /// フォーカス履歴の最新を `id` にする。
    pub fn record_focus(&mut self, id: WindowId) {
        self.focus_history.retain(|w| *w != id);
        self.focus_history.push(id);
    }

    /// フォーカス履歴の最新。なければ順序列の先頭。どちらもなければ `None`（D12）。
    #[must_use]
    pub fn preferred_focus(&self) -> Option<WindowId> {
        self.focus_history
            .last()
            .or_else(|| self.windows.first())
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;
    use crate::domain::column::{ColumnWidth, Ratio};
    use crate::domain::id::IdKind;

    /// 窓 `n` の ID。
    fn wid(n: u64) -> WindowId {
        WindowId::from_raw(n)
    }

    /// 窓 `ids` を 1 列ずつに入れたワークスペース。
    fn with_columns(ids: &[u64]) -> Workspace {
        let mut ws = Workspace::new(WorkspaceId::from_raw(1));
        for n in ids {
            ws.windows.push(wid(*n));
            ws.columns.push(Column::single(
                wid(*n),
                ColumnWidth::Proportion(Ratio::HALF),
            ));
        }
        ws
    }

    #[test]
    fn removing_a_window_drops_it_from_its_column_and_deletes_empty_columns() {
        let mut ws = with_columns(&[1, 2, 3]);
        ws.columns
            .get_mut(0)
            .map(|c| c.windows.push(wid(4)))
            .unwrap_or_default();
        ws.windows.push(wid(4));
        ws.remove_window(wid(1));
        // 列 0 には 4 だけが残る。
        assert_eq!(ws.column_of(wid(4)), Some((0, 0)));
        ws.remove_window(wid(4));
        // 空になった列は消える。
        assert_eq!(ws.columns.len(), 2);
        assert_eq!(ws.column_of(wid(2)), Some((0, 0)));
        assert_eq!(ws.column_of(wid(3)), Some((1, 0)));
    }

    #[test]
    fn remove_from_columns_leaves_the_window_order_alone() {
        let mut ws = with_columns(&[1, 2]);
        ws.remove_from_columns(wid(1));
        assert_eq!(ws.windows, vec![wid(1), wid(2)]);
        assert_eq!(ws.column_of(wid(1)), None);
        assert_eq!(ws.columns.len(), 1);
    }
}
