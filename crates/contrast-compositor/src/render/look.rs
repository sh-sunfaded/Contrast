//! 角丸・枠の輪・影の計算とシェーダ。
//!
//! 距離関数とアルファは純粋な関数（`rrect_distance`・`edge_coverage`・`ring_alpha`・`shadow_alpha`）で、
//! 同じ式を GLSL にも書いている（Rust 側の関数はテストで GLSL と突き合わせるためのもの）。単体テストは関数を、オフスクリーンのテスト（`tests::offscreen`）はシェーダを確かめる。

use smithay::backend::renderer::gles::{
    GlesError, GlesPixelProgram, GlesRenderer, GlesTexProgram, UniformName, UniformType,
};

/// 中心が原点で半分の大きさが `half`（幅/2、高さ/2）、角の半径が `radius` の角丸矩形への符号つき距離。
/// 内側が負、外側が正。`radius` は短い辺の半分に収める。
#[cfg(test)]
pub fn rrect_distance(p: (f32, f32), half: (f32, f32), radius: f32) -> f32 {
    let r = radius.clamp(0.0, half.0.min(half.1).max(0.0));
    let qx = p.0.abs() - half.0 + r;
    let qy = p.1.abs() - half.1 + r;
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - r
}

/// 距離 `d`（px）の画素が図形に覆われる割合（縁を 1px でなめらかにする）。
#[cfg(test)]
pub fn edge_coverage(d: f32) -> f32 {
    (0.5 - d).clamp(0.0, 1.0)
}

/// 枠の輪（外側の角丸矩形から内側を引いたもの）の画素のアルファ。`d_outer`・`d_inner` は各矩形への距離。
#[cfg(test)]
pub fn ring_alpha(d_outer: f32, d_inner: f32) -> f32 {
    edge_coverage(d_outer) * (1.0 - edge_coverage(d_inner))
}

/// 影のアルファ（0〜1）。`d` は影の元の図形への距離、`blur` はぼかしの幅。図形の縁（d=0）で 0.5、
/// 内側 `blur` で 1、外側 `blur` で 0。`blur` が 0 以下なら影なし。
#[cfg(test)]
pub fn shadow_alpha(d: f32, blur: f32) -> f32 {
    if blur <= 0.0 {
        return 0.0;
    }
    let t = ((d + blur) / (2.0 * blur)).clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

/// GLSL（距離関数）。3 つのシェーダで共通。
const GLSL_SDF: &str = r#"
float rrect(vec2 p, vec2 half_size, float radius) {
    float r = clamp(radius, 0.0, max(min(half_size.x, half_size.y), 0.0));
    vec2 q = abs(p) - half_size + vec2(r);
    return length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - r;
}
"#;

/// 窓の内容を角丸で切り抜くテクスチャシェーダ。Smithay の既定のテクスチャシェーダに角丸のマスクを足したもの。
/// `v_coords` は要素の画像の全体（0〜1）として扱う。
fn rounded_texture_source() -> String {
    format!(
        r#"#version 100

//_DEFINES_

#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif

precision mediump float;
#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

uniform float alpha;
varying vec2 v_coords;
uniform vec2 elem_size;
uniform float radius;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif
{GLSL_SDF}
void main() {{
    vec4 color = texture2D(tex, v_coords);

#if defined(NO_ALPHA)
    color = vec4(color.rgb, 1.0) * alpha;
#else
    color = color * alpha;
#endif

    vec2 p = (v_coords - vec2(0.5)) * elem_size;
    float d = rrect(p, elem_size * 0.5, radius);
    color = color * clamp(0.5 - d, 0.0, 1.0);

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}}
"#
    )
}

/// 枠の輪のピクセルシェーダ。要素の領域が外側の矩形（外側の半径は `radius + thickness`）。
fn ring_source() -> String {
    format!(
        r#"
precision mediump float;
uniform float alpha;
varying vec2 v_coords;
uniform vec2 size;
uniform vec4 color;
uniform float radius;
uniform float thickness;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif
{GLSL_SDF}
void main() {{
    vec2 p = (v_coords - vec2(0.5)) * size;
    float d_outer = rrect(p, size * 0.5, radius + thickness);
    float d_inner = rrect(p, size * 0.5 - vec2(thickness), radius);
    float a = clamp(0.5 - d_outer, 0.0, 1.0) * (1.0 - clamp(0.5 - d_inner, 0.0, 1.0));
    gl_FragColor = color * (a * alpha);
}}
"#
    )
}

/// 影のピクセルシェーダ。要素の領域は、元の図形を `blur` だけ広げたもの。
fn shadow_source() -> String {
    format!(
        r#"
precision mediump float;
uniform float alpha;
varying vec2 v_coords;
uniform vec2 size;
uniform vec4 color;
uniform float radius;
uniform float blur;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif
{GLSL_SDF}
void main() {{
    vec2 p = (v_coords - vec2(0.5)) * size;
    float d = rrect(p, size * 0.5 - vec2(blur), radius);
    float t = clamp((d + blur) / (2.0 * blur), 0.0, 1.0);
    float a = 1.0 - t * t * (3.0 - 2.0 * t);
    gl_FragColor = color * (a * alpha);
}}
"#
    )
}

/// コンパイル済みのシェーダ一式。
#[derive(Clone)]
pub struct Shaders {
    /// 窓の内容を角丸で切り抜く。
    pub rounded: GlesTexProgram,
    /// 枠の輪。
    pub ring: GlesPixelProgram,
    /// 影。
    pub shadow: GlesPixelProgram,
}

impl Shaders {
    fn compile(renderer: &mut GlesRenderer) -> Result<Self, GlesError> {
        let rounded = renderer.compile_custom_texture_shader(
            rounded_texture_source(),
            &[
                UniformName::new("elem_size", UniformType::_2f),
                UniformName::new("radius", UniformType::_1f),
            ],
        )?;
        let ring = renderer.compile_custom_pixel_shader(
            ring_source(),
            &[
                UniformName::new("color", UniformType::_4f),
                UniformName::new("radius", UniformType::_1f),
                UniformName::new("thickness", UniformType::_1f),
            ],
        )?;
        let shadow = renderer.compile_custom_pixel_shader(
            shadow_source(),
            &[
                UniformName::new("color", UniformType::_4f),
                UniformName::new("radius", UniformType::_1f),
                UniformName::new("blur", UniformType::_1f),
            ],
        )?;
        Ok(Self {
            rounded,
            ring,
            shadow,
        })
    }

    /// このレンダラーのシェーダ。最初に使うときにコンパイルして、レンダラーの EGL コンテキストに覚えさせる。
    /// コンパイルに失敗したら `None`（角丸・影なしで描く）。失敗も覚えて、毎フレームやり直さない。
    pub fn get(renderer: &mut GlesRenderer) -> Option<Self> {
        if let Some(cached) = renderer.egl_context().user_data().get::<Option<Shaders>>() {
            return cached.clone();
        }
        let compiled = match Self::compile(renderer) {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::warn!("shaders: compile failed, rounded corners and shadows are off: {e}");
                None
            }
        };
        renderer
            .egl_context()
            .user_data()
            .insert_if_missing(|| compiled.clone());
        compiled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-4;

    #[test]
    fn distance_is_signed_and_zero_on_the_edge() {
        // 100x60 の矩形（半分 50x30）、角丸なし。
        let half = (50.0, 30.0);
        assert!((rrect_distance((0.0, 0.0), half, 0.0) + 30.0).abs() < EPS);
        assert!(rrect_distance((50.0, 0.0), half, 0.0).abs() < EPS);
        assert!((rrect_distance((60.0, 0.0), half, 0.0) - 10.0).abs() < EPS);
        // 角: 角丸なしなら角は図形の内側の縁（距離 0）。
        assert!(rrect_distance((50.0, 30.0), half, 0.0).abs() < EPS);
    }

    #[test]
    fn rounded_corner_cuts_the_corner() {
        let half = (50.0, 30.0);
        // 半径 10 の角: 角の点は外側（正）、角から半径ぶん斜め内側の円周上は 0。
        assert!(rrect_distance((50.0, 30.0), half, 10.0) > 4.0);
        let on_arc = 10.0 * std::f32::consts::FRAC_1_SQRT_2;
        let p = (40.0 + on_arc, 20.0 + on_arc);
        assert!(rrect_distance(p, half, 10.0).abs() < EPS);
        // 辺の中央は角丸の影響を受けない。
        assert!(rrect_distance((50.0, 0.0), half, 10.0).abs() < EPS);
    }

    #[test]
    fn radius_is_limited_to_the_short_side() {
        let half = (50.0, 30.0);
        // 半径が大きすぎても、短い辺の半分（30）に収まって有限。
        let d = rrect_distance((0.0, 0.0), half, 1000.0);
        assert!((d + 30.0).abs() < EPS);
        // 負の半径は 0 と同じ。
        assert_eq!(
            rrect_distance((10.0, 5.0), half, -3.0),
            rrect_distance((10.0, 5.0), half, 0.0)
        );
    }

    #[test]
    fn coverage_is_one_inside_zero_outside_half_on_the_edge() {
        assert_eq!(edge_coverage(-5.0), 1.0);
        assert_eq!(edge_coverage(5.0), 0.0);
        assert!((edge_coverage(0.0) - 0.5).abs() < EPS);
    }

    #[test]
    fn ring_is_opaque_only_between_the_two_edges() {
        // 外側の縁から 2px 内側 → 輪の中、内側の図形の中 → 透明、外 → 透明。
        assert!(ring_alpha(-2.0, 2.0) > 0.99);
        assert!(ring_alpha(-6.0, -2.0) < 0.01);
        assert!(ring_alpha(3.0, 8.0) < 0.01);
    }

    #[test]
    fn shadow_alpha_falls_off_across_the_blur() {
        let blur = 20.0;
        assert_eq!(shadow_alpha(-blur, blur), 1.0);
        assert_eq!(shadow_alpha(-100.0, blur), 1.0);
        assert!((shadow_alpha(0.0, blur) - 0.5).abs() < EPS);
        assert_eq!(shadow_alpha(blur, blur), 0.0);
        assert_eq!(shadow_alpha(100.0, blur), 0.0);
        // 外へ行くほど単調に小さくなる。
        let mut prev = 1.0;
        for i in 0..=40 {
            let a = shadow_alpha(-20.0 + i as f32, blur);
            assert!(a <= prev + EPS);
            prev = a;
        }
    }

    #[test]
    fn no_blur_means_no_shadow() {
        assert_eq!(shadow_alpha(-10.0, 0.0), 0.0);
        assert_eq!(shadow_alpha(0.0, -3.0), 0.0);
    }
}

/// オフスクリーンの EGL（`/dev/dri/renderD*`）でシェーダを動かし、ピクセルを純粋な関数と突き合わせるテスト。
/// 描画ノードが開けない・EGL が使えない環境では、何もせず戻る。
#[cfg(test)]
pub(crate) mod offscreen {
    use super::*;
    use smithay::{
        backend::{
            allocator::{Fourcc, gbm::GbmDevice},
            egl::{EGLContext, EGLDisplay},
            renderer::{
                Bind, ExportMem, Frame, ImportMem, Offscreen, Renderer,
                gles::{GlesTexture, Uniform},
            },
        },
        utils::{Buffer, Physical, Rectangle, Size, Transform},
    };

    /// 描画ノードから GLES のレンダラーを作る。作れなければ `None`。
    pub(crate) fn renderer() -> Option<GlesRenderer> {
        let mut nodes: Vec<_> = std::fs::read_dir("/dev/dri")
            .ok()?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("renderD"))
            })
            .collect();
        nodes.sort();
        for path in nodes {
            let Ok(file) = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
            else {
                continue;
            };
            let Ok(gbm) = GbmDevice::new(file) else {
                continue;
            };
            // SAFETY: `gbm` は有効な GBM デバイスで、EGLDisplay が複製を持つ。
            let Ok(display) = (unsafe { EGLDisplay::new(gbm) }) else {
                continue;
            };
            let Ok(context) = EGLContext::new(&display) else {
                continue;
            };
            // SAFETY: このテストのスレッドだけで使う新しい EGL コンテキスト。
            if let Ok(r) = unsafe { GlesRenderer::new(context) } {
                return Some(r);
            }
        }
        None
    }

    /// `size` のテクスチャに `draw` で描いて、RGBA のバイト列を読む。
    fn render_to_pixels(
        renderer: &mut GlesRenderer,
        size: (i32, i32),
        draw: impl FnOnce(
            &mut smithay::backend::renderer::gles::GlesFrame<'_, '_>,
        ) -> Result<(), String>,
    ) -> Result<Vec<u8>, String> {
        let buf_size: Size<i32, Buffer> = size.into();
        let mut texture: GlesTexture = renderer
            .create_buffer(Fourcc::Abgr8888, buf_size)
            .map_err(|e| e.to_string())?;
        let mut target = renderer.bind(&mut texture).map_err(|e| e.to_string())?;
        let phys: Size<i32, Physical> = size.into();
        {
            let mut frame = renderer
                .render(&mut target, phys, Transform::Normal)
                .map_err(|e| e.to_string())?;
            frame
                .clear([0.0, 0.0, 0.0, 0.0].into(), &[Rectangle::from_size(phys)])
                .map_err(|e| e.to_string())?;
            draw(&mut frame)?;
            frame
                .finish()
                .map_err(|e| e.to_string())?
                .wait()
                .map_err(|_| "sync wait failed".to_string())?;
        }
        let mapping = renderer
            .copy_framebuffer(&target, Rectangle::from_size(buf_size), Fourcc::Abgr8888)
            .map_err(|e| e.to_string())?;
        let bytes = renderer.map_texture(&mapping).map_err(|e| e.to_string())?;
        Ok(bytes.to_vec())
    }

    /// 画素 `(x, y)` のアルファ（0〜1）。行は上から下、どちら向きでも対称な図形を使う。
    fn alpha_at(bytes: &[u8], width: i32, x: i32, y: i32) -> Option<f32> {
        let i = usize::try_from((y * width + x) * 4 + 3).ok()?;
        bytes.get(i).map(|a| f32::from(*a) / 255.0)
    }

    #[test]
    fn ring_and_shadow_shaders_match_the_pure_functions() -> Result<(), String> {
        let Some(mut renderer) = renderer() else {
            eprintln!("offscreen: no usable EGL render node, skipping");
            return Ok(());
        };
        let Some(shaders) = Shaders::get(&mut renderer) else {
            return Err("shaders failed to compile".to_string());
        };

        // 枠の輪: 64x64、内側の半径 8、太さ 6、不透明な赤。
        let (radius, thickness) = (8.0_f32, 6.0_f32);
        let px = render_to_pixels(&mut renderer, (64, 64), |frame| {
            let area = Rectangle::<i32, Physical>::from_size((64, 64).into());
            frame
                .render_pixel_shader_to(
                    &shaders.ring,
                    Rectangle::from_size((64.0, 64.0).into()),
                    area,
                    (64, 64).into(),
                    None,
                    1.0,
                    &[
                        Uniform::new("color", [1.0_f32, 0.0, 0.0, 1.0]),
                        Uniform::new("radius", radius),
                        Uniform::new("thickness", thickness),
                    ],
                )
                .map_err(|e| e.to_string())
        })?;
        for (x, y) in [
            (32, 1),
            (32, 4),
            (32, 8),
            (32, 32),
            (1, 32),
            (1, 1),
            (4, 4),
            (62, 62),
            (10, 3),
        ] {
            let p = (x as f32 + 0.5 - 32.0, y as f32 + 0.5 - 32.0);
            let want = ring_alpha(
                rrect_distance(p, (32.0, 32.0), radius + thickness),
                rrect_distance(p, (26.0, 26.0), radius),
            );
            let got = alpha_at(&px, 64, x, y).ok_or("pixel out of range")?;
            assert!(
                (got - want).abs() < 0.08,
                "ring ({x},{y}): got {got}, want {want}"
            );
        }

        // 影: 窓 64x64 を blur 16 で広げた 96x96 の領域、窓の角の半径 8。
        let blur = 16.0_f32;
        let px = render_to_pixels(&mut renderer, (96, 96), |frame| {
            let area = Rectangle::<i32, Physical>::from_size((96, 96).into());
            frame
                .render_pixel_shader_to(
                    &shaders.shadow,
                    Rectangle::from_size((96.0, 96.0).into()),
                    area,
                    (96, 96).into(),
                    None,
                    1.0,
                    &[
                        Uniform::new("color", [0.0_f32, 0.0, 0.0, 1.0]),
                        Uniform::new("radius", radius),
                        Uniform::new("blur", blur),
                    ],
                )
                .map_err(|e| e.to_string())
        })?;
        for (x, y) in [
            (48, 48),
            (48, 10),
            (48, 16),
            (48, 2),
            (2, 2),
            (20, 48),
            (90, 48),
        ] {
            let p = (x as f32 + 0.5 - 48.0, y as f32 + 0.5 - 48.0);
            let want = shadow_alpha(rrect_distance(p, (32.0, 32.0), radius), blur);
            let got = alpha_at(&px, 96, x, y).ok_or("pixel out of range")?;
            assert!(
                (got - want).abs() < 0.08,
                "shadow ({x},{y}): got {got}, want {want}"
            );
        }
        Ok(())
    }

    #[test]
    fn rounded_texture_shader_clips_the_corners() -> Result<(), String> {
        let Some(mut renderer) = renderer() else {
            eprintln!("offscreen: no usable EGL render node, skipping");
            return Ok(());
        };
        let Some(shaders) = Shaders::get(&mut renderer) else {
            return Err("shaders failed to compile".to_string());
        };
        // 白い不透明な 48x48 のテクスチャを、半径 12 の角丸で描く。
        let white = vec![255u8; 48 * 48 * 4];
        let texture = renderer
            .import_memory(&white, Fourcc::Abgr8888, (48, 48).into(), false)
            .map_err(|e| e.to_string())?;
        let radius = 12.0_f32;
        let px = render_to_pixels(&mut renderer, (48, 48), |frame| {
            let dst = Rectangle::<i32, Physical>::from_size((48, 48).into());
            frame
                .render_texture_from_to(
                    &texture,
                    Rectangle::from_size((48.0, 48.0).into()),
                    dst,
                    &[dst],
                    &[],
                    Transform::Normal,
                    1.0,
                    Some(&shaders.rounded),
                    &[
                        Uniform::new("elem_size", (48.0_f32, 48.0_f32)),
                        Uniform::new("radius", radius),
                    ],
                )
                .map_err(|e| e.to_string())
        })?;
        for (x, y) in [
            (0, 0),
            (47, 0),
            (0, 47),
            (47, 47),
            (24, 24),
            (24, 0),
            (0, 24),
            (3, 3),
            (6, 6),
            (12, 12),
        ] {
            let p = (x as f32 + 0.5 - 24.0, y as f32 + 0.5 - 24.0);
            let want = edge_coverage(rrect_distance(p, (24.0, 24.0), radius));
            let got = alpha_at(&px, 48, x, y).ok_or("pixel out of range")?;
            assert!(
                (got - want).abs() < 0.08,
                "rounded ({x},{y}): got {got}, want {want}"
            );
        }
        // 角は切れて、中央と辺の中央は残る。
        assert!(alpha_at(&px, 48, 0, 0).ok_or("px")? < 0.05);
        assert!(alpha_at(&px, 48, 24, 24).ok_or("px")? > 0.95);
        assert!(alpha_at(&px, 48, 24, 0).ok_or("px")? > 0.95);
        Ok(())
    }
}
