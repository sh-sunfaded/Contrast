//! U5 段階 (a): ズームの幾何（`contrast_policy::zoom`）の性質テスト（INV-Z1・INV-S2）。
//! 参照は実装の関数を呼ばず、整数の有理数（`i128`）の式を別に書く。生成器は core の列レイアウトと同じ形
//! （列が横に隣り合い、各列に窓が縦に積まれる。間隔 0）の矩形の集合。

use contrast_core::Rect;
use contrast_policy::{ZoomLevel, unzoom_point, zoom_edge, zoom_rect};
use proptest::prelude::*;

type Pivot = (i32, i32);

fn level(d: u8) -> ZoomLevel {
    ZoomLevel::new(d.max(1)).unwrap_or(ZoomLevel::IDENTITY)
}

/// 参照: `pivot + (v − pivot) / d` の最も近い整数（同じ近さなら 0 から遠い方）。床と余りで別に書く。
fn reference_edge(v: i32, pivot: i32, d: u8) -> i64 {
    let d = i128::from(d.max(1));
    let x = i128::from(v).saturating_sub(i128::from(pivot));
    let (q, r) = (x.div_euclid(d), x.rem_euclid(d));
    let twice = r.saturating_mul(2);
    let nearest = if twice > d || (twice == d && x > 0) {
        q.saturating_add(1)
    } else {
        q
    };
    i64::try_from(i128::from(pivot).saturating_add(nearest)).unwrap_or(0)
}

/// 列レイアウト: 列の幅と、各列の窓の高さの並び。左端 `x0`、上端 `y0`、間隔 0。
fn layout(x0: i32, y0: i32, columns: &[(i32, Vec<i32>)]) -> Vec<Rect> {
    let mut rects = Vec::new();
    let mut x = x0;
    for (width, heights) in columns {
        let mut y = y0;
        for h in heights {
            if let Some(r) = Rect::new(x, y, *width, *h) {
                rects.push(r);
            }
            y = y.saturating_add(*h);
        }
        x = x.saturating_add(*width);
    }
    rects
}

fn columns() -> impl Strategy<Value = Vec<(i32, Vec<i32>)>> {
    proptest::collection::vec(
        (20i32..800, proptest::collection::vec(20i32..400, 1..4)),
        1..7,
    )
}

fn pivot() -> impl Strategy<Value = Pivot> {
    (0i32..1920, 0i32..1080)
}

proptest! {
    #[test]
    fn edges_match_the_reference_and_are_monotone(
        v1 in -5000i32..5000, dv in 0i32..3000, p in -2000i32..2000, d in 1u8..=8,
    ) {
        let l = level(d);
        let v2 = v1.saturating_add(dv);
        prop_assert_eq!(i64::from(zoom_edge(v1, p, l)), reference_edge(v1, p, d));
        prop_assert!(zoom_edge(v1, p, l) <= zoom_edge(v2, p, l), "順序の保存");
    }

    #[test]
    fn the_identity_changes_nothing(
        x0 in -500i32..500, y0 in -500i32..500, cols in columns(), p in pivot(),
    ) {
        for r in layout(x0, y0, &cols) {
            prop_assert_eq!(zoom_rect(r, p, ZoomLevel::IDENTITY), Some(r));
        }
    }

    #[test]
    fn neighbours_stay_exactly_adjacent(
        x0 in -500i32..500, y0 in -500i32..500, cols in columns(), p in pivot(), d in 2u8..=8,
    ) {
        let l = level(d);
        let rects = layout(x0, y0, &cols);
        for a in &rects {
            for b in &rects {
                let (za, zb) = (zoom_rect(*a, p, l), zoom_rect(*b, p, l));
                let (Some(za), Some(zb)) = (za, zb) else { continue };
                // 横に隣り合う（右 == 左）なら、写したあとも隙間・重なりがない。
                if a.x().saturating_add(a.width()) == b.x() {
                    prop_assert_eq!(za.x().saturating_add(za.width()), zb.x());
                }
                // 縦に隣り合う（下 == 上）も同じ。
                if a.y().saturating_add(a.height()) == b.y() {
                    prop_assert_eq!(za.y().saturating_add(za.height()), zb.y());
                }
            }
        }
    }

    #[test]
    fn sizes_and_gaps_are_within_one_pixel(
        x0 in -500i32..500, y0 in -500i32..500, cols in columns(), p in pivot(), d in 2u8..=8,
        gap in 0i32..200,
    ) {
        let l = level(d);
        let dd = i64::from(d);
        let rects = layout(x0, y0, &cols);
        for r in &rects {
            let Some(z) = zoom_rect(*r, p, l) else { continue };
            // |w' − w/d| ≤ 1（幅・高さ）。丸めは「同じ近さなら 0 から遠い方」なので、基準点をまたぐ辺が
            // ちょうど半分の位置にあると、誤差がちょうど 1 になる（それ以外は 1 未満）。
            // 窓は d px 以上あるので 1 への切り上げは起きない。
            let dw = i64::from(z.width()).saturating_mul(dd).saturating_sub(i64::from(r.width())).abs();
            let dh = i64::from(z.height()).saturating_mul(dd).saturating_sub(i64::from(r.height())).abs();
            prop_assert!(dw <= dd && dh <= dd, "dw={} dh={} d={}", dw, dh, d);
        }
        // 間隔 `gap` を空けた 2 つの列。
        if let (Some(a), Some(b)) = (
            Rect::new(x0, y0, 100, 100),
            Rect::new(x0.saturating_add(100).saturating_add(gap), y0, 100, 100),
        ) && let (Some(za), Some(zb)) = (zoom_rect(a, p, l), zoom_rect(b, p, l)) {
            let g2 = i64::from(zb.x()).saturating_sub(i64::from(za.x()).saturating_add(i64::from(za.width())));
            let err = g2.saturating_mul(dd).saturating_sub(i64::from(gap)).abs();
            prop_assert!(err <= dd, "間隔の誤差 {} d={}", err, d);
        }
    }

    /// 誤差がちょうど 1 になるのは、基準点をまたぎ、両辺がちょうど半分の位置にあるときだけ（それ以外は 1 未満）。
    #[test]
    fn the_size_error_is_below_one_unless_a_tie_straddles_the_pivot(
        v0 in -3000i32..3000, w in 1i32..1500, p in -2000i32..2000, d in 2u8..=8,
    ) {
        let l = level(d);
        let dd = i64::from(d);
        let v1 = v0.saturating_add(w);
        let size = i64::from(zoom_edge(v1, p, l)).saturating_sub(i64::from(zoom_edge(v0, p, l)));
        let err = size.saturating_mul(dd).saturating_sub(i64::from(w)).abs();
        prop_assert!(err <= dd);
        if err == dd {
            // ちょうど 1: 両辺が基準点をまたぎ、どちらも 0.5 の端数（`d` で割った余りが d/2）。
            let r0 = i64::from(v0).saturating_sub(i64::from(p)).rem_euclid(dd);
            let r1 = i64::from(v1).saturating_sub(i64::from(p)).rem_euclid(dd);
            prop_assert!(dd % 2 == 0 && r0 == dd / 2 && r1 == dd / 2);
            prop_assert!(v0 < p && p < v1);
        }
    }

    #[test]
    fn the_map_is_one_uniform_scale_plus_a_translation(
        v in -4000i32..4000, p in -2000i32..2000, d in 1u8..=8,
    ) {
        // INV-S2: x と y に同じ倍率 1/d。写した辺は、連続の写像 pivot + (v − pivot)/d から 0.5 px 以内。
        // 整数で: |2·(v' − pivot)·d − 2·(v − pivot)| ≤ d。
        let l = level(d);
        let v2 = i64::from(zoom_edge(v, p, l)).saturating_sub(i64::from(p));
        let lhs = v2.saturating_mul(2).saturating_mul(i64::from(d))
            .saturating_sub(i64::from(v).saturating_sub(i64::from(p)).saturating_mul(2)).abs();
        prop_assert!(lhs <= i64::from(d));
    }

    #[test]
    fn two_halvings_are_a_quarter_within_one_pixel(v in -4000i32..4000, p in -2000i32..2000) {
        let half = zoom_edge(v, p, level(2));
        let twice = zoom_edge(half, p, level(2));
        let quarter = zoom_edge(v, p, level(4));
        prop_assert!((i64::from(twice).saturating_sub(i64::from(quarter))).abs() <= 1);
    }

    #[test]
    fn pointer_mapping_round_trips_and_stays_inside_the_rect(
        x0 in -500i32..500, y0 in -500i32..500, cols in columns(), p in pivot(), d in 2u8..=8,
        fx in 0.0f64..1.0, fy in 0.0f64..1.0,
    ) {
        let l = level(d);
        let dd = f64::from(d);
        for r in layout(x0, y0, &cols) {
            let Some(z) = zoom_rect(r, p, l) else { continue };
            // 画面の矩形の内側の点を逆写像すると、元の矩形から d/2 px 以内（辺の丸めの分）。
            let q = (
                f64::from(z.x()) + fx * f64::from(z.width()),
                f64::from(z.y()) + fy * f64::from(z.height()),
            );
            let (ux, uy) = unzoom_point(q, p, l);
            let margin = dd / 2.0 + 1e-9;
            prop_assert!(ux >= f64::from(r.x()) - margin && ux <= f64::from(r.x().saturating_add(r.width())) + margin);
            prop_assert!(uy >= f64::from(r.y()) - margin && uy <= f64::from(r.y().saturating_add(r.height())) + margin);
            // 往復: 等倍の点 → 連続の写像 → 逆写像で元に戻る。
            let (px, py) = (f64::from(p.0), f64::from(p.1));
            let w = (f64::from(r.x()), f64::from(r.y()));
            let forward = (px + (w.0 - px) / dd, py + (w.1 - py) / dd);
            let (bx, by) = unzoom_point(forward, p, l);
            prop_assert!((bx - w.0).abs() < 1e-6 && (by - w.1).abs() < 1e-6);
        }
    }
}
