//! ゴムのように、範囲の外へ引っ張るほど動きが鈍くなる関数（niri の `src/rubber_band.rs` と同じ式）。
//!
//! 縦のスワイプでワークスペースの端を越えて引いたときに使う。範囲の中では何もしない（そのまま）。
//! 外へ `x` だけ出ると、`band(x) = (1 − 1 / (x·c/d + 1))·d`（`c` は硬さ、`d` は限界）だけ動き、限界 `d` を超えない。

/// ゴムの性質。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RubberBand {
    /// 硬さ。
    pub stiffness: f64,
    /// 外へ動ける限界。
    pub limit: f64,
}

impl RubberBand {
    /// 範囲の外へ `x`（0 以上）出たときに、実際に動く量。
    pub fn band(&self, x: f64) -> f64 {
        (1.0 - 1.0 / (x * self.stiffness / self.limit + 1.0)) * self.limit
    }

    /// `band` の傾き。
    pub fn derivative(&self, x: f64) -> f64 {
        let (c, d) = (self.stiffness, self.limit);
        c * d * d / (c * x + d).powi(2)
    }

    /// `x` を `[min, max]` に、範囲の外ではゴムのように収める。
    pub fn clamp(&self, min: f64, max: f64, x: f64) -> f64 {
        let clamped = x.clamp(min, max);
        let sign = if x < clamped { -1.0 } else { 1.0 };
        clamped + sign * self.band((x - clamped).abs())
    }

    /// `clamp` の `x` での傾き（範囲の中は 1）。離した瞬間の速度に掛ける。
    pub fn clamp_derivative(&self, min: f64, max: f64, x: f64) -> f64 {
        if min <= x && x <= max {
            return 1.0;
        }
        self.derivative((x - x.clamp(min, max)).abs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RB: RubberBand = RubberBand {
        stiffness: 0.5,
        limit: 0.05,
    };

    #[test]
    fn inside_the_range_nothing_changes() {
        for x in [0.0, 0.3, 1.0] {
            assert_eq!(RB.clamp(0.0, 1.0, x), x);
            assert_eq!(RB.clamp_derivative(0.0, 1.0, x), 1.0);
        }
    }

    #[test]
    fn outside_it_moves_less_and_never_past_the_limit() {
        let mut last = 1.0_f64;
        for pulled in [0.01, 0.1, 1.0, 10.0, 1000.0] {
            let y = RB.clamp(0.0, 1.0, 1.0 + pulled);
            assert!(y > last, "monotonic: {y} after {last}");
            assert!(y < 1.0 + pulled && y < 1.0 + RB.limit, "{y}");
            last = y;
        }
        // 反対側は対称。
        assert!((RB.clamp(0.0, 1.0, -0.2) + RB.clamp(0.0, 1.0, 1.2) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn the_slope_starts_at_the_stiffness_and_falls_off() {
        // 範囲の端の外側で、傾きは硬さ `c`（1 ではない。式は niri と同じ）。
        assert!((RB.derivative(0.0) - RB.stiffness).abs() < 1e-12);
        assert!(RB.derivative(0.1) < RB.derivative(0.01));
        assert!(RB.clamp_derivative(0.0, 1.0, 1.5) < 1.0);
    }

    #[test]
    fn the_derivative_matches_a_numeric_slope() {
        let x = 0.2;
        let h = 1e-6;
        let numeric = (RB.band(x + h) - RB.band(x - h)) / (2.0 * h);
        assert!((numeric - RB.derivative(x)).abs() < 1e-6);
    }
}
