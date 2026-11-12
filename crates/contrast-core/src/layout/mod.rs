//! レイアウト: Tiled 窓の矩形を決める `Layout` トレイトと参照実装。
//!
//! 守る不変条件:
//! - I5 実装は、作業領域内に収まり互いに重ならない矩形を、渡された窓ごとに 1 つ返す。

mod columns;

use alloc::vec::Vec;

use crate::domain::geometry::Rect;
use crate::domain::id::WindowId;
use crate::error::LayoutError;

pub use columns::Columns;

/// Tiled 窓の配置計算。
pub trait Layout {
    /// 作業領域 `area` に `windows`（順序列順）を並べ、`(窓, 矩形)` を `out` に足す。
    ///
    /// # Errors
    /// 領域が足りないなどで配置できなければ `LayoutError`。
    fn arrange(
        &self,
        area: Rect,
        windows: &[WindowId],
        out: &mut Vec<(WindowId, Rect)>,
    ) -> Result<(), LayoutError>;
}
