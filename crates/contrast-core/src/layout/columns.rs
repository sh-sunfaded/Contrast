//! 参照レイアウト `Columns`: 作業領域を窓の数で横に等分する。
//!
//! 守る不変条件:
//! - I5 `Rect::split_columns` の結果をそのまま使うので、列は作業領域内に収まり重ならない。

use alloc::vec::Vec;

use crate::domain::geometry::Rect;
use crate::domain::id::WindowId;
use crate::error::LayoutError;
use crate::layout::Layout;

/// 窓を横一列に等分して並べる。余りの幅は先頭の列から 1 ずつ配る。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Columns;

impl Columns {
    /// `Columns` を作る。
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Layout for Columns {
    fn arrange(
        &self,
        area: Rect,
        windows: &[WindowId],
        out: &mut Vec<(WindowId, Rect)>,
    ) -> Result<(), LayoutError> {
        if windows.is_empty() {
            return Ok(());
        }
        let rects = area.split_columns(windows.len())?;
        out.extend(windows.iter().copied().zip(rects));
        Ok(())
    }
}
