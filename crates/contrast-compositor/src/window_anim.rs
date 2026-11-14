//! 窓の開く・閉じるアニメーション（niri の既定: 開く 150ms・ease-out-expo、閉じる 150ms・ease-out-quad）。
//!
//! niri（`src/layout/opening_window.rs`・`closing_window.rs`）と同じく、窓をオフスクリーンのテクスチャに描いてから、
//! 透明度と拡大率を掛けて重ねる。開く: 透明度 = 進み、拡大率 = 進み / 2 + 0.5。閉じる: 透明度 = 1 − 進み、拡大率 = (1 − 進み) / 5 + 0.8。
//! どちらも窓の中心を基準に拡大する。
//!
//! 閉じる窓は、面が壊れたあとでは描けないので、毎フレーム、表示中の窓の要素（テクスチャを持つ）を覚えておき、
//! 窓が消えたら、その要素をテクスチャに描いて使う。オフスクリーンへの描画は、フレームの描画（winit の `bind`〜`submit`）の前に行う。

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use smithay::{
    backend::renderer::{
        Renderer,
        element::{
            AsRenderElements, Element, Id, Kind, solid::SolidColorRenderElement,
            surface::WaylandSurfaceRenderElement, texture::TextureRenderElement,
            utils::RescaleRenderElement,
        },
        gles::{GlesRenderer, GlesTexture},
        utils::CommitCounter,
    },
    output::Output,
    utils::{Logical, Physical, Point, Rectangle, Scale, Transform},
};

use contrast_core::{ExternalKey, WindowId};
use contrast_policy::{EasingCurve, MotionCurve, MotionKind, MotionSettings};

use crate::spring::{Spring, SpringParams};
use crate::state::Contrast;

/// 開くアニメーションの長さ（niri の既定）。
pub const OPEN_DURATION: Duration = Duration::from_millis(150);
/// 閉じるアニメーションの長さ（niri の既定）。
pub const CLOSE_DURATION: Duration = Duration::from_millis(150);

/// niri の ease-out-expo: `1 − 2^(−10t)`（t ≥ 1 で 1）。
pub fn ease_out_expo(t: f64) -> f64 {
    if t >= 1.0 {
        1.0
    } else {
        1.0 - 2f64.powf(-10.0 * t.max(0.0))
    }
}

/// ease-out-quad: `1 − (1 − t)^2`。
pub fn ease_out_quad(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t)
}

/// 開くときの `(透明度, 拡大率)`。`p` は曲線を通した進み。
pub fn open_look(p: f64) -> (f32, f64) {
    (p.clamp(0.0, 1.0) as f32, (p / 2.0 + 0.5).max(0.0))
}

/// 閉じるときの `(透明度, 拡大率)`。`p` は曲線を通した進み。
pub fn close_look(p: f64) -> (f32, f64) {
    let c = p.clamp(0.0, 1.0);
    ((1.0 - c) as f32, ((1.0 - c) / 5.0 + 0.8).max(0.0))
}

/// 開閉の動きの曲線。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OpenCloseCurve {
    /// 従来の固定の曲線（開く: ease-out-expo、閉じる: ease-out-quad）。`<Motion>` が無い設定のとき。
    Legacy,
    /// `<Motion>` で選んだ easing。
    Easing(EasingCurve),
    /// niri のバネ（`niriSpring`）。長さは収束するまで。
    Spring,
}

/// 開閉 1 回の曲線と長さ。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timing {
    /// 曲線。
    pub curve: OpenCloseCurve,
    /// 長さ（`Spring` は収束までの時間）。
    pub duration: Duration,
}

impl Timing {
    /// 経過 `elapsed` の、曲線を通した進み（0〜1）。`kind` は `Legacy` の曲線（開く・閉じる）を決める。
    pub fn value(&self, kind: MotionKind, elapsed: Duration) -> f64 {
        match self.curve {
            OpenCloseCurve::Legacy => {
                let f = fraction(elapsed, stretched(self.duration));
                if kind == MotionKind::Open {
                    ease_out_expo(f)
                } else {
                    ease_out_quad(f)
                }
            }
            OpenCloseCurve::Easing(curve) => f64::from(contrast_policy::motion::ease(
                curve,
                contrast_policy::motion::progress(elapsed, stretched(self.duration)),
            )),
            OpenCloseCurve::Spring => Spring::progress(SpringParams::movement())
                .value_at(elapsed.as_secs_f64() / slowdown())
                .clamp(0.0, 1.0),
        }
    }

    /// 検証用の遅さを含めた全体の長さ。
    pub fn total(&self) -> Duration {
        stretched(self.duration)
    }
}

/// 種別 `kind`（`Open` か `Close`）の動き。`None` は動かさない（即時）。
/// `<Motion>` が無い（`motion` が `None`）ときは従来どおり、`animation_ms != 0` なら 150ms の固定の曲線。
/// ある（`Some`）ときは Motion に従い、書かれていない種別・長さ 0 の easing は動かさない。
pub fn timing_for(
    kind: MotionKind,
    motion: Option<MotionSettings>,
    legacy_animation_ms: u32,
) -> Option<Timing> {
    let Some(settings) = motion else {
        return (legacy_animation_ms != 0).then_some(Timing {
            curve: OpenCloseCurve::Legacy,
            duration: if kind == MotionKind::Open {
                OPEN_DURATION
            } else {
                CLOSE_DURATION
            },
        });
    };
    let spec = settings.spec(kind)?;
    match spec.curve {
        MotionCurve::Easing(curve) => (spec.duration_ms != 0).then_some(Timing {
            curve: OpenCloseCurve::Easing(curve),
            duration: Duration::from_millis(u64::from(spec.duration_ms)),
        }),
        MotionCurve::NiriSpring => Some(Timing {
            curve: OpenCloseCurve::Spring,
            duration: Spring::progress(SpringParams::movement()).settle_time(),
        }),
    }
}

/// 検証用: 環境変数 `CONTRAST_DEBUG_ANIM_SLOWDOWN`（1 以上の数）で、開閉のアニメーションを遅くする（既定 1）。
fn slowdown() -> f64 {
    static SLOWDOWN: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    *SLOWDOWN.get_or_init(|| {
        std::env::var("CONTRAST_DEBUG_ANIM_SLOWDOWN")
            .ok()
            .and_then(|v| v.trim().parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 1.0)
            .unwrap_or(1.0)
    })
}

/// 長さ `total` を、検証用の遅さで伸ばしたもの。
fn stretched(total: Duration) -> Duration {
    total.mul_f64(slowdown())
}

/// 経過 `elapsed` の、長さ `total` に対する 0〜1 の割合。
fn fraction(elapsed: Duration, total: Duration) -> f64 {
    if total.is_zero() {
        return 1.0;
    }
    (elapsed.as_secs_f64() / total.as_secs_f64()).clamp(0.0, 1.0)
}

/// 窓の絵: テクスチャの左上を原点にした要素、その大きさ、全体の論理座標での描く位置と中心。
struct Snapshot {
    /// 要素（テクスチャを持つので、面が壊れても描ける）。
    elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>>,
    /// 要素を囲む矩形（物理 px。左上は窓の描画の原点からのずれ）。
    bbox: Rectangle<i32, Physical>,
    /// 窓の描画の原点（全体の論理座標）。
    origin: Point<i32, Logical>,
    /// 窓（geometry）の中心（全体の論理座標）。
    center: Point<i32, Logical>,
    /// 表示していた出力。
    output: Output,
    /// 出力の拡大率。
    scale: f64,
    /// 枠の帯（物理 px。左上は窓の描画の原点からのずれ）と色。閉じる窓の絵にだけ含める（niri はタイル全体を縮める）。
    bands: Vec<(Rectangle<i32, Physical>, [f32; 4])>,
    /// この窓の最後のフレームの減光の乗数（1 で減光なし）。開閉の絵は、画面で見えていたとおりに描く（一瞬明るくならない）。
    /// `screenshot-window` はこの値を使わない（`render::dim::snapshot_dim`）。
    dim: f32,
}

/// テクスチャに描いた窓。
struct Drawn {
    texture: GlesTexture,
    /// 枠の帯（閉じる窓だけ。`Snapshot::bands` と同じ）。
    bands: Vec<(Rectangle<i32, Physical>, [f32; 4])>,
    bbox: Rectangle<i32, Physical>,
    origin: Point<i32, Logical>,
    center: Point<i32, Logical>,
    output: Output,
    scale: f64,
}

/// 閉じている途中の窓。
struct Closing {
    started: Instant,
    /// 始めたときに決めた曲線と長さ。
    timing: Timing,
    snapshot: Option<Snapshot>,
    drawn: Option<Drawn>,
}

/// 窓の開閉のアニメーションの状態。
#[derive(Default)]
pub struct WindowAnims {
    /// 開いている途中の窓と、始めた時刻・曲線と長さ・今のフレームの絵。
    opening: BTreeMap<ExternalKey, (Instant, Timing, Option<Drawn>)>,
    /// 閉じている途中の窓。
    closing: Vec<Closing>,
    /// 表示中の窓の、最後のフレームの絵（閉じるときに使う）。
    last: BTreeMap<ExternalKey, Snapshot>,
    /// 中身を一度でも描いた窓（開くアニメーションは最初の 1 回だけ）。
    seen: BTreeSet<ExternalKey>,
}

/// 描く要素: テクスチャを、中心を基準に拡大したもの。
pub type AnimElement = RescaleRenderElement<TextureRenderElement<GlesTexture>>;

impl WindowAnims {
    /// 窓 `key` が消えた。最後の絵があり `timing`（`None` なら動かさない）があれば、閉じるアニメーションを始める。
    pub fn start_closing(&mut self, key: ExternalKey, timing: Option<Timing>) {
        self.seen.remove(&key);
        self.opening.remove(&key);
        let Some(snapshot) = self.last.remove(&key) else {
            return;
        };
        if let Some(timing) = timing {
            self.closing.push(Closing {
                started: Instant::now(),
                timing,
                snapshot: Some(snapshot),
                drawn: None,
            });
        }
    }

    /// 窓 `key` が開くアニメーションの途中で、絵があるか（あれば、その窓の生の要素は描かない）。
    pub fn replaces(&self, key: ExternalKey) -> bool {
        self.opening.get(&key).is_some_and(|(_, _, d)| d.is_some())
    }

    /// 窓 `key` が開く途中なら、今の `(透明度, 拡大率)`（枠と影にも同じものを掛ける。niri はタイルの枠ごとオフスクリーンに描く）。
    pub fn open_look_of(&self, key: ExternalKey) -> Option<(f32, f64)> {
        let (started, timing, drawn) = self.opening.get(&key)?;
        drawn.as_ref()?;
        let p = timing.value(MotionKind::Open, started.elapsed());
        Some(open_look(p))
    }

    /// 進行中のアニメーションがあるか（次のフレームも描く）。
    pub fn is_running(&self) -> bool {
        !self.opening.is_empty() || !self.closing.is_empty()
    }

    /// 出力 `output` に重ねる要素（描画先の左上が全体の論理座標の `origin`）。上に描くものから。
    pub fn elements(
        &self,
        renderer: &GlesRenderer,
        output: &Output,
        origin: Point<i32, Logical>,
        live: impl Fn(ExternalKey) -> bool,
    ) -> (
        Vec<(ExternalKey, AnimElement)>,
        Vec<crate::render::SceneElement>,
    ) {
        let now = Instant::now();
        let make = |d: &Drawn, alpha: f32, scale: f64| -> AnimElement {
            let at: Point<i32, Physical> = (d.origin - origin).to_physical_precise_round(d.scale);
            let loc = at + d.bbox.loc;
            let texture = TextureRenderElement::from_static_texture(
                Id::new(),
                renderer.context_id(),
                loc.to_f64(),
                d.texture.clone(),
                1,
                Transform::Normal,
                Some(alpha),
                None,
                Some(d.bbox.size.to_f64().to_logical(d.scale).to_i32_round()),
                None,
                Kind::Unspecified,
            );
            let center: Point<i32, Physical> =
                (d.center - origin).to_physical_precise_round(d.scale);
            RescaleRenderElement::from_element(texture, center, Scale::from(scale))
        };
        let opening = self
            .opening
            .iter()
            .filter(|(key, _)| live(**key))
            .filter_map(|(key, (started, timing, drawn))| {
                let d = drawn.as_ref().filter(|d| &d.output == output)?;
                let p = timing.value(MotionKind::Open, now.duration_since(*started));
                let (alpha, scale) = open_look(p);
                Some((*key, make(d, alpha, scale)))
            })
            .collect();
        let closing = self
            .closing
            .iter()
            .filter_map(|c| {
                let d = c.drawn.as_ref().filter(|d| &d.output == output)?;
                let p = c
                    .timing
                    .value(MotionKind::Close, now.duration_since(c.started));
                let (alpha, scale) = close_look(p);
                // 窓の絵と、同じ中心・拡大率・透明度の枠の帯（niri は枠も含めたタイルの絵を縮める）。
                let at: Point<i32, Physical> =
                    (d.origin - origin).to_physical_precise_round(d.scale);
                let center: Point<i32, Physical> =
                    (d.center - origin).to_physical_precise_round(d.scale);
                let mut out = vec![crate::render::SceneElement::Anim(make(d, alpha, scale))];
                out.extend(d.bands.iter().map(|(r, color)| {
                    let mut c = crate::render::premultiplied(*color);
                    c.iter_mut().for_each(|v| *v *= alpha);
                    let solid = SolidColorRenderElement::new(
                        Id::new(),
                        Rectangle::new(at + r.loc, r.size),
                        CommitCounter::default(),
                        c,
                        Kind::Unspecified,
                    );
                    crate::render::SceneElement::AnimBand(RescaleRenderElement::from_element(
                        solid,
                        center,
                        Scale::from(scale),
                    ))
                }));
                Some(out)
            })
            .flatten()
            .collect();
        (opening, closing)
    }
}

/// 窓の要素を、描画の原点 `loc`（物理）で作る。
fn window_elements(
    window: &smithay::desktop::Window,
    renderer: &mut GlesRenderer,
    loc: Point<i32, Physical>,
    scale: f64,
) -> Vec<WaylandSurfaceRenderElement<GlesRenderer>> {
    AsRenderElements::<GlesRenderer>::render_elements::<WaylandSurfaceRenderElement<GlesRenderer>>(
        window,
        renderer,
        loc,
        Scale::from(scale),
        1.0,
    )
}

/// 枠の矩形 `outer`（全体の論理座標、太さ `width`）の 4 本の帯を、窓の描画の原点 `origin` からの物理座標にする。
fn band_rects(
    outer: contrast_core::Rect,
    width: i32,
    origin: Point<i32, Logical>,
    scale: f64,
) -> Vec<Rectangle<i32, Physical>> {
    crate::render::edges(outer, width)
        .into_iter()
        .flatten()
        .map(|(x, y, w, h)| {
            let loc = Point::<i32, Logical>::from((x - origin.x, y - origin.y))
                .to_physical_precise_round(scale);
            let size =
                smithay::utils::Size::<i32, Logical>::from((w, h)).to_physical_precise_round(scale);
            Rectangle::new(loc, size)
        })
        .collect()
}

/// 要素を囲む矩形（物理）。要素が無ければ `None`。
fn bbox_of(
    elements: &[WaylandSurfaceRenderElement<GlesRenderer>],
    scale: f64,
) -> Option<Rectangle<i32, Physical>> {
    elements
        .iter()
        .map(|e| e.geometry(Scale::from(scale)))
        .reduce(|a, b| a.merge(b))
}

impl Contrast {
    /// フレームの描画の前に呼ぶ: 表示中の窓の絵を覚え、開く・閉じる窓をテクスチャに描く。
    /// winit では `bind` より前に呼ぶ（オフスクリーンの bind を、窓の面の bind と submit の間に挟まない）。
    pub fn prepare_window_animations(&mut self, renderer: &mut GlesRenderer) {
        // フレームごとの時間の処理（DnD の端スクロール、オーバービューのバネの終わり）もここで。
        self.dnd_tick();
        self.overview.settle();
        let legacy_ms = self.policy.layout_settings().animation_ms;
        let open_timing = timing_for(MotionKind::Open, self.policy.motion(), legacy_ms);
        let now = Instant::now();
        let view = self.core.view();
        let visible: Vec<(ExternalKey, smithay::desktop::Window, Output, WindowId)> = view
            .visible_windows()
            .iter()
            .filter_map(|w| {
                let key = view.window_key(w.id)?;
                let window = self.shell.windows.get(&key)?.clone();
                let output = self.output_of_window(w.id)?;
                Some((key, window, output, w.id))
            })
            .collect();
        let mut last = BTreeMap::new();
        let focus_id = view.focus();
        let unfocused_dim = self.policy.unfocused_dim();
        for (key, window, output, w_id) in visible {
            let Some(location) = self.space.element_location(&window) else {
                continue;
            };
            let scale = output.current_scale().fractional_scale();
            let origin = location - window.geometry().loc;
            let probe = window_elements(&window, renderer, Point::from((0, 0)), scale);
            let Some(bbox) = bbox_of(&probe, scale) else {
                continue;
            };
            drop(probe);
            // テクスチャの左上を原点にした要素。
            let elements =
                window_elements(&window, renderer, Point::from((0, 0)) - bbox.loc, scale);
            let bands: Vec<_> = self
                .window_border(w_id, &output)
                .map(|(outer, width, color)| {
                    band_rects(outer, width, origin, scale)
                        .into_iter()
                        .map(|r| (r, color))
                        .collect()
                })
                .unwrap_or_default();
            let dim = crate::render::dim::window_dim(
                focus_id == Some(w_id),
                view.window(w_id)
                    .is_some_and(|w| w.state == contrast_core::WindowState::Fullscreen),
                unfocused_dim,
            );
            let geo = window.geometry();
            let center = location + Point::from((geo.size.w / 2, geo.size.h / 2));
            let first_time = self.window_anims.seen.insert(key);
            if first_time && let Some(timing) = open_timing {
                self.window_anims.opening.insert(key, (now, timing, None));
            }
            last.insert(
                key,
                Snapshot {
                    elements,
                    bbox,
                    origin,
                    center,
                    output,
                    scale,
                    bands,
                    dim,
                },
            );
        }
        self.window_anims.last = last;

        // 開く窓: 今のフレームの絵をテクスチャに描く（中身は毎フレーム変わる）。
        let anims = &mut self.window_anims;
        anims
            .opening
            .retain(|_, (started, timing, _)| now.duration_since(*started) < timing.total());
        for (key, (_, _, drawn)) in &mut anims.opening {
            *drawn = anims.last.get(key).and_then(|s| draw(renderer, s, false));
        }
        // 閉じる窓: 最初の 1 回だけテクスチャに描き、要素は捨てる。
        anims
            .closing
            .retain(|c| now.duration_since(c.started) < c.timing.total());
        for c in &mut anims.closing {
            if let Some(snapshot) = c.snapshot.take() {
                c.drawn = draw(renderer, &snapshot, true);
            }
        }
    }
}

impl Contrast {
    /// `screenshot-window` が押されていれば、フォーカス窓の最後のフレームの絵（ポップアップ・サブサーフェスを含み、枠は含まない）を、
    /// 透明な背景で PNG に保存する（niri の `screenshot_window`。`show-pointer` の既定 false と同じく、ポインタは入れない）。
    /// `prepare_window_animations` のあと、フレームの描画の前に呼ぶ。
    pub fn take_window_screenshot_if_pending(&mut self, renderer: &mut GlesRenderer) {
        if !std::mem::take(&mut self.screenshot_window_pending) {
            return;
        }
        let view = self.core.view();
        let Some(key) = view.focus().and_then(|id| view.window_key(id)) else {
            tracing::info!("screenshot-window: no focused window");
            return;
        };
        let Some(snapshot) = self.window_anims.last.get(&key) else {
            tracing::info!("screenshot-window: the focused window has nothing to draw");
            return;
        };
        let size = snapshot.bbox.size;
        let dim = crate::render::dim::snapshot_dim(
            crate::render::dim::SnapshotPurpose::ScreenshotWindow,
            snapshot.dim,
        );
        let rgba = match render_snapshot_rgba(renderer, snapshot, dim) {
            Ok(rgba) => rgba,
            Err(e) => {
                tracing::warn!("screenshot-window: {e}");
                return;
            }
        };
        let (Ok(width), Ok(height)) = (u32::try_from(size.w), u32::try_from(size.h)) else {
            return;
        };
        crate::screenshot::deliver(self, width, height, &rgba, true);
    }
}

/// 窓の絵を、乗数 `dim`（1 で減光なし）を掛けて、テクスチャに描く。シェーダが使えなければ減光しない。
fn render_snapshot_texture(
    renderer: &mut GlesRenderer,
    s: &Snapshot,
    dim: f32,
) -> Result<GlesTexture, String> {
    let clear = [0.0, 0.0, 0.0, 0.0];
    if crate::render::dim::is_dimming(dim)
        && let Some(sh) = crate::render::look::Shaders::get(renderer)
    {
        let styled: Vec<_> = s
            .elements
            .iter()
            .map(|e| crate::render::StyledSurface::new(e, sh.rounded.clone(), 0.0, dim))
            .collect();
        return crate::render::offscreen::render_to_texture(
            renderer,
            s.bbox.size,
            s.scale,
            &styled,
            clear,
        );
    }
    crate::render::offscreen::render_to_texture(renderer, s.bbox.size, s.scale, &s.elements, clear)
}

/// 窓の絵を、乗数 `dim` を掛けて、RGBA に描く。`screenshot-window` は `dim = 1`（`snapshot_dim`）で呼ぶ。
fn render_snapshot_rgba(
    renderer: &mut GlesRenderer,
    s: &Snapshot,
    dim: f32,
) -> Result<Vec<u8>, String> {
    let clear = [0.0, 0.0, 0.0, 0.0];
    if crate::render::dim::is_dimming(dim)
        && let Some(sh) = crate::render::look::Shaders::get(renderer)
    {
        let styled: Vec<_> = s
            .elements
            .iter()
            .map(|e| crate::render::StyledSurface::new(e, sh.rounded.clone(), 0.0, dim))
            .collect();
        return crate::render::offscreen::render_to_rgba(
            renderer,
            s.bbox.size,
            s.scale,
            &styled,
            clear,
        );
    }
    crate::render::offscreen::render_to_rgba(renderer, s.bbox.size, s.scale, &s.elements, clear)
}

/// 窓の絵をテクスチャに描く（枠は含めない）。開閉の絵は、画面で見えていた乗数のまま描く。失敗したら警告して `None`。
fn draw(renderer: &mut GlesRenderer, s: &Snapshot, with_bands: bool) -> Option<Drawn> {
    let dim =
        crate::render::dim::snapshot_dim(crate::render::dim::SnapshotPurpose::Animation, s.dim);
    match render_snapshot_texture(renderer, s, dim) {
        Ok(texture) => Some(Drawn {
            texture,
            bbox: s.bbox,
            bands: if with_bands {
                s.bands.clone()
            } else {
                Vec::new()
            },
            origin: s.origin,
            center: s.center,
            output: s.output.clone(),
            scale: s.scale,
        }),
        Err(e) => {
            tracing::warn!("window animation: offscreen render failed: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_start_at_zero_and_end_at_one() {
        assert_eq!(ease_out_expo(0.0), 0.0);
        assert_eq!(ease_out_expo(1.0), 1.0);
        assert!(ease_out_expo(0.5) > 0.96 && ease_out_expo(0.5) < 0.97);
        assert_eq!(ease_out_quad(0.0), 0.0);
        assert_eq!(ease_out_quad(1.0), 1.0);
        assert!((ease_out_quad(0.5) - 0.75).abs() < 1e-12);
    }

    #[test]
    fn opening_fades_in_from_half_size_and_closing_fades_out_to_four_fifths() {
        assert_eq!(open_look(0.0), (0.0, 0.5));
        assert_eq!(open_look(1.0), (1.0, 1.0));
        assert_eq!(close_look(0.0), (1.0, 1.0));
        let (a, s) = close_look(1.0);
        assert_eq!(a, 0.0);
        assert!((s - 0.8).abs() < 1e-12);
    }

    #[test]
    fn a_border_is_four_bands_around_the_window_relative_to_its_origin() {
        // 窓の枠 (10,20) から 100x50、太さ 4。窓の描画の原点は (6,16)、拡大率 1。
        let outer = contrast_core::Rect::new(10, 20, 100, 50).expect("rect");
        let got = band_rects(outer, 4, Point::from((6, 16)), 1.0);
        let at = |x, y, w, h| Rectangle::<i32, Physical>::new(Point::from((x, y)), (w, h).into());
        assert_eq!(
            got,
            [
                at(4, 4, 100, 4),
                at(4, 50, 100, 4),
                at(4, 8, 4, 42),
                at(100, 8, 4, 42)
            ]
        );
        // 太さが 0 なら帯は無い。
        assert!(band_rects(outer, 0, Point::from((0, 0)), 1.0).is_empty());
    }

    use contrast_policy::MotionSpec;

    fn motion_with(kind: MotionKind, curve: MotionCurve, ms: u32) -> Option<MotionSettings> {
        let mut m = MotionSettings::default();
        m.set(
            kind,
            MotionSpec {
                curve,
                duration_ms: ms,
            },
        );
        Some(m)
    }

    #[test]
    fn without_a_motion_element_open_and_close_keep_the_fixed_150ms_enabled_by_animation_ms() {
        for kind in [MotionKind::Open, MotionKind::Close] {
            assert_eq!(timing_for(kind, None, 0), None);
            // `animationMs` の値は長さに使わない（0 かどうかだけ）。
            assert_eq!(
                timing_for(kind, None, 1),
                Some(Timing {
                    curve: OpenCloseCurve::Legacy,
                    duration: Duration::from_millis(150),
                })
            );
        }
        // 従来の曲線: 開く = expo、閉じる = quad。
        let t = Timing {
            curve: OpenCloseCurve::Legacy,
            duration: OPEN_DURATION,
        };
        let half = Duration::from_millis(75);
        assert_eq!(t.value(MotionKind::Open, half), ease_out_expo(0.5));
        assert_eq!(t.value(MotionKind::Close, half), ease_out_quad(0.5));
    }

    #[test]
    fn a_motion_element_decides_open_and_close_and_ignores_animation_ms() {
        let quad = MotionCurve::Easing(EasingCurve::EaseOutQuad);
        // 書かれていない種別は動かさない（`animationMs` が 150 でも）。
        assert_eq!(
            timing_for(
                MotionKind::Open,
                motion_with(MotionKind::Close, quad, 100),
                150
            ),
            None
        );
        assert_eq!(
            timing_for(MotionKind::Open, motion_with(MotionKind::Open, quad, 90), 0),
            Some(Timing {
                curve: OpenCloseCurve::Easing(EasingCurve::EaseOutQuad),
                duration: Duration::from_millis(90),
            })
        );
        // 長さ 0 は動かさない。
        assert_eq!(
            timing_for(
                MotionKind::Open,
                motion_with(MotionKind::Open, quad, 0),
                150
            ),
            None
        );
        // niriSpring は長さを持たず、収束までの時間を使う。
        let spring = timing_for(
            MotionKind::Close,
            motion_with(MotionKind::Close, MotionCurve::NiriSpring, 0),
            0,
        );
        let t = spring.expect("spring");
        assert_eq!(t.curve, OpenCloseCurve::Spring);
        assert!(t.duration > Duration::ZERO);
        assert_eq!(t.value(MotionKind::Close, Duration::ZERO), 0.0);
        assert!(t.value(MotionKind::Close, t.duration) > 0.99);
    }

    #[test]
    fn the_fraction_is_clamped() {
        assert_eq!(fraction(Duration::from_millis(75), OPEN_DURATION), 0.5);
        assert_eq!(fraction(Duration::from_millis(300), OPEN_DURATION), 1.0);
        assert_eq!(fraction(Duration::ZERO, Duration::ZERO), 1.0);
    }
}
