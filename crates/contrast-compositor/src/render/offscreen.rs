//! オフスクリーン描画の基盤: 要素をテクスチャに描く・画素を読み出す。
//!
//! winit では、窓の面を `bind` してから `submit` するまでの間にオフスクリーンを `bind` すると、窓の面の提出が壊れる
//! （`.cc-logs/report-niri-cside.md` の問題 1）。ここの関数は、その間では呼ばない（フレームの描画の前か、提出の後に呼ぶ）。

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, ExportMem, Offscreen, TextureMapping,
            damage::OutputDamageTracker,
            element::RenderElement,
            gles::{GlesRenderer, GlesTexture},
        },
    },
    utils::{Buffer, Physical, Rectangle, Size, Transform},
};

/// `elements`（位置は描画先の左上からの物理座標）を、大きさ `size`（物理 px）の、`clear` の色で塗ったテクスチャに描く。
pub fn render_to_texture<E: RenderElement<GlesRenderer>>(
    renderer: &mut GlesRenderer,
    size: Size<i32, Physical>,
    scale: f64,
    elements: &[E],
    clear: [f32; 4],
) -> Result<GlesTexture, String> {
    let buffer_size = Size::<i32, Buffer>::from((size.w.max(1), size.h.max(1)));
    let mut texture: GlesTexture =
        Offscreen::<GlesTexture>::create_buffer(renderer, Fourcc::Abgr8888, buffer_size)
            .map_err(|e| e.to_string())?;
    {
        let mut target = renderer.bind(&mut texture).map_err(|e| e.to_string())?;
        OutputDamageTracker::new(size, scale, Transform::Normal)
            .render_output(renderer, &mut target, 0, elements, clear)
            .map_err(|e| e.to_string())?;
    }
    Ok(texture)
}

/// `elements` を大きさ `size` の透明な描画先に描いて、RGBA 8 ビット（上の行から）で読み出す（スクリーンショット）。
pub fn render_to_rgba<E: RenderElement<GlesRenderer>>(
    renderer: &mut GlesRenderer,
    size: Size<i32, Physical>,
    scale: f64,
    elements: &[E],
    background: [f32; 4],
) -> Result<Vec<u8>, String> {
    let buffer_size = Size::<i32, Buffer>::from((size.w.max(1), size.h.max(1)));
    let mut texture: GlesTexture =
        Offscreen::<GlesTexture>::create_buffer(renderer, Fourcc::Abgr8888, buffer_size)
            .map_err(|e| e.to_string())?;
    let mapping = {
        let mut target = renderer.bind(&mut texture).map_err(|e| e.to_string())?;
        // GL のテクスチャに描いて読むと上下が逆になるので、上下を反転して描く。
        OutputDamageTracker::new(size, scale, Transform::Flipped180)
            .render_output(renderer, &mut target, 0, elements, background)
            .map_err(|e| e.to_string())?;
        renderer
            .copy_framebuffer(&target, Rectangle::from_size(buffer_size), Fourcc::Abgr8888)
            .map_err(|e| e.to_string())?
    };
    let flipped = mapping.flipped();
    let bytes = renderer.map_texture(&mapping).map_err(|e| e.to_string())?;
    let row = usize::try_from(buffer_size.w)
        .unwrap_or(0)
        .saturating_mul(4)
        .max(1);
    Ok(if flipped {
        // GL は下の行から返す。
        bytes.chunks_exact(row).rev().flatten().copied().collect()
    } else {
        bytes.to_vec()
    })
}

/// 実際の GL（`/dev/dri/renderD*` のオフスクリーン EGL）で、撮った画像の大きさが出力のモードの大きさと一致し、
/// 右端・下端の画素まで描かれることを確かめる。描画ノードが無い環境では、何もせず戻る。
#[cfg(test)]
mod tests {
    use smithay::{
        backend::renderer::{
            element::{Id, Kind, solid::SolidColorRenderElement},
            utils::CommitCounter,
        },
        output::{Mode, Output, PhysicalProperties, Subpixel},
        utils::{Physical, Point, Rectangle, Size, Transform},
    };

    use super::render_to_rgba;
    use crate::render::{capture, look::offscreen::renderer};

    const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

    /// 出力の全体を `RED` で塗る 1 要素。
    fn full(width: i32, height: i32) -> SolidColorRenderElement {
        SolidColorRenderElement::new(
            Id::new(),
            Rectangle::new(
                Point::from((0, 0)),
                Size::<i32, Physical>::from((width, height)),
            ),
            CommitCounter::default(),
            RED,
            Kind::Unspecified,
        )
    }

    fn pixel(rgba: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
        let i = (y * width + x) * 4;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    }

    #[test]
    fn render_to_rgba_covers_every_pixel_of_the_requested_size() {
        let Some(mut renderer) = renderer() else {
            eprintln!("skipped: no usable render node");
            return;
        };
        for (w, h) in [(1920, 1200), (1919, 1200), (928, 1076), (1, 1)] {
            let size = Size::<i32, Physical>::from((w, h));
            let rgba = render_to_rgba(&mut renderer, size, 1.0, &[full(w, h)], BLUE)
                .expect("render_to_rgba");
            let (wu, hu) = (w as usize, h as usize);
            assert_eq!(rgba.len(), wu * hu * 4, "{w}x{h}");
            // 四隅と右端・下端の列が、背景（青）でなく要素（赤）の色。
            for (x, y) in [(0, 0), (wu - 1, 0), (0, hu - 1), (wu - 1, hu - 1)] {
                assert_eq!(
                    pixel(&rgba, wu, x, y),
                    [255, 0, 0, 255],
                    "{w}x{h} at ({x},{y})"
                );
            }
        }
    }

    #[test]
    fn capture_matches_the_mode_size() {
        let Some(mut renderer) = renderer() else {
            eprintln!("skipped: no usable render node");
            return;
        };
        for (w, h) in [(1920, 1200), (1920, 1080), (2560, 1440)] {
            let output = Output::new(
                "test".to_owned(),
                PhysicalProperties {
                    size: (300, 200).into(),
                    subpixel: Subpixel::Unknown,
                    make: "t".into(),
                    model: "t".into(),
                },
            );
            let mode = Mode {
                size: (w, h).into(),
                refresh: 60_000,
            };
            output.change_current_state(Some(mode), Some(Transform::Normal), None, None);
            output.set_preferred(mode);
            let shot = capture(&mut renderer, &output, &[full(w, h)], BLUE).expect("capture");
            assert_eq!((shot.width, shot.height), (w as u32, h as u32));
            let (wu, hu) = (w as usize, h as usize);
            assert_eq!(shot.rgba.len(), wu * hu * 4, "{w}x{h}");
            assert_eq!(
                pixel(&shot.rgba, wu, wu - 1, hu - 1),
                [255, 0, 0, 255],
                "{w}x{h}"
            );
            assert_eq!(
                pixel(&shot.rgba, wu, wu - 1, 0),
                [255, 0, 0, 255],
                "{w}x{h}"
            );
        }
    }
}
