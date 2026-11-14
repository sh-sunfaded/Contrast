//! ズーム中の描画の変換（U5。`docs/ui-spec.md` §4.5）。窓の実サイズ（`configure`）は等倍のまま、窓・枠・影・開閉の絵・減光の
//! 要素を、作業領域の中心を基準に 1/divisor 倍に縮めて描く。レイヤー面（バー）・カーソル・ほかの出力は縮めない。
//!
//! 守ること:
//! - 等倍の段階では、要素の列を**一切変えない**（`zoom_elements(_, None)` は恒等）。
//! - 縮めるのは窓まわりの要素だけ（`scene_elements` が、その範囲を渡す）。出力の範囲で切り抜く。
//! - 矩形・順序・フォーカスは core のまま（描画だけの変換。INV-S1・S2・F1・R1）。

use smithay::{
    backend::renderer::element::{
        Element,
        utils::{CropRenderElement, RescaleRenderElement},
    },
    output::Output,
    utils::{Logical, Physical, Point, Rectangle, Scale},
};

use contrast_core::Rect;
use contrast_policy::ZoomLevel;

use crate::render::SceneElement;
use crate::state::Contrast;

/// 縮める変換: 約数、基準点（描画先の物理座標）、切り抜く範囲（描画先の物理座標）、出力の倍率。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ZoomXform {
    /// 約数（2 以上。1 = 等倍なら変換を作らない）。
    pub divisor: u8,
    /// 基準点（描画先の物理座標）。
    pub pivot: Point<i32, Physical>,
    /// 切り抜く範囲（描画先の物理座標。出力の範囲）。
    pub crop: Rectangle<i32, Physical>,
    /// 出力の倍率。
    pub scale: f64,
}

/// 作業領域 `(x, y, w, h)` の中心（論理座標。整数で切り捨て）。ポインタの逆写像と共通の基準点。
pub(crate) fn pivot_of(area: (i32, i32, i32, i32)) -> (i32, i32) {
    let (x, y, w, h) = area;
    (
        x.saturating_add(w.checked_div(2).unwrap_or(0)),
        y.saturating_add(h.checked_div(2).unwrap_or(0)),
    )
}

/// 段階 `current`（0 始まり）を `delta`（+1 = 縮める、-1 = 戻す）だけ動かす。範囲 `0..count` に収める。
/// 動かせない（端）ときは `None`（何もしない）。
pub(crate) fn step_level(current: usize, count: usize, delta: i8) -> Option<usize> {
    if count <= 1 || current >= count {
        return None;
    }
    let next = match delta.cmp(&0) {
        std::cmp::Ordering::Greater => current.checked_add(1)?,
        std::cmp::Ordering::Less => current.checked_sub(1)?,
        std::cmp::Ordering::Equal => return None,
    };
    (next < count).then_some(next)
}

/// `element` を `pivot` 中心に 1/divisor 倍に縮め、出力の範囲で切り抜く。何も残らなければ `None`。
fn shrink<E: Element>(
    element: E,
    xf: &ZoomXform,
) -> Option<CropRenderElement<RescaleRenderElement<E>>> {
    CropRenderElement::from_element(
        RescaleRenderElement::from_element(
            element,
            xf.pivot,
            Scale::from(1.0 / f64::from(xf.divisor)),
        ),
        Scale::from(xf.scale),
        xf.crop,
    )
}

/// 点 `p`（論理座標）が矩形 `rect` の中か（左・上は含み、右・下は含まない）。整数の点の `Rect::contains` と、
/// 点の床を取った結果が一致する（`floor(p)` が矩形の中 ⇔ `p` が実数として矩形の中）。
pub(crate) fn rect_contains_f64(rect: Rect, p: (f64, f64)) -> bool {
    let (x, y) = (f64::from(rect.x()), f64::from(rect.y()));
    p.0 >= x && p.0 < x + f64::from(rect.width()) && p.1 >= y && p.1 < y + f64::from(rect.height())
}

/// 画面の点 `q` にある窓。`windows` は下から上への並び（上に描かれるものが後ろ）で、矩形は core の等倍の矩形。
/// `level` が等倍でないとき、`q` を逆写像（`pivot` 中心）で等倍の点にしてから探す。上の窓を優先する。
pub(crate) fn pick_window<T: Copy>(
    windows: &[(T, Rect)],
    q: (f64, f64),
    pivot: (i32, i32),
    level: ZoomLevel,
) -> Option<T> {
    let world = contrast_policy::unzoom_point(q, pivot, level);
    windows
        .iter()
        .rev()
        .find(|(_, r)| rect_contains_f64(*r, world))
        .map(|(t, _)| *t)
}

/// クライアントに渡す「面の原点」。Smithay は `location − 面の原点` を面内座標として送るので、`location` を画面の位置
/// （カーソルの位置）のままにして、面内座標を等倍の点 `world` に対する `world − 本当の原点` にするために、
/// 原点を `origin + (pos − world)` にする。
pub(crate) fn client_surface_origin(
    origin: (f64, f64),
    pos: (f64, f64),
    world: (f64, f64),
) -> (f64, f64) {
    (origin.0 + (pos.0 - world.0), origin.1 + (pos.1 - world.1))
}

/// 要素 1 つを縮める。縮められない種類（カーソル・UI・すでに縮めたもの）はそのまま。切り抜いて何も残らなければ `None`。
fn zoom_element(element: SceneElement, xf: &ZoomXform) -> Option<SceneElement> {
    match element {
        // 窓まわりの要素: 縮めて切り抜く。
        SceneElement::Surface(e) => shrink(e, xf).map(SceneElement::CroppedSurface),
        SceneElement::Rounded(e) => shrink(e, xf).map(SceneElement::ZRounded),
        SceneElement::Solid(e) => shrink(e, xf).map(SceneElement::ZSolid),
        SceneElement::Pixel(e) => shrink(e, xf).map(SceneElement::ZPixel),
        SceneElement::Anim(e) => shrink(e, xf).map(SceneElement::ZAnim),
        SceneElement::AnimBand(e) => shrink(e, xf).map(SceneElement::ZAnimBand),
        // カーソル・UI・すでに縮めたものは、そのまま。
        other => Some(other),
    }
}

/// 窓まわりの要素の列を縮める。`None`（等倍）なら恒等。
pub(crate) fn zoom_elements(
    elements: Vec<SceneElement>,
    xf: Option<&ZoomXform>,
) -> Vec<SceneElement> {
    let Some(xf) = xf else {
        return elements;
    };
    elements
        .into_iter()
        .filter_map(|e| zoom_element(e, xf))
        .collect()
}

impl Contrast {
    /// 出力 `output`（描画先の原点が全体の論理座標の `origin`）に掛ける変換。ズームしていない・アクティブな出力でない・
    /// 作業領域が無いときは `None`。
    pub(crate) fn zoom_xform(
        &self,
        output: &Output,
        origin: Point<i32, Logical>,
    ) -> Option<ZoomXform> {
        if self.zoom_level == 0 || self.active_output().as_ref() != Some(output) {
            return None;
        }
        let level = self.policy.zoom_settings().level(self.zoom_level);
        if level.is_identity() {
            return None;
        }
        let area = self.work_area(output)?;
        let geo = self.space.output_geometry(output)?;
        let scale = output.current_scale().fractional_scale();
        let (px, py) = pivot_of((area.x(), area.y(), area.width(), area.height()));
        let pivot: Point<i32, Physical> =
            (Point::<i32, Logical>::from((px, py)) - origin).to_physical_precise_round(scale);
        let crop: Rectangle<i32, Physical> =
            Rectangle::new(geo.loc - origin, geo.size).to_physical_precise_round(scale);
        Some(ZoomXform {
            divisor: level.divisor(),
            pivot,
            crop,
            scale,
        })
    }

    /// 画面の点 `pos` を、等倍の点にする（ポインタ → 窓・面の座標）。ズームしていない・アクティブな出力の外・作業領域が
    /// 無いときは、そのまま。レイヤー面（バー）は縮めないので、レイヤー面の判定には使わない。
    pub(crate) fn zoom_unmap(&self, pos: Point<f64, Logical>) -> Point<f64, Logical> {
        let Some((pivot, level)) = self.zoom_context(pos) else {
            return pos;
        };
        let (x, y) = contrast_policy::unzoom_point((pos.x, pos.y), pivot, level);
        Point::from((x, y))
    }

    /// 画面の点 `pos` にズームが掛かっているとき、その基準点（論理座標）と段階。掛かっていなければ `None`
    /// （等倍・アクティブな出力の外・作業領域が無い）。
    pub(crate) fn zoom_context(&self, pos: Point<f64, Logical>) -> Option<((i32, i32), ZoomLevel)> {
        if self.zoom_level == 0 {
            return None;
        }
        let output = self.output_at(pos.x, pos.y)?;
        if self.active_output().as_ref() != Some(&output) {
            return None;
        }
        let level = self.policy.zoom_settings().level(self.zoom_level);
        if level.is_identity() {
            return None;
        }
        let area = self.work_area(&output)?;
        Some((
            pivot_of((area.x(), area.y(), area.width(), area.height())),
            level,
        ))
    }

    /// ズームの段階を `delta`（+1 = 縮める、-1 = 戻す）だけ動かす。端なら何もしない。
    pub(crate) fn zoom_step(&mut self, delta: i8) {
        let count = self.policy.zoom_settings().count();
        if let Some(next) = step_level(self.zoom_level, count, delta) {
            self.set_zoom_level(next);
        }
    }

    /// ズームを等倍に戻す。
    pub(crate) fn zoom_reset(&mut self) {
        self.set_zoom_level(0);
    }

    /// ズームの段階を `level`（0 始まり。範囲外は等倍）にする。変わったときだけ記録に残す。
    pub(crate) fn set_zoom_level(&mut self, level: usize) {
        let settings = self.policy.zoom_settings();
        let level = if level < settings.count() { level } else { 0 };
        if level == self.zoom_level {
            return;
        }
        self.zoom_level = level;
        let divisor = settings.level(level).divisor();
        tracing::info!(level, divisor, "zoom");
        let stamp = self.record_stamp();
        if let Some(recorder) = &mut self.recorder {
            recorder.record_zoom(level, divisor, stamp);
        }
    }

    /// 設定を読み直したあと、段階が範囲外（段階が減った）なら等倍に戻す。
    pub(crate) fn clamp_zoom_after_reload(&mut self) {
        let count = self.policy.zoom_settings().count();
        if self.zoom_level >= count {
            self.set_zoom_level(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use smithay::backend::renderer::element::{Element, Id, Kind, solid::SolidColorRenderElement};
    use smithay::backend::renderer::utils::CommitCounter;

    use super::*;

    fn solid(x: i32, y: i32, w: i32, h: i32) -> SceneElement {
        SceneElement::Solid(SolidColorRenderElement::new(
            Id::new(),
            Rectangle::<i32, Physical>::new(Point::from((x, y)), (w, h).into()),
            CommitCounter::default(),
            [1.0; 4],
            Kind::Unspecified,
        ))
    }

    fn xf(divisor: u8) -> ZoomXform {
        ZoomXform {
            divisor,
            pivot: Point::from((960, 540)),
            crop: Rectangle::new(Point::from((0, 0)), (1920, 1080).into()),
            scale: 1.0,
        }
    }

    #[test]
    fn the_pivot_is_the_centre_of_the_work_area() {
        assert_eq!(pivot_of((0, 0, 1920, 1080)), (960, 540));
        // バーで上 32 px を引いた作業領域。
        assert_eq!(pivot_of((0, 32, 1920, 1048)), (960, 556));
        assert_eq!(pivot_of((-1920, 0, 1920, 1080)), (-960, 540));
        // 奇数は切り捨て。
        assert_eq!(pivot_of((0, 0, 101, 51)), (50, 25));
    }

    #[test]
    fn levels_step_within_range_and_stop_at_the_ends() {
        // 3 段（等倍・1/2・1/4）。
        assert_eq!(step_level(0, 3, 1), Some(1));
        assert_eq!(step_level(1, 3, 1), Some(2));
        assert_eq!(step_level(2, 3, 1), None);
        assert_eq!(step_level(2, 3, -1), Some(1));
        assert_eq!(step_level(0, 3, -1), None);
        // 等倍だけの設定では動かない。
        assert_eq!(step_level(0, 1, 1), None);
        assert_eq!(step_level(0, 1, -1), None);
        // 範囲外の現在値（リロードで段階が減った）も動かさない。
        assert_eq!(step_level(5, 2, 1), None);
    }

    #[test]
    fn without_a_transform_the_elements_are_untouched() {
        let list = vec![solid(10, 10, 100, 100), solid(500, 20, 40, 40)];
        let ids: Vec<_> = list.iter().map(|e| Element::id(e).clone()).collect();
        let out = zoom_elements(list, None);
        assert_eq!(out.len(), 2);
        let out_ids: Vec<_> = out.iter().map(|e| Element::id(e).clone()).collect();
        assert_eq!(out_ids, ids);
    }

    #[test]
    fn a_zoomed_element_is_shrunk_about_the_pivot_within_one_pixel_of_the_pure_map() {
        // 画面全体の矩形は、基準点（画面の中心）を中心に半分になる。
        let out = zoom_elements(vec![solid(0, 0, 1920, 1080)], Some(&xf(2)));
        assert_eq!(out.len(), 1);
        let g = Element::geometry(&out[0], Scale::from(1.0));
        // 純粋な写像（`contrast_policy::zoom_rect`）の結果。GPU 側の丸めとの差は ±1 px。
        let want = contrast_policy::zoom_rect(
            contrast_core::Rect::new(0, 0, 1920, 1080).unwrap(),
            (960, 540),
            contrast_policy::ZoomLevel::new(2).unwrap(),
        )
        .unwrap();
        assert!(
            (g.loc.x - want.x()).abs() <= 1 && (g.loc.y - want.y()).abs() <= 1,
            "{g:?} {want:?}"
        );
        assert!(
            (g.size.w - want.width()).abs() <= 1 && (g.size.h - want.height()).abs() <= 1,
            "{g:?} {want:?}"
        );
    }

    #[test]
    fn elements_that_end_up_outside_the_crop_are_dropped_and_the_order_is_kept() {
        let far = solid(100_000, 100_000, 10, 10);
        let near = solid(900, 500, 120, 80);
        let near_id = Element::id(&near).clone();
        let out = zoom_elements(vec![far, near], Some(&xf(2)));
        assert_eq!(out.len(), 1);
        assert_eq!(Element::id(&out[0]).clone(), near_id);
    }

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect::new(x, y, w, h).unwrap()
    }

    #[test]
    fn a_real_point_is_inside_exactly_when_its_floor_is() {
        let r = rect(10, 20, 30, 40);
        for (p, inside) in [
            ((10.0, 20.0), true),
            ((39.999, 59.999), true),
            ((40.0, 30.0), false),
            ((20.0, 60.0), false),
            ((9.999, 30.0), false),
            ((20.0, 19.999), false),
            ((-5.5, 30.0), false),
        ] {
            assert_eq!(rect_contains_f64(r, p), inside, "{p:?}");
        }
    }

    #[test]
    fn picking_at_the_identity_is_the_plain_hit_test_with_the_top_window_first() {
        // 下: 0、上: 1（重なる）。
        let windows = [(0u32, rect(0, 0, 500, 500)), (1, rect(200, 200, 500, 500))];
        let id = ZoomLevel::IDENTITY;
        assert_eq!(
            pick_window(&windows, (300.0, 300.0), (960, 540), id),
            Some(1)
        );
        assert_eq!(
            pick_window(&windows, (100.0, 100.0), (960, 540), id),
            Some(0)
        );
        assert_eq!(pick_window(&windows, (900.0, 900.0), (960, 540), id), None);
    }

    #[test]
    fn picking_while_zoomed_uses_the_inverse_map() {
        // 1/2、基準点 (960, 540)。等倍の x 1920〜2400 の窓は、画面の x 1440〜1680 に出る。
        let level = ZoomLevel::new(2).unwrap();
        let windows = [
            (7u32, rect(1920, 400, 480, 300)),
            (8, rect(0, 0, 960, 1080)),
        ];
        // 画面の (1500, 600) → 等倍の (2040, 660)（窓 7）。
        assert_eq!(
            pick_window(&windows, (1500.0, 600.0), (960, 540), level),
            Some(7)
        );
        // 等倍だったら (1500, 600) は何も無い（窓 8 は x < 960）。
        assert_eq!(
            pick_window(&windows, (1500.0, 600.0), (960, 540), ZoomLevel::IDENTITY),
            None
        );
        // 画面の (700, 300) → 等倍の (440, 60)（窓 8）。
        assert_eq!(
            pick_window(&windows, (700.0, 300.0), (960, 540), level),
            Some(8)
        );
        // 基準点は動かない: 画面の中心はどちらでも等倍の中心。
        assert_eq!(
            pick_window(&windows, (960.0, 540.0), (960, 540), level),
            None
        );
    }

    #[test]
    fn the_client_gets_the_world_local_coordinates_while_the_cursor_stays_on_screen() {
        // 画面の位置 (1500, 600) → 等倍の (2040, 660)。面の本当の原点が (1920, 400) なら、面内座標は (120, 260)。
        let (pos, world, origin) = ((1500.0, 600.0), (2040.0, 660.0), (1920.0, 400.0));
        let given = client_surface_origin(origin, pos, world);
        // Smithay が送る面内座標 = location − 渡した原点。
        let local = (pos.0 - given.0, pos.1 - given.1);
        assert_eq!(local, (120.0, 260.0));
        // 等倍（pos == world）なら原点は変わらない。
        assert_eq!(
            client_surface_origin(origin, (100.0, 50.0), (100.0, 50.0)),
            origin
        );
    }

    #[test]
    fn a_window_beyond_the_screen_comes_into_view_when_zoomed_out() {
        // 画面の右外（x = 1920〜2400）にある窓は、1/2 で画面の中に入る（基準点が中心）。
        let out = zoom_elements(vec![solid(1920, 400, 480, 300)], Some(&xf(2)));
        assert_eq!(out.len(), 1);
        let g = Element::geometry(&out[0], Scale::from(1.0));
        assert!(g.loc.x >= 1400 && g.loc.x + g.size.w <= 1920, "{g:?}");
    }
}
