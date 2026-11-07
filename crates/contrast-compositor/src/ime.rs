//! 日本語入力（fcitx5）のための入力メソッド周りの処理: 入力メソッドのポップアップ（候補ウィンドウ）の管理・配置・描画。
//!
//! プロトコルの本体（text-input-v3、input-method-v2、virtual-keyboard-v1）は Smithay が処理する。
//! キーボードのグラブ（入力メソッドへキーを回す）も Smithay の `KeyboardGrab` で、`on_key` のフィルタ
//! （VT 切り替えとコンポジタのバインド）のあとに働く。入力メソッドが消える・グラブを外すと、Smithay がグラブを外す。
//!
//! ポップアップは、text-input が通知するカーソル矩形（窓の面の座標）の近くに置く。窓の位置は、アニメーション中の
//! 矩形ではなく、core が決めた最終の矩形を使う。

use smithay::{
    backend::renderer::{
        element::{Kind, surface::render_elements_from_surface_tree},
        gles::GlesRenderer,
    },
    desktop::utils::bbox_from_surface_tree,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Physical, Point, Rectangle, Size},
    wayland::input_method::{InputMethodHandler, PopupSurface},
};

use contrast_core::WindowState;

use crate::render::SceneElement;
use crate::state::Contrast;

/// ポップアップの左上の位置（出力 `bounds` の座標）。
///
/// 既定はカーソル矩形 `cursor` の真下に左端を合わせる。下に収まらなければ、真上に置く。
/// どちらの向きでも `bounds` の外にはみ出さないよう収める（ポップアップが `bounds` より大きければ、左上に合わせる）。
pub fn place_popup(
    cursor: Rectangle<i32, Logical>,
    popup: Size<i32, Logical>,
    bounds: Rectangle<i32, Logical>,
) -> Point<i32, Logical> {
    let below = cursor.loc.y.saturating_add(cursor.size.h);
    let fits_below = below.saturating_add(popup.h) <= bounds.loc.y.saturating_add(bounds.size.h);
    let y = if fits_below {
        below
    } else {
        cursor.loc.y.saturating_sub(popup.h)
    };
    (
        clamp_axis(cursor.loc.x, popup.w, bounds.loc.x, bounds.size.w),
        clamp_axis(y, popup.h, bounds.loc.y, bounds.size.h),
    )
        .into()
}

/// 長さ `len` の区間を、始点 `start` で置いたとき、`[lo, lo + extent)` の中に収まる始点。収まらなければ `lo`。
fn clamp_axis(start: i32, len: i32, lo: i32, extent: i32) -> i32 {
    let max = lo.saturating_add(extent).saturating_sub(len);
    if max < lo { lo } else { start.clamp(lo, max) }
}

impl InputMethodHandler for Contrast {
    fn new_popup(&mut self, surface: PopupSurface) {
        tracing::info!("ime: popup added");
        self.ime_popups.retain(|p| p != &surface);
        self.ime_popups.push(surface);
    }

    fn dismiss_popup(&mut self, surface: PopupSurface) {
        tracing::info!("ime: popup dismissed");
        self.ime_popups.retain(|p| p != &surface);
    }

    fn popup_repositioned(&mut self, _surface: PopupSurface) {
        // 位置は描画のたびに、カーソル矩形と窓の最終の矩形から求める。
    }

    fn parent_geometry(&self, parent: &WlSurface) -> Rectangle<i32, Logical> {
        self.final_window_rect(parent)
            .map(|r| Rectangle::new((r.x(), r.y()).into(), (r.width(), r.height()).into()))
            .unwrap_or_default()
    }
}

impl Contrast {
    /// 窓の面 `surface` の、core が決めた最終の窓の矩形（フレームそのもの。アニメーション中の矩形ではない）。
    fn final_window_rect(&self, surface: &WlSurface) -> Option<contrast_core::Rect> {
        let key = self.key_of_surface(surface)?;
        let view = self.core.view();
        let w = view.window(view.window_id(key)?)?;
        let tile = w.rect?;
        Some(match w.state {
            WindowState::Fullscreen => tile,
            state => self.frame_of(tile, state),
        })
    }

    /// 窓の面 `surface` の原点（面の座標 (0, 0)）の出力座標。最終の矩形から、窓の geometry のずれを引く。
    pub(crate) fn final_surface_origin(&self, surface: &WlSurface) -> Option<Point<i32, Logical>> {
        let rect = self.final_window_rect(surface)?;
        let offset = self
            .window_of_surface(surface)
            .map(|w| w.geometry().loc)
            .unwrap_or_default();
        Some((rect.x() - offset.x, rect.y() - offset.y).into())
    }

    /// ポップアップ `popup` を置く位置と面（出力座標）。親の窓が見つからない・面がまだ空なら `None`。
    fn popup_placement(&self, popup: &PopupSurface) -> Option<Point<i32, Logical>> {
        let parent = popup.get_parent()?.surface.clone();
        let origin = self.final_surface_origin(&parent)?;
        let size = bbox_from_surface_tree(popup.wl_surface(), (0, 0)).size;
        if size.w <= 0 || size.h <= 0 {
            return None;
        }
        let output = self.space.outputs().next()?;
        let bounds = self.space.output_geometry(output)?;
        let rect = popup.text_input_rectangle();
        let cursor = Rectangle::new(origin + rect.loc, rect.size);
        Some(place_popup(cursor, size, bounds))
    }

    /// 入力メソッドのポップアップの描画要素（窓より前面）。
    pub(crate) fn ime_popup_elements(
        &self,
        renderer: &mut GlesRenderer,
        scale: f64,
    ) -> Vec<SceneElement> {
        self.ime_popups
            .iter()
            .filter(|p| p.alive())
            .filter_map(|p| {
                let at: Point<i32, Physical> = self
                    .popup_placement(p)?
                    .to_f64()
                    .to_physical_precise_round(scale);
                Some(
                    render_elements_from_surface_tree(
                        renderer,
                        p.wl_surface(),
                        at,
                        scale,
                        1.0,
                        Kind::Unspecified,
                    )
                    .into_iter()
                    .map(SceneElement::Surface),
                )
            })
            .flatten()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    fn at(p: Point<i32, Logical>) -> (i32, i32) {
        (p.x, p.y)
    }

    /// 幅 1000・高さ 800 の出力。
    fn screen() -> Rectangle<i32, Logical> {
        rect(0, 0, 1000, 800)
    }

    #[test]
    fn the_popup_goes_right_below_the_cursor() {
        let p = place_popup(rect(100, 200, 2, 20), (300, 150).into(), screen());
        assert_eq!(at(p), (100, 220));
    }

    #[test]
    fn the_popup_flips_above_when_it_does_not_fit_below() {
        // 下の空き 800 − 740 = 60 < 150。
        let p = place_popup(rect(100, 720, 2, 20), (300, 150).into(), screen());
        assert_eq!(at(p), (100, 570));
        // ちょうど収まるときは下。
        let p = place_popup(rect(100, 630, 2, 20), (300, 150).into(), screen());
        assert_eq!(at(p), (100, 650));
    }

    #[test]
    fn the_popup_stays_inside_the_output_horizontally() {
        let p = place_popup(rect(900, 100, 2, 20), (300, 100).into(), screen());
        assert_eq!(p.x, 700);
        let p = place_popup(rect(-50, 100, 2, 20), (300, 100).into(), screen());
        assert_eq!(p.x, 0);
    }

    #[test]
    fn the_popup_stays_inside_when_neither_side_fits() {
        // カーソルが画面の上端近く・popup が高い: 下にも上にも収まらない。
        let p = place_popup(rect(10, 10, 2, 20), (300, 790).into(), screen());
        assert!(p.y >= 0 && p.y + 790 <= 800, "{p:?}");
    }

    #[test]
    fn a_popup_larger_than_the_output_is_aligned_to_the_top_left() {
        let p = place_popup(rect(10, 10, 2, 20), (2000, 1000).into(), screen());
        assert_eq!(at(p), (0, 0));
    }

    #[test]
    fn an_output_with_an_offset_origin_is_respected() {
        let bounds = rect(1000, 0, 800, 600);
        let p = place_popup(rect(1700, 100, 2, 20), (300, 100).into(), bounds);
        assert_eq!(p.x, 1500);
        let p = place_popup(rect(900, 100, 2, 20), (300, 100).into(), bounds);
        assert_eq!(p.x, 1000);
    }
}
