//! 作業領域の余白: `Insets`。
//!
//! 守る不変条件:
//! - D30 4 辺はすべて 0 以上（`new` が負の値を拒否し、フィールドは非公開）。

/// 出力の論理矩形から作業領域を切り出すときに、各辺から除く幅。
///
/// layer-shell のバーなどが確保する領域を表す。すべて 0 以上。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Insets {
    /// 上辺から除く高さ。
    top: i32,
    /// 右辺から除く幅。
    right: i32,
    /// 下辺から除く高さ。
    bottom: i32,
    /// 左辺から除く幅。
    left: i32,
}

impl Insets {
    /// 4 辺の余白から作る。負の値が 1 つでもあれば `None`。
    #[must_use]
    pub const fn new(top: i32, right: i32, bottom: i32, left: i32) -> Option<Self> {
        if top < 0 || right < 0 || bottom < 0 || left < 0 {
            return None;
        }
        Some(Self {
            top,
            right,
            bottom,
            left,
        })
    }

    /// 上辺の余白。
    #[must_use]
    pub const fn top(&self) -> i32 {
        self.top
    }

    /// 右辺の余白。
    #[must_use]
    pub const fn right(&self) -> i32 {
        self.right
    }

    /// 下辺の余白。
    #[must_use]
    pub const fn bottom(&self) -> i32 {
        self.bottom
    }

    /// 左辺の余白。
    #[must_use]
    pub const fn left(&self) -> i32 {
        self.left
    }
}
