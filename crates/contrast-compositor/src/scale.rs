//! 出力の拡大率の自動の決め方（niri の `src/utils/scale.rs`。Mutter の `meta-monitor.c` と同じ規則）。
//!
//! 物理の大きさ（mm）と解像度から DPI を出し、対角が 20 インチ未満なら目標 135 DPI、以上なら 110 DPI に近い拡大率を、
//! 1〜4 の 1/4 刻みから選ぶ（論理の面積が 800×480 未満になる拡大率は使わない）。大きさが分からなければ 1。
//! 最後に、fractional-scale のプロトコルで表せる値（N/120）に丸める。

/// 拡大率の範囲と刻み。
const MIN_SCALE: i32 = 1;
const MAX_SCALE: i32 = 4;
const STEPS: i32 = 4;
/// 論理の面積の下限。
const MIN_LOGICAL_AREA: i32 = 800 * 480;
/// 目標の DPI。
const MOBILE_TARGET_DPI: f64 = 135.0;
const LARGE_TARGET_DPI: f64 = 110.0;
/// 大きい画面とみなす対角（インチ）。
const LARGE_MIN_SIZE_INCHES: f64 = 20.0;

/// 拡大率 `scale` が解像度 `res` で使えるか（論理の面積が下限以上）。
fn valid_for(res: (i32, i32), scale: f64) -> bool {
    let w = (f64::from(res.0) / scale).round() as i32;
    let h = (f64::from(res.1) / scale).round() as i32;
    w.saturating_mul(h) >= MIN_LOGICAL_AREA
}

/// 使える拡大率（1〜4 の 1/4 刻み）。
pub fn supported_scales(res: (i32, i32)) -> impl Iterator<Item = f64> {
    (MIN_SCALE * STEPS..=MAX_SCALE * STEPS)
        .map(|x| f64::from(x) / f64::from(STEPS))
        .filter(move |s| valid_for(res, *s))
}

/// 物理の大きさ `size_mm` と解像度 `res` からの拡大率。
pub fn guess_monitor_scale(size_mm: (i32, i32), res: (i32, i32)) -> f64 {
    if size_mm.0 == 0 || size_mm.1 == 0 {
        return 1.0;
    }
    let diag = f64::from(size_mm.0 * size_mm.0 + size_mm.1 * size_mm.1).sqrt() / 25.4;
    let target = if diag < LARGE_MIN_SIZE_INCHES {
        MOBILE_TARGET_DPI
    } else {
        LARGE_TARGET_DPI
    };
    let dpi = f64::from(res.0 * res.0 + res.1 * res.1).sqrt() / diag;
    let perfect = dpi / target;
    supported_scales(res)
        .map(|s| (s, (s - perfect).abs()))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(1.0, |(s, _)| s)
}

/// fractional-scale のプロトコルで表せる値（N/120）に丸める。
pub fn closest_representable_scale(scale: f64) -> f64 {
    (scale * 120.0).round() / 120.0
}

#[cfg(test)]
mod tests {
    use super::*;

    // 期待値は niri（Mutter）のテストの機種と同じ。
    #[test]
    fn guesses_follow_mutter() {
        assert_eq!(guess_monitor_scale((65, 129), (720, 1440)), 1.5);
        assert_eq!(guess_monitor_scale((68, 144), (1080, 2280)), 2.5);
        assert_eq!(guess_monitor_scale((286, 179), (2560, 1600)), 1.75);
        assert_eq!(guess_monitor_scale((303, 202), (2400, 1600)), 1.5);
        assert_eq!(guess_monitor_scale((290, 180), (3840, 2400)), 2.5);
        assert_eq!(guess_monitor_scale((509, 286), (1920, 1080)), 1.0);
        assert_eq!(guess_monitor_scale((509, 286), (3840, 2160)), 1.75);
        assert_eq!(guess_monitor_scale((598, 336), (3840, 2160)), 1.5);
        assert_eq!(guess_monitor_scale((708, 398), (3840, 2160)), 1.25);
        assert_eq!(guess_monitor_scale((554, 312), (3840, 2160)), 1.5);
        assert_eq!(guess_monitor_scale((522, 294), (3840, 2160)), 1.75);
        assert_eq!(guess_monitor_scale((340, 210), (2560, 1600)), 1.5);
        assert_eq!(guess_monitor_scale((700, 390), (2560, 1440)), 1.0);
        assert_eq!(guess_monitor_scale((260, 170), (2736, 1824)), 2.0);
        assert_eq!(guess_monitor_scale((0, 0), (1920, 1080)), 1.0);
    }

    #[test]
    fn scales_round_to_n_over_120() {
        assert!((closest_representable_scale(1.3) - 1.3).abs() < 1e-12);
        assert!((closest_representable_scale(1.31) - 157.0 / 120.0).abs() < 1e-12);
        assert!((closest_representable_scale(1.33) - 160.0 / 120.0).abs() < 1e-12);
    }
}
