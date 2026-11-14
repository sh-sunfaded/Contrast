//! ズームの幾何（純粋）。`docs/ui-spec.md` §4.5・INV-Z1・INV-S2。
//!
//! ズームは「ビューの見え方の変換」で、**平行移動と一様な拡縮だけ**（リフローなし）。窓の実サイズ（`configure`）は等倍のまま、
//! 描画だけを基準点 `pivot` 中心に `1 / divisor` 倍にする。倍率を整数の約数にするので、写像は有理数で厳密に決まる。
//!
//! 矩形は**4 辺を別々に**同じ写像で写し、幅・高さは写した辺の差とする。したがって:
//! - 順序の保存（写像は単調非減少）。
//! - 隣接の厳密な保存（`A.右 == B.左` なら `A'.右 == B'.左`。同じ整数が同じ整数に写る）。
//! - 大きさ・間隔の誤差は 1 論理 px 以下（各辺は ±0.5 の丸め。ちょうど 1 になるのは、基準点をまたぐ 2 辺がどちらも
//!   ちょうど半分の位置にあるときだけで、それ以外は 1 未満。丸めが「同じ近さなら 0 から遠い方」で、基準点の両側で向きが逆になるため）。
//! - `divisor = 1` は恒等。
//!
//! 丸めは「最も近い整数、同じ近さなら 0 から遠い方」（`round_half_away`）。時計・GL に依存しない。

use contrast_core::Rect;

/// ズームの段階の約数（1 = 等倍、2 = 1/2 倍、…）。1 以上。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ZoomLevel(u8);

impl ZoomLevel {
    /// 等倍。
    pub const IDENTITY: Self = Self(1);

    /// 約数 `divisor`（1 以上）から作る。0 は `None`。
    #[must_use]
    pub const fn new(divisor: u8) -> Option<Self> {
        if divisor == 0 {
            None
        } else {
            Some(Self(divisor))
        }
    }

    /// 約数。
    #[must_use]
    pub const fn divisor(self) -> u8 {
        self.0
    }

    /// 等倍か。
    #[must_use]
    pub const fn is_identity(self) -> bool {
        self.0 == 1
    }
}

impl Default for ZoomLevel {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// 段階の列（先頭は常に等倍）。`<Zoom>` が無ければ等倍だけ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoomSettings {
    /// 段階（先頭が等倍。2〜3 個。厳密に増える約数の順）。
    pub levels: Vec<ZoomLevel>,
}

impl Default for ZoomSettings {
    fn default() -> Self {
        Self {
            levels: vec![ZoomLevel::IDENTITY],
        }
    }
}

impl ZoomSettings {
    /// 段階の数（1 = 等倍のみ）。
    #[must_use]
    pub fn count(&self) -> usize {
        self.levels.len()
    }

    /// 段階 `index`（0 始まり）の約数の水準。範囲外は等倍。
    #[must_use]
    pub fn level(&self, index: usize) -> ZoomLevel {
        self.levels.get(index).copied().unwrap_or_default()
    }
}

/// 辺（座標）`v` を、基準点 `pivot` 中心に `1 / divisor` 倍に写す。
#[must_use]
pub fn zoom_edge(v: i32, pivot: i32, level: ZoomLevel) -> i32 {
    let divisor = u64::from(level.0);
    let offset = i64::from(v).saturating_sub(i64::from(pivot));
    // 最も近い整数、同じ近さなら 0 から遠い方: `sign × ((2|x| + d) ÷ 2d)`（整数の商）。
    let numerator = offset
        .unsigned_abs()
        .saturating_mul(2)
        .saturating_add(divisor);
    let quotient = numerator
        .checked_div(divisor.saturating_mul(2))
        .and_then(|q| i64::try_from(q).ok())
        .unwrap_or(i64::MAX);
    let signed = if offset < 0 {
        quotient.saturating_neg()
    } else {
        quotient
    };
    let out = i64::from(pivot).saturating_add(signed);
    i32::try_from(out).unwrap_or(if out < 0 { i32::MIN } else { i32::MAX })
}

/// 矩形 `rect` を、基準点 `pivot`（x, y）中心に写す。4 辺を別々に写し、幅・高さが 1 未満になるときは 1 にする。
/// 座標が `i32` に収まらないときは `None`。
#[must_use]
pub fn zoom_rect(rect: Rect, pivot: (i32, i32), level: ZoomLevel) -> Option<Rect> {
    let right = rect.x().checked_add(rect.width())?;
    let bottom = rect.y().checked_add(rect.height())?;
    let (x0, x1) = (
        zoom_edge(rect.x(), pivot.0, level),
        zoom_edge(right, pivot.0, level),
    );
    let (y0, y1) = (
        zoom_edge(rect.y(), pivot.1, level),
        zoom_edge(bottom, pivot.1, level),
    );
    let width = x1.checked_sub(x0)?.max(1);
    let height = y1.checked_sub(y0)?.max(1);
    Rect::new(x0, y0, width, height)
}

/// 画面の点 `q` を、等倍の点にする（`zoom_rect` の逆写像。ポインタ用）: `pivot + (q − pivot) × divisor`。
#[must_use]
pub fn unzoom_point(q: (f64, f64), pivot: (i32, i32), level: ZoomLevel) -> (f64, f64) {
    let divisor = f64::from(level.0);
    let (px, py) = (f64::from(pivot.0), f64::from(pivot.1));
    (px + (q.0 - px) * divisor, py + (q.1 - py) * divisor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lv(d: u8) -> ZoomLevel {
        ZoomLevel::new(d).unwrap()
    }

    /// 参照: 最も近い整数、同じ近さなら 0 から遠い方。実装と別の式（床と余り）で書く。
    fn reference_edge(v: i32, pivot: i32, d: i64) -> i32 {
        let x = i64::from(v).saturating_sub(i64::from(pivot));
        let (q, r) = (x.div_euclid(d), x.rem_euclid(d));
        let twice = r.saturating_mul(2);
        let nearest = if twice > d || (twice == d && x > 0) {
            q.saturating_add(1)
        } else {
            q
        };
        i32::try_from(i64::from(pivot).saturating_add(nearest)).unwrap()
    }

    #[test]
    fn a_level_needs_a_positive_divisor() {
        assert_eq!(ZoomLevel::new(0), None);
        assert_eq!(ZoomLevel::new(1), Some(ZoomLevel::IDENTITY));
        assert!(ZoomLevel::IDENTITY.is_identity());
        assert!(!lv(2).is_identity());
        assert_eq!(lv(4).divisor(), 4);
    }

    #[test]
    fn edges_round_to_the_nearest_with_ties_away_from_the_pivot() {
        // pivot 0、2 で割る: 1→0.5→1（遠い方）、-1→-0.5→-1、3→1.5→2、-3→-2。
        assert_eq!(zoom_edge(1, 0, lv(2)), 1);
        assert_eq!(zoom_edge(-1, 0, lv(2)), -1);
        assert_eq!(zoom_edge(3, 0, lv(2)), 2);
        assert_eq!(zoom_edge(-3, 0, lv(2)), -2);
        assert_eq!(zoom_edge(4, 0, lv(2)), 2);
        // pivot は動かない。
        assert_eq!(zoom_edge(500, 500, lv(4)), 500);
        // 3 で割る: 1→0.33→0、2→0.67→1。
        assert_eq!(zoom_edge(1, 0, lv(3)), 0);
        assert_eq!(zoom_edge(2, 0, lv(3)), 1);
    }

    #[test]
    fn edges_match_the_independent_reference_over_a_grid() {
        for d in 1u8..=8 {
            for pivot in [-37, 0, 5, 960] {
                for v in (-2000..=2000).step_by(7) {
                    assert_eq!(
                        zoom_edge(v, pivot, lv(d)),
                        reference_edge(v, pivot, i64::from(d)),
                        "v={v} pivot={pivot} d={d}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_identity_changes_nothing() {
        let r = Rect::new(-30, 7, 400, 300).unwrap();
        assert_eq!(zoom_rect(r, (960, 540), ZoomLevel::IDENTITY), Some(r));
        for v in [-5, 0, 77, i32::MAX, i32::MIN] {
            assert_eq!(zoom_edge(v, 123, ZoomLevel::IDENTITY), v);
        }
    }

    #[test]
    fn a_rect_is_mapped_by_its_four_edges() {
        // pivot (960, 540)、1/2: 左右 0..1920 → 480..1440、上下 0..1080 → 270..810。
        let r = Rect::new(0, 0, 1920, 1080).unwrap();
        assert_eq!(
            zoom_rect(r, (960, 540), lv(2)),
            Rect::new(480, 270, 960, 540)
        );
        // 1/4。
        assert_eq!(
            zoom_rect(r, (960, 540), lv(4)),
            Rect::new(720, 405, 480, 270)
        );
    }

    #[test]
    fn adjacent_rects_stay_exactly_adjacent() {
        // 幅 101 と 77 の隣り合う列（右端 == 左端）。丸めが奇数でも隙間・重なりが生まれない。
        let a = Rect::new(0, 0, 101, 50).unwrap();
        let b = Rect::new(101, 0, 77, 50).unwrap();
        for d in 2u8..=8 {
            let (za, zb) = (
                zoom_rect(a, (40, 25), lv(d)).unwrap(),
                zoom_rect(b, (40, 25), lv(d)).unwrap(),
            );
            assert_eq!(za.x() + za.width(), zb.x(), "d={d}");
        }
    }

    #[test]
    fn a_tiny_rect_keeps_at_least_one_pixel() {
        let r = Rect::new(100, 100, 1, 1).unwrap();
        let z = zoom_rect(r, (0, 0), lv(8)).unwrap();
        assert!(z.width() >= 1 && z.height() >= 1);
    }

    #[test]
    fn unzoom_is_the_inverse_of_the_continuous_map() {
        // 画面の点 (480, 270) は、1/2 で pivot (960, 540) から等倍の (0, 0)。
        let (x, y) = unzoom_point((480.0, 270.0), (960, 540), lv(2));
        assert!((x - 0.0).abs() < 1e-9 && (y - 0.0).abs() < 1e-9);
        // pivot は動かない。
        let (x, y) = unzoom_point((960.0, 540.0), (960, 540), lv(4));
        assert!((x - 960.0).abs() < 1e-9 && (y - 540.0).abs() < 1e-9);
        // 等倍は恒等。
        assert_eq!(
            unzoom_point((12.5, -3.0), (100, 100), ZoomLevel::IDENTITY),
            (12.5, -3.0)
        );
    }

    #[test]
    fn settings_default_to_the_identity_only() {
        let s = ZoomSettings::default();
        assert_eq!(s.count(), 1);
        assert_eq!(s.level(0), ZoomLevel::IDENTITY);
        // 範囲外は等倍。
        assert_eq!(s.level(5), ZoomLevel::IDENTITY);
    }
}
