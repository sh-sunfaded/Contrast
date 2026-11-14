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

    #[test]
    fn a_window_beyond_the_screen_comes_into_view_when_zoomed_out() {
        // 画面の右外（x = 1920〜2400）にある窓は、1/2 で画面の中に入る（基準点が中心）。
        let out = zoom_elements(vec![solid(1920, 400, 480, 300)], Some(&xf(2)));
        assert_eq!(out.len(), 1);
        let g = Element::geometry(&out[0], Scale::from(1.0));
        assert!(g.loc.x >= 1400 && g.loc.x + g.size.w <= 1920, "{g:?}");
    }
}
