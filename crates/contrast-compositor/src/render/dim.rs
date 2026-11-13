//! 減光の規則（純粋な関数）。窓の画素に掛ける乗数を、窓の状態から決める。
//!
//! 守ること（INV-R1）: 乗数は窓の画素にだけ掛ける。矩形・枠・影・アルファは変えない。
//! `screenshot-window` は常に乗数 1（`snapshot_dim`）。乗数の値そのものは `contrast-policy` の `dim_factor`（仮置き）。

/// 乗数が 1（減光なし）でないか。
pub fn is_dimming(dim: f32) -> bool {
    (dim - 1.0).abs() > f32::EPSILON
}

/// 窓 1 枚に掛ける乗数。フォーカス窓と全画面の窓は 1、それ以外（フォーカス外の Tiled / Floating）は `unfocused_dim`。
pub fn window_dim(focused: bool, fullscreen: bool, unfocused_dim: f32) -> f32 {
    if focused || fullscreen {
        1.0
    } else {
        unfocused_dim.clamp(0.0, 1.0)
    }
}

/// 窓の絵（スナップショット）の使い道。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotPurpose {
    /// 開く・閉じるアニメーション。窓が画面で見えていたとおり（減光を含む）に描く。
    Animation,
    /// `screenshot-window`。窓そのものの絵なので、減光は入れない。
    ScreenshotWindow,
}

/// スナップショットを描くときに掛ける乗数。`window_dim` は、その窓の最後のフレームの乗数。
pub fn snapshot_dim(purpose: SnapshotPurpose, window_dim: f32) -> f32 {
    match purpose {
        SnapshotPurpose::Animation => window_dim,
        SnapshotPurpose::ScreenshotWindow => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_unfocused_non_fullscreen_windows_are_dimmed() {
        assert_eq!(window_dim(true, false, 0.7), 1.0);
        assert_eq!(window_dim(false, true, 0.7), 1.0);
        assert_eq!(window_dim(true, true, 0.7), 1.0);
        assert_eq!(window_dim(false, false, 0.7), 0.7);
    }

    #[test]
    fn the_multiplier_is_kept_in_range() {
        assert_eq!(window_dim(false, false, 1.5), 1.0);
        assert_eq!(window_dim(false, false, -1.0), 0.0);
    }

    #[test]
    fn screenshot_window_never_dims() {
        for k in [0.0, 0.3, 0.7, 1.0] {
            assert_eq!(snapshot_dim(SnapshotPurpose::ScreenshotWindow, k), 1.0);
            assert_eq!(snapshot_dim(SnapshotPurpose::Animation, k), k);
        }
    }

    #[test]
    fn dimming_is_detected_by_a_difference_from_one() {
        assert!(!is_dimming(1.0));
        assert!(is_dimming(0.99));
        assert!(is_dimming(0.0));
    }
}
