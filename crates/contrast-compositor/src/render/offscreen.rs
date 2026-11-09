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
