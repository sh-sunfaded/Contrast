//! 1 フレームの描画: 背景色で塗り、Space 上の窓を重ね、フォーカス窓のタイルの縁に枠を描く。

use std::fmt;
use std::time::Duration;

use smithay::{
    backend::allocator::Fourcc,
    backend::egl::EGLDevice,
    backend::renderer::{
        Bind, ExportMem, ImportDma, Offscreen, TextureMapping,
        damage::OutputDamageTracker,
        element::{
            AsRenderElements, Kind, memory::MemoryRenderBufferRenderElement, render_elements,
            solid::SolidColorRenderElement, surface::WaylandSurfaceRenderElement,
            utils::CropRenderElement,
        },
        gles::{GlesRenderer, GlesTarget, Uniform, element::PixelShaderElement},
    },
    desktop::{LayerSurface, layer_map_for_output},
    output::Output,
    utils::{Buffer, Logical, Physical, Point, Rectangle, Scale, Size},
    wayland::dmabuf::{DmabufFeedback, DmabufFeedbackBuilder},
    wayland::shell::wlr_layer::Layer,
};

use crate::state::Contrast;

mod look;
pub mod offscreen;
mod rounded;

use look::Shaders;
use rounded::RoundedSurface;

/// linux-dmabuf の既定のフィードバック: 描画デバイス（`renderer` の EGL デバイスの render node）と、
/// レンダラーが受け付けるフォーマット。デバイスを特定できなければ `None`（v3 で公開する）。
pub fn dmabuf_feedback(renderer: &GlesRenderer) -> Option<DmabufFeedback> {
    let device = EGLDevice::device_for_display(renderer.egl_context().display()).ok()?;
    let node = device.try_get_render_node().ok().flatten()?;
    let formats = renderer.dmabuf_formats();
    DmabufFeedbackBuilder::new(node.dev_id(), formats.iter().copied())
        .build()
        .ok()
}

/// 描画の失敗。バックエンド（winit / DRM）が受けて、ログに出すか、フレームを諦めるかを決める。
#[derive(Debug)]
pub enum RenderError {
    /// 描画先（フレームバッファ）を束縛できなかった。
    Bind(String),
    /// 要素の描画に失敗した。
    Render(String),
    /// 描き終えたバッファの提出に失敗した。
    Submit(String),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::Bind(e) => write!(f, "bind failed: {e}"),
            RenderError::Render(e) => write!(f, "render failed: {e}"),
            RenderError::Submit(e) => write!(f, "submit failed: {e}"),
        }
    }
}

impl std::error::Error for RenderError {}

render_elements! {
    /// DRM の `DrmOutput::render_frame` と winit の描画に渡す要素。前から順に、カーソル・枠・上のレイヤー・窓・影・下のレイヤー。
    pub SceneElement<=GlesRenderer>;
    /// 枠の帯と、簡易カーソル。
    Solid=SolidColorRenderElement,
    /// テーマのカーソル。
    Memory=MemoryRenderBufferRenderElement<GlesRenderer>,
    /// 窓・レイヤー面・クライアントが指定したカーソルの面。
    Surface=WaylandSurfaceRenderElement<GlesRenderer>,
    /// 角丸で描く窓の面。
    Rounded=RoundedSurface,
    /// 枠の輪と影（ピクセルシェーダ）。
    Pixel=PixelShaderElement,
    /// 開く・閉じる途中の窓（オフスクリーンのテクスチャを拡大・半透明にしたもの）。
    Anim=crate::window_anim::AnimElement,
    /// オーバービューで、ワークスペースの範囲に切り抜いた窓・レイヤー面。
    CroppedSurface=CropRenderElement<WaylandSurfaceRenderElement<GlesRenderer>>,
    /// スクリーンショットの UI で止めた出力の絵。
    Frozen=smithay::backend::renderer::element::texture::TextureRenderElement<smithay::backend::renderer::gles::GlesTexture>,
}

render_elements! {
    /// 1 枚の描画先に複数の出力を並べて描くとき（winit で 1 窓を複数の出力に分ける検証用）の要素。
    /// 出力ごとの要素を、その出力の範囲で切り抜く（スクロール列の画面外の窓が隣の出力に出ないように）。
    pub FrameElement<=GlesRenderer>;
    /// 切り抜かない要素（カーソルなど）。
    Scene=SceneElement,
    /// 出力の範囲で切り抜いた要素。
    Cropped=CropRenderElement<SceneElement>,
}

/// 出力 `output` の要素を、描画先の原点が全体の論理座標の `origin` のときの位置で集め、出力の範囲で切り抜く。
pub fn cropped_output_elements(
    state: &mut Contrast,
    renderer: &mut GlesRenderer,
    output: &Output,
    origin: Point<i32, Logical>,
) -> Result<Vec<FrameElement>, RenderError> {
    let Some(geo) = state.space.output_geometry(output) else {
        return Ok(Vec::new());
    };
    let scale = output.current_scale().fractional_scale();
    let crop: Rectangle<i32, Physical> =
        Rectangle::new(geo.loc - origin, geo.size).to_physical_precise_round(scale);
    let elements = scene_elements(state, renderer, output, origin, Vec::new())?;
    Ok(elements
        .into_iter()
        .filter_map(|e| CropRenderElement::from_element(e, Scale::from(scale), crop))
        .map(FrameElement::Cropped)
        .collect())
}

/// 出力 1 枚分の要素を前から順に集める。`cursor` があれば最前面。
/// `origin` は描画先の左上の、全体の論理座標（出力ごとの描画先なら出力の位置）。窓はこの出力に表示中のものだけ。
pub fn scene_elements(
    state: &mut Contrast,
    renderer: &mut GlesRenderer,
    output: &Output,
    origin: Point<i32, Logical>,
    cursor: Vec<SceneElement>,
) -> Result<Vec<SceneElement>, RenderError> {
    let appearance = state.policy.appearance();
    let wants_shaders = appearance.radius > 0 || appearance.shadow.blur > 0;
    let shaders = if wants_shaders {
        Shaders::get(renderer)
    } else {
        None
    };
    let radius = if shaders.is_some() {
        appearance.radius.max(0)
    } else {
        0
    };
    let scale = output.current_scale().fractional_scale();
    let Some(geo) = state.space.output_geometry(output) else {
        return Err(RenderError::Render("output is not mapped".to_string()));
    };

    let mut elements: Vec<SceneElement> = cursor;
    // スクリーンショットの UI が開いていれば、それだけを描く（niri と同じ。カーソルは前面）。
    if let Some(ui) = state.screenshot_ui_elements(renderer, output, origin) {
        elements.extend(ui);
        return Ok(elements);
    }
    // ホットキーのオーバーレイ（niri と同じく、窓・レイヤー面より前）。
    {
        let policy = state.policy.clone();
        let rows = || crate::hotkey_overlay::rows(&policy.keybindings(), policy.mod_key());
        if let Some(e) = state
            .hotkey_overlay
            .element(renderer, output, geo, origin, rows)
        {
            elements.push(SceneElement::Frozen(e));
        }
    }
    // オーバービューが見えていれば、それを描く（niri と同じく、オーバーレイとカーソルは前面）。
    if let Some(overview) = state.overview_elements(renderer, output, origin) {
        elements.extend(overview);
        return Ok(elements);
    }
    // 入力メソッドのポップアップ（候補ウィンドウ）は、カーソルの次に前面（枠・窓・レイヤー面より前）。
    elements.extend(state.ime_popup_elements(renderer, scale, origin));
    match shaders.as_ref().filter(|_| radius > 0) {
        Some(sh) => elements.extend(ring_elements(state, sh, radius, output, origin)),
        None => elements.extend(
            border_elements(state, output, origin)
                .into_iter()
                .map(SceneElement::Solid),
        ),
    }
    // レイヤー面の位置は出力の中の座標なので、描画先の原点からの出力のずれを足す。
    let output_offset = geo.loc - origin;

    let layer_map = layer_map_for_output(output);
    let (lower, upper): (Vec<&LayerSurface>, Vec<&LayerSurface>) = layer_map
        .layers()
        .rev()
        .partition(|s| matches!(s.layer(), Layer::Background | Layer::Bottom));
    let layer_elements = |renderer: &mut GlesRenderer, layers: Vec<&LayerSurface>| {
        let mut out = Vec::new();
        for surface in layers {
            let Some(loc) = layer_map.layer_geometry(surface).map(|g| g.loc) else {
                continue;
            };
            out.extend(
                AsRenderElements::<GlesRenderer>::render_elements::<
                    WaylandSurfaceRenderElement<GlesRenderer>,
                >(
                    surface,
                    renderer,
                    (loc + output_offset).to_physical_precise_round(scale),
                    Scale::from(scale),
                    1.0,
                )
                .into_iter()
                .map(SceneElement::Surface),
            );
        }
        out
    };
    elements.extend(layer_elements(renderer, upper));

    // 窓。角丸のときは、全画面の窓があるあいだは切り抜かない。
    let round = shaders
        .as_ref()
        .filter(|_| radius > 0 && !state.has_visible_fullscreen(output));
    // 閉じる途中・開く途中の窓（niri の window-close／window-open）。開く途中の窓は、生の要素の代わりに描く。
    let (opening, closing) = {
        let shell_windows = &state.shell.windows;
        state
            .window_anims
            .elements(renderer, output, origin, |k| shell_windows.contains_key(&k))
    };
    elements.extend(closing.into_iter().map(SceneElement::Anim));
    let replaced: Vec<contrast_core::ExternalKey> = opening.iter().map(|(k, _)| *k).collect();
    elements.extend(opening.into_iter().map(|(_, e)| SceneElement::Anim(e)));
    // この出力に表示中の窓だけを、上に描かれるものから（Space の重なりの順）。
    let on_output: Vec<smithay::desktop::Window> = {
        let ids: Vec<contrast_core::WindowId> =
            state.windows_on(output).iter().map(|w| w.id).collect();
        let view = state.core.view();
        ids.iter()
            .filter_map(|id| view.window_key(*id))
            .filter(|k| !replaced.contains(k))
            .filter_map(|k| state.shell.windows.get(&k).cloned())
            .collect()
    };
    let window_elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> = state
        .space
        .elements()
        .rev()
        .filter(|w| on_output.contains(w))
        .filter_map(|w| {
            let loc = state.space.element_location(w)? - w.geometry().loc - origin;
            Some(AsRenderElements::<GlesRenderer>::render_elements::<
                WaylandSurfaceRenderElement<GlesRenderer>,
            >(
                w,
                renderer,
                loc.to_physical_precise_round(scale),
                Scale::from(scale),
                1.0,
            ))
        })
        .flatten()
        .collect();
    for e in window_elements {
        elements.push(match round {
            Some(sh) => {
                SceneElement::Rounded(RoundedSurface::new(e, sh.rounded.clone(), radius as f32))
            }
            None => SceneElement::Surface(e),
        });
    }

    if let Some(sh) = shaders.as_ref() {
        elements.extend(shadow_elements(
            state,
            sh,
            &appearance,
            radius,
            output,
            origin,
        ));
    }
    elements.extend(layer_elements(renderer, lower));
    Ok(elements)
}

/// 出力 1 枚分の絵（背景・窓・レイヤー面・枠・影）を `framebuffer` に描く。バックエンドには依存しない。
///
/// 描画先の用意（bind）と提出（submit / queue_frame）は呼び出し側のバックエンドが行う。
pub fn render_scene(
    state: &mut Contrast,
    renderer: &mut GlesRenderer,
    framebuffer: &mut GlesTarget<'_>,
    output: &Output,
    damage_tracker: &mut OutputDamageTracker,
) -> Result<(), RenderError> {
    let background = state.policy.background();
    let elements = match state.space.output_geometry(output) {
        Some(geo) => scene_elements(state, renderer, output, geo.loc, Vec::new())?,
        None => Vec::new(),
    };
    damage_tracker
        .render_output(renderer, framebuffer, 0, &elements, background)
        .map(|_| ())
        .map_err(|e| RenderError::Render(e.to_string()))
}

/// 読み出した出力の絵。RGBA 8 ビット、上の行から。
pub struct Shot {
    /// 幅（画素）。
    pub width: u32,
    /// 高さ（画素）。
    pub height: u32,
    /// 画素（`width × height × 4` バイト）。
    pub rgba: Vec<u8>,
}

/// `elements` を出力と同じ大きさのオフスクリーンに描いて、画素を読み出す（スクリーンショット）。
pub fn capture<E: smithay::backend::renderer::element::RenderElement<GlesRenderer>>(
    renderer: &mut GlesRenderer,
    output: &Output,
    elements: &[E],
    background: [f32; 4],
) -> Result<Shot, RenderError> {
    let fail = |e: &dyn fmt::Display| RenderError::Render(format!("screenshot: {e}"));
    let mode = output
        .current_mode()
        .ok_or_else(|| fail(&"output has no mode"))?;
    let size = Size::<i32, Buffer>::from((mode.size.w, mode.size.h));
    let (Ok(width), Ok(height)) = (u32::try_from(size.w), u32::try_from(size.h)) else {
        return Err(fail(&"bad output size"));
    };
    let mut texture = Offscreen::<smithay::backend::renderer::gles::GlesTexture>::create_buffer(
        renderer,
        Fourcc::Abgr8888,
        size,
    )
    .map_err(|e| fail(&e))?;
    let mapping = {
        let mut target = renderer.bind(&mut texture).map_err(|e| fail(&e))?;
        // GL のテクスチャに描いて読むと、上下が逆になる。上下を反転して描く（出力の変換に依らない。
        // 以前は出力の変換を使っていたので、winit（Flipped180）では正しく、DRM（Normal）では上下が逆になっていた）。
        let scale = output.current_scale().fractional_scale();
        OutputDamageTracker::new(mode.size, scale, smithay::utils::Transform::Flipped180)
            .render_output(renderer, &mut target, 0, elements, background)
            .map_err(|e| fail(&e))?;
        renderer
            .copy_framebuffer(&target, Rectangle::from_size(size), Fourcc::Abgr8888)
            .map_err(|e| fail(&e))?
    };
    let flipped = mapping.flipped();
    let bytes = renderer.map_texture(&mapping).map_err(|e| fail(&e))?;
    let row = (width as usize).saturating_mul(4);
    let rgba = if flipped {
        // GL は下の行から返す。
        bytes
            .chunks_exact(row.max(1))
            .rev()
            .flatten()
            .copied()
            .collect()
    } else {
        bytes.to_vec()
    };
    Ok(Shot {
        width,
        height,
        rgba,
    })
}

/// `screenshot-screen` が押されていれば、`elements` を読み出して保存する（押されていなければ何もしない）。
/// 失敗しても描画は止めない（ログに出す）。
pub fn take_screenshot_if_pending<
    E: smithay::backend::renderer::element::RenderElement<GlesRenderer>,
>(
    state: &mut Contrast,
    renderer: &mut GlesRenderer,
    output: &Output,
    elements: &[E],
) {
    if !std::mem::take(&mut state.screenshot_pending) {
        return;
    }
    let shot = match capture(renderer, output, elements, state.policy.background()) {
        Ok(shot) => shot,
        Err(e) => {
            tracing::warn!("{e}");
            return;
        }
    };
    crate::screenshot::deliver(state, shot.width, shot.height, &shot.rgba, true);
}

/// 提出したあとの後始末: クライアントへ frame コールバックを送り、Space・popup・レイヤーを整理する。
/// `outputs` はこのフレームで描いた出力。窓には、表示している出力（なければ先頭の出力）で 1 回だけ送る。
pub fn finish_frame(state: &mut Contrast, outputs: &[Output]) {
    let Some(first) = outputs.first() else {
        return;
    };
    let elapsed = state.start_time.elapsed();
    let windows: Vec<(smithay::desktop::Window, Output)> = state
        .space
        .elements()
        .map(|window| {
            let output = window
                .toplevel()
                .and_then(|t| state.key_of_surface(t.wl_surface()))
                .and_then(|k| state.core.view().window_id(k))
                .and_then(|id| state.output_of_window(id))
                .filter(|o| outputs.contains(o))
                .unwrap_or_else(|| first.clone());
            (window.clone(), output)
        })
        .collect();
    for (window, output) in windows {
        window.send_frame(&output, elapsed, Some(Duration::ZERO), |_, _| {
            Some(output.clone())
        });
    }

    // 入力メソッドのポップアップにも frame コールバックを送る（消えたものは外す）。
    state.ime_popups.retain(|p| p.alive());
    for popup in &state.ime_popups {
        smithay::desktop::utils::send_frames_surface_tree(
            popup.wl_surface(),
            first,
            elapsed,
            Some(Duration::ZERO),
            |_, _| Some(first.clone()),
        );
    }

    state.space.refresh();
    state.popups.cleanup();
    for output in outputs {
        smithay::desktop::layer_map_for_output(output).cleanup();
    }
    let _ = state.display_handle.flush_clients();
}

/// 帯の位置と大きさ `(x, y, w, h)`。
type Band = (i32, i32, i32, i32);

/// タイルの縁（窓とタイルの間の余白）を埋める 4 本の帯（上・下・左・右）。太さが足りなければ `None`。
fn edges(tile: contrast_core::Rect, b: i32) -> Option<[Band; 4]> {
    if b <= 0 || tile.width() <= b * 2 || tile.height() <= b * 2 {
        return None;
    }
    let (x, y, w, h) = (tile.x(), tile.y(), tile.width(), tile.height());
    Some([
        (x, y, w, b),
        (x, y + h - b, w, b),
        (x, y + b, b, h - 2 * b),
        (x + w - b, y + b, b, h - 2 * b),
    ])
}

/// 論理矩形。
fn logical(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
    Rectangle::new((x, y).into(), (w.max(1), h.max(1)).into())
}

/// 色をプリマルチプライドのアルファにする。
fn premultiplied(c: [f32; 4]) -> [f32; 4] {
    [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]]
}

/// 角丸のときの枠の輪（フォーカス窓と、設定があればフォーカス外の窓）。
fn ring_elements(
    state: &mut Contrast,
    sh: &Shaders,
    radius: i32,
    output: &Output,
    origin: Point<i32, Logical>,
) -> Vec<SceneElement> {
    let b = state.policy.border_width();
    if b <= 0 {
        return Vec::new();
    }
    let ring = |rect: contrast_core::Rect, thickness: i32, color: [f32; 4], alpha: f32| {
        SceneElement::Pixel(PixelShaderElement::new(
            sh.ring.clone(),
            logical(
                rect.x() - origin.x,
                rect.y() - origin.y,
                rect.width(),
                rect.height(),
            ),
            None,
            alpha,
            vec![
                Uniform::new("color", premultiplied(color)),
                Uniform::new("radius", radius as f32),
                Uniform::new("thickness", thickness as f32),
            ],
            Kind::Unspecified,
        ))
    };
    let mut out = Vec::new();
    // 枠は窓（フレーム）の外側に描く（niri のフォーカスリングと同じ）。
    if let Some(frame) = state.focused_tile(output) {
        let alpha = state.focused_open_look().map_or(1.0, |(a, _)| a);
        out.push(ring(
            crate::geometry::outset(frame, b),
            b,
            state.policy.focus_border_color(),
            alpha,
        ));
    }
    if let Some(color) = state.policy.unfocused_border_color() {
        let thin = unfocused_width(b, state.policy.appearance().inactive_border_width);
        if thin > 0 {
            for frame in state.unfocused_tiles(output) {
                out.push(ring(crate::geometry::outset(frame, thin), thin, color, 1.0));
            }
        }
    }
    out
}

/// 窓の影（全画面でない表示中の窓ごと）。
fn shadow_elements(
    state: &mut Contrast,
    sh: &Shaders,
    appearance: &contrast_policy::Appearance,
    radius: i32,
    output: &Output,
    origin: Point<i32, Logical>,
) -> Vec<SceneElement> {
    let s = appearance.shadow;
    if s.blur <= 0 {
        return Vec::new();
    }
    let border = appearance.border_width.max(0);
    let frame_radius = (radius + border) as f32;
    let focused_alpha = state.focused_open_look().map_or(1.0, |(a, _)| a);
    let mut frames: Vec<(contrast_core::Rect, f32)> = state
        .focused_tile(output)
        .into_iter()
        .map(|f| (f, focused_alpha))
        .collect();
    frames.extend(state.unfocused_tiles(output).into_iter().map(|f| (f, 1.0)));
    frames
        .into_iter()
        // 影は枠の外側の矩形に付ける。
        .map(|(f, alpha)| (crate::geometry::outset(f, border), alpha))
        .map(|(f, alpha)| {
            let area = logical(
                f.x() - s.blur + s.offset_x - origin.x,
                f.y() - s.blur + s.offset_y - origin.y,
                f.width() + 2 * s.blur,
                f.height() + 2 * s.blur,
            );
            SceneElement::Pixel(PixelShaderElement::new(
                sh.shadow.clone(),
                area,
                None,
                alpha,
                vec![
                    Uniform::new("color", premultiplied(s.color)),
                    Uniform::new("radius", frame_radius),
                    Uniform::new("blur", s.blur as f32),
                ],
                Kind::Unspecified,
            ))
        })
        .collect()
}

/// フォーカス外の枠の太さ。未指定は `border`、指定は 0〜`border` に収める。
fn unfocused_width(border: i32, inactive: Option<i32>) -> i32 {
    inactive.map_or(border, |w| w.clamp(0, border.max(0)))
}

/// フォーカス窓の枠（`focusColor`）と、設定があればフォーカス外の窓の枠（`borderColor`）の帯。
fn border_elements(
    state: &mut Contrast,
    output: &Output,
    origin: Point<i32, Logical>,
) -> Vec<SolidColorRenderElement> {
    let b = state.policy.border_width();
    let mut bands: Vec<(Band, [f32; 4], f32)> = Vec::new();
    let focused_alpha = state.focused_open_look().map_or(1.0, |(a, _)| a);
    // 枠は窓（フレーム）の外側に描く。
    if let Some(tile) = state.focused_tile(output)
        && let Some(es) = edges(crate::geometry::outset(tile, b), b)
    {
        let color = state.policy.focus_border_color();
        bands.extend(es.into_iter().map(|e| (e, color, focused_alpha)));
    }
    if let Some(color) = state.policy.unfocused_border_color() {
        for tile in state.unfocused_tiles(output) {
            let thin = unfocused_width(b, state.policy.appearance().inactive_border_width);
            if let Some(es) = edges(crate::geometry::outset(tile, thin), thin) {
                bands.extend(es.into_iter().map(|e| (e, color, 1.0)));
            }
        }
    }
    // 帯 1 本につきバッファ 1 つ（Id を保って描画の差分追跡を効かせる）。足りなければ増やす。
    while state.border_buffers.len() < bands.len() {
        state.border_buffers.push(Default::default());
    }
    state
        .border_buffers
        .iter_mut()
        .zip(bands)
        .map(|(buffer, ((ex, ey, ew, eh), color, alpha))| {
            buffer.update((ew, eh), color);
            SolidColorRenderElement::from_buffer(
                buffer,
                (ex - origin.x, ey - origin.y),
                1.0,
                alpha,
                Kind::Unspecified,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfocused_width_defaults_to_the_border_and_is_clamped() {
        assert_eq!(unfocused_width(4, None), 4);
        assert_eq!(unfocused_width(4, Some(2)), 2);
        assert_eq!(unfocused_width(4, Some(9)), 4);
        assert_eq!(unfocused_width(4, Some(-1)), 0);
        assert_eq!(unfocused_width(0, Some(3)), 0);
    }
}
