//! 1 フレームの描画: 背景色で塗り、Space 上の窓を重ね、フォーカス窓のタイルの縁に枠を描く。

use std::fmt;
use std::time::Duration;

use smithay::{
    backend::egl::EGLDevice,
    backend::renderer::{
        ImportDma,
        damage::OutputDamageTracker,
        element::{
            AsRenderElements, Kind, memory::MemoryRenderBufferRenderElement, render_elements,
            solid::SolidColorRenderElement, surface::WaylandSurfaceRenderElement,
        },
        gles::{GlesRenderer, GlesTarget, Uniform, element::PixelShaderElement},
    },
    desktop::{LayerSurface, layer_map_for_output},
    output::Output,
    utils::{Logical, Rectangle, Scale},
    wayland::dmabuf::{DmabufFeedback, DmabufFeedbackBuilder},
    wayland::shell::wlr_layer::Layer,
};

use crate::state::Contrast;

mod look;
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
}

/// 出力 1 枚分の要素を前から順に集める。`cursor` があれば最前面。
pub fn scene_elements(
    state: &mut Contrast,
    renderer: &mut GlesRenderer,
    output: &Output,
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
    // 入力メソッドのポップアップ（候補ウィンドウ）は、カーソルの次に前面（枠・窓・レイヤー面より前）。
    elements.extend(state.ime_popup_elements(renderer, scale));
    match shaders.as_ref().filter(|_| radius > 0) {
        Some(sh) => elements.extend(ring_elements(state, sh, radius)),
        None => elements.extend(border_elements(state).into_iter().map(SceneElement::Solid)),
    }

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
                    loc.to_physical_precise_round(scale),
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
        .filter(|_| radius > 0 && !state.has_visible_fullscreen());
    for e in state
        .space
        .render_elements_for_region(renderer, &geo, scale, 1.0)
    {
        elements.push(match round {
            Some(sh) => {
                SceneElement::Rounded(RoundedSurface::new(e, sh.rounded.clone(), radius as f32))
            }
            None => SceneElement::Surface(e),
        });
    }

    if let Some(sh) = shaders.as_ref() {
        elements.extend(shadow_elements(state, sh, &appearance, radius));
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
    let elements = scene_elements(state, renderer, output, Vec::new())?;
    damage_tracker
        .render_output(renderer, framebuffer, 0, &elements, background)
        .map(|_| ())
        .map_err(|e| RenderError::Render(e.to_string()))
}

/// 提出したあとの後始末: クライアントへ frame コールバックを送り、Space・popup・レイヤーを整理する。
pub fn finish_frame(state: &mut Contrast, output: &Output) {
    state.space.elements().for_each(|window| {
        window.send_frame(
            output,
            state.start_time.elapsed(),
            Some(Duration::ZERO),
            |_, _| Some(output.clone()),
        )
    });

    // 入力メソッドのポップアップにも frame コールバックを送る（消えたものは外す）。
    state.ime_popups.retain(|p| p.alive());
    for popup in &state.ime_popups {
        smithay::desktop::utils::send_frames_surface_tree(
            popup.wl_surface(),
            output,
            state.start_time.elapsed(),
            Some(Duration::ZERO),
            |_, _| Some(output.clone()),
        );
    }

    state.space.refresh();
    state.popups.cleanup();
    smithay::desktop::layer_map_for_output(output).cleanup();
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
fn ring_elements(state: &mut Contrast, sh: &Shaders, radius: i32) -> Vec<SceneElement> {
    let b = state.policy.border_width();
    if b <= 0 {
        return Vec::new();
    }
    let ring = |rect: contrast_core::Rect, thickness: i32, color: [f32; 4]| {
        SceneElement::Pixel(PixelShaderElement::new(
            sh.ring.clone(),
            logical(rect.x(), rect.y(), rect.width(), rect.height()),
            None,
            1.0,
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
    if let Some(frame) = state.focused_tile() {
        out.push(ring(
            crate::geometry::outset(frame, b),
            b,
            state.policy.focus_border_color(),
        ));
    }
    if let Some(color) = state.policy.unfocused_border_color() {
        let thin = unfocused_width(b, state.policy.appearance().inactive_border_width);
        if thin > 0 {
            for frame in state.unfocused_tiles() {
                out.push(ring(crate::geometry::outset(frame, thin), thin, color));
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
) -> Vec<SceneElement> {
    let s = appearance.shadow;
    if s.blur <= 0 {
        return Vec::new();
    }
    let border = appearance.border_width.max(0);
    let frame_radius = (radius + border) as f32;
    let mut frames: Vec<contrast_core::Rect> = state.focused_tile().into_iter().collect();
    frames.extend(state.unfocused_tiles());
    frames
        .into_iter()
        // 影は枠の外側の矩形に付ける。
        .map(|f| crate::geometry::outset(f, border))
        .map(|f| {
            let area = logical(
                f.x() - s.blur + s.offset_x,
                f.y() - s.blur + s.offset_y,
                f.width() + 2 * s.blur,
                f.height() + 2 * s.blur,
            );
            SceneElement::Pixel(PixelShaderElement::new(
                sh.shadow.clone(),
                area,
                None,
                1.0,
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
fn border_elements(state: &mut Contrast) -> Vec<SolidColorRenderElement> {
    let b = state.policy.border_width();
    let mut bands: Vec<(Band, [f32; 4])> = Vec::new();
    // 枠は窓（フレーム）の外側に描く。
    if let Some(tile) = state.focused_tile()
        && let Some(es) = edges(crate::geometry::outset(tile, b), b)
    {
        let color = state.policy.focus_border_color();
        bands.extend(es.into_iter().map(|e| (e, color)));
    }
    if let Some(color) = state.policy.unfocused_border_color() {
        for tile in state.unfocused_tiles() {
            let thin = unfocused_width(b, state.policy.appearance().inactive_border_width);
            if let Some(es) = edges(crate::geometry::outset(tile, thin), thin) {
                bands.extend(es.into_iter().map(|e| (e, color)));
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
        .map(|(buffer, ((ex, ey, ew, eh), color))| {
            buffer.update((ew, eh), color);
            SolidColorRenderElement::from_buffer(buffer, (ex, ey), 1.0, 1.0, Kind::Unspecified)
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
