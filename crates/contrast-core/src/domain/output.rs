//! 出力（モニタ）: `Output` と `OutputName`。
//!
//! 守る不変条件:
//! - I3 各出力は表示中ワークスペースをちょうど 1 つ持つ（`shown` は常に値を持つ）。
//!   2 つの出力が同じワークスペースを表示しないことは `State` の操作が保証する。
//! - D32 作業領域は論理矩形から `insets` を引いた矩形で、幅・高さが 1 以上でなければならない。

use alloc::string::String;

use crate::domain::geometry::Rect;
use crate::domain::id::{OutputId, WorkspaceId};
use crate::domain::insets::Insets;
use crate::error::LayoutError;

/// 出力の名前（例: `"winit"`、`"DP-1"`）。core は表示用に保持するだけ。
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct OutputName(String);

impl OutputName {
    /// 名前 `name` を作る。
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// 文字列として返す。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// core が管理する出力 1 つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// ID。
    pub id: OutputId,
    /// 名前。
    pub name: OutputName,
    /// 論理矩形。
    pub geometry: Rect,
    /// 表示中のワークスペースの id（I3。D73）。
    pub shown: WorkspaceId,
    /// 作業領域から除く余白（D31。追加時は 0）。
    pub insets: Insets,
}

impl Output {
    /// 作業領域。論理矩形から `insets` を引いた矩形（D32）。
    ///
    /// # Errors
    /// 幅または高さが 1 未満になる（計算が i32 に収まらない場合を含む）と `AreaTooSmall`。
    pub fn work_area(&self) -> Result<Rect, LayoutError> {
        let g = &self.geometry;
        let i = &self.insets;
        let x = g.x().checked_add(i.left());
        let y = g.y().checked_add(i.top());
        let width = g
            .width()
            .checked_sub(i.left())
            .and_then(|w| w.checked_sub(i.right()));
        let height = g
            .height()
            .checked_sub(i.top())
            .and_then(|h| h.checked_sub(i.bottom()));
        match (x, y, width, height) {
            (Some(x), Some(y), Some(w), Some(h)) => {
                Rect::new(x, y, w, h).ok_or(LayoutError::AreaTooSmall)
            }
            _ => Err(LayoutError::AreaTooSmall),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OutputName;

    #[test]
    fn output_name_returns_what_it_was_given() {
        assert_eq!(OutputName::new("DP-1").as_str(), "DP-1");
        assert_eq!(
            OutputName::new(alloc::string::String::from("winit")).as_str(),
            "winit"
        );
    }
}
