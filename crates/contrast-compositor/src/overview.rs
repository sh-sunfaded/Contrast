//! オーバービュー（niri の `toggle-overview`、4 本指のスワイプ、左上のホットコーナー）。
//!
//! niri（`src/layout/mod.rs` の `toggle_overview`・`overview_gesture_*`、`monitor.rs` の `workspaces_render_geo`）と同じく、
//! 出力ごとに、その出力のワークスペースを縦に並べて縮める。縮める率 `zoom = 1 − 進み × (1 − 0.5)`、ワークスペースの間は
//! 出力の高さ × 0.1 × zoom、アクティブなワークスペースが中央。背景は (0.15, 0.15, 0.15)。開閉はバネ（減衰比 1.0、stiffness 800）。
//! 窓をクリックするとその窓を、ワークスペースの空き所をクリックするとそのワークスペースを選んで閉じる。Escape・Enter で閉じる。
//! 左右の矢印は列のフォーカス、上下は窓のフォーカス（端ならワークスペース）。
//!
//! niri との差: 窓のドラッグでの移動、ワークスペースの影、背景のレイヤー面の配置（niri は各ワークスペースの中に縮めて描く。
//! ここでも同じく縮めて描く）、ワークスペースの切り替えとの連動したアニメーションは無い。

use std::time::Instant;

use contrast_core::{ExternalKey, WindowState, WorkspaceId};
use smithay::{
    backend::renderer::{
        element::{
            AsRenderElements, Element, Id, Kind,
            solid::SolidColorRenderElement,
            surface::WaylandSurfaceRenderElement,
            utils::{CropRenderElement, RescaleRenderElement},
        },
        gles::GlesRenderer,
        utils::CommitCounter,
    },
    desktop::layer_map_for_output,
    output::Output,
    utils::{Logical, Physical, Point, Rectangle, Scale, Size},
    wayland::shell::wlr_layer::Layer,
};

use crate::render::SceneElement;
use crate::spring::{Spring, SpringParams};
use crate::state::Contrast;

/// 4 本指で開き切るのに要る指の移動（niri の `OVERVIEW_GESTURE_MOVEMENT`）。
pub const GESTURE_MOVEMENT: f64 = 300.0;
/// 縮める率（niri の既定の `overview { zoom 0.5; }`）。
pub const ZOOM: f64 = 0.5;
/// 背景の色（niri の `DEFAULT_BACKDROP_COLOR`）。
const BACKDROP: [f32; 4] = [0.15, 0.15, 0.15, 1.0];

/// 進みの動き。
#[derive(Debug, Clone, Copy)]
enum Progress {
    /// 止まっている（0 か 1）。
    Static(f64),
    /// 指で動かしている。
    Gesture(f64),
    /// バネで動いている。
    Animation { spring: Spring, start: Instant },
}

/// オーバービューの状態。
#[derive(Debug, Clone, Copy)]
pub struct Overview {
    /// 開いているか（行き先）。
    pub open: bool,
    progress: Progress,
}

impl Default for Overview {
    fn default() -> Self {
        Self {
            open: false,
            progress: Progress::Static(0.0),
        }
    }
}

impl Overview {
    /// 今の進み（0 で閉じている、1 で開き切っている。バネで行き過ぎることがある）。
    pub fn value(&self) -> f64 {
        match self.progress {
            Progress::Static(v) | Progress::Gesture(v) => v,
            Progress::Animation { spring, start } => spring.value_at(start.elapsed().as_secs_f64()),
        }
    }

    /// 動いているか（次のフレームも描く）。
    pub fn is_animating(&self) -> bool {
        match self.progress {
            Progress::Static(_) => false,
            Progress::Gesture(_) => true,
            Progress::Animation { spring, start } => start.elapsed() < spring.settle_time(),
        }
    }

    /// 見えているか（進みが 0 より大きい）。
    pub fn is_visible(&self) -> bool {
        self.open || self.value() > 0.0001
    }

    /// 終わったアニメーションを止める。
    pub fn settle(&mut self) {
        if let Progress::Animation { spring, start } = self.progress
            && start.elapsed() >= spring.settle_time()
        {
            self.progress = Progress::Static(spring.to);
        }
    }

    /// 開く・閉じるを切り替える（今の進みから、速度 `velocity` でバネ）。
    pub fn toggle(&mut self, velocity: f64) {
        self.open = !self.open;
        self.animate_to(if self.open { 1.0 } else { 0.0 }, velocity);
    }

    /// `to` へバネで動かす。
    fn animate_to(&mut self, to: f64, velocity: f64) {
        let from = self.value();
        self.progress = Progress::Animation {
            spring: Spring {
                from,
                to,
                initial_velocity: velocity,
                params: SpringParams::movement(),
            },
            start: Instant::now(),
        };
    }

    /// 4 本指で始めた。
    pub fn gesture_begin(&mut self) {
        self.open = true;
        self.progress = Progress::Gesture(self.value());
    }

    /// 指の位置での進み（0〜1 に収める。niri はゴムのように少し越える）。
    pub fn gesture_update(&mut self, value: f64) {
        if let Progress::Gesture(v) = &mut self.progress {
            *v = value.clamp(0.0, 1.0);
        }
    }

    /// 指を離した。減速して止まるはずの進み `projected` を丸めて、開くか閉じるかを決める。
    pub fn gesture_end(&mut self, projected: f64) {
        let to = projected.clamp(0.0, 1.0).round();
        self.open = to == 1.0;
        self.animate_to(to, 0.0);
    }
}

/// 進み `p` の縮める率。
pub fn zoom_at(p: f64) -> f64 {
    (1.0 - p * (1.0 - ZOOM)).max(0.0001)
}

/// 出力の大きさ `size`（論理）、縮める率 `zoom`、アクティブな位置 `active`、ワークスペースの数 `count` から、
/// 各ワークスペースの矩形（出力の中の論理座標）。niri の `workspaces_render_geo`。
#[cfg(test)]
pub fn workspace_rects(
    size: (f64, f64),
    zoom: f64,
    active: usize,
    count: usize,
) -> Vec<(f64, f64, f64, f64)> {
    let gap = (size.1 * 0.1 * zoom).round().max(1.0);
    workspace_rects_at(size, zoom, active as f64, count, gap)
}

/// `workspace_rects` の、アクティブな位置が小数（縦のスワイプの途中）で、隙間 `gap`（論理 px）を指定できる版。
pub fn workspace_rects_at(
    size: (f64, f64),
    zoom: f64,
    active: f64,
    count: usize,
    gap: f64,
) -> Vec<(f64, f64, f64, f64)> {
    let (w, h) = (size.0 * zoom, size.1 * zoom);
    let off_x = ((size.0 - w) / 2.0).round();
    let off_y = ((size.1 - h) / 2.0).round();
    (0..count)
        .map(|k| {
            let y = (k as f64 - active) * (h + gap) + off_y;
            (off_x, y.round(), w, h)
        })
        .collect()
}

impl Contrast {
    /// 4 本指のジェスチャーの始めの進み。
    pub fn overview_progress_target(&self) -> f64 {
        self.overview.value()
    }

    /// 4 本指のジェスチャーを始めた。
    pub fn overview_gesture_begin(&mut self) {
        self.overview.gesture_begin();
    }

    /// 4 本指の移動（進み）。
    pub fn overview_gesture_update(&mut self, value: f64) {
        self.overview.gesture_update(value);
    }

    /// 4 本指を離した。
    pub fn overview_gesture_end(&mut self, projected: f64) {
        self.overview.gesture_end(projected);
        tracing::info!(open = self.overview.open, "overview: gesture ended");
    }

    /// `toggle-overview` とホットコーナー。
    pub fn toggle_overview(&mut self) {
        self.overview.toggle(0.0);
        tracing::info!(open = self.overview.open, "overview: toggled");
    }

    /// 出力 `output` のワークスペースの、オーバービューでの矩形（全体の論理座標）と id。
    fn overview_workspaces(&self, output: &Output) -> Vec<(WorkspaceId, Rectangle<f64, Logical>)> {
        let Some(geo) = self.space.output_geometry(output) else {
            return Vec::new();
        };
        let Some(id) = self.core_output(output) else {
            return Vec::new();
        };
        let view = self.core.view();
        let list = view.output_workspaces(id);
        let active = self.shown_position(output).map_or(0, usize::from);
        let zoom = zoom_at(self.overview.value());
        let size = (f64::from(geo.size.w), f64::from(geo.size.h));
        // 縦のスワイプの途中（とその後のバネ）は、オーバービューと同じ並べ方を、ズーム 1・隙間 0 で、小数の位置で描く。
        let offset = self.workspace_follow_offset();
        let rects = if self.overview.is_visible() {
            // オーバービューの中の DnD の縦の端スクロールでも、位置は小数になる。
            let gap = (size.1 * 0.1 * zoom).round().max(1.0);
            workspace_rects_at(
                size,
                zoom,
                active as f64 + offset.unwrap_or(0.0),
                list.len(),
                gap,
            )
        } else {
            workspace_rects_at(
                size,
                1.0,
                active as f64 + offset.unwrap_or(0.0),
                list.len(),
                0.0,
            )
        };
        list.into_iter()
            .zip(rects)
            .map(|(ws, (x, y, w, h))| {
                (
                    ws,
                    Rectangle::new(
                        Point::from((f64::from(geo.loc.x) + x, f64::from(geo.loc.y) + y)),
                        Size::from((w, h)),
                    ),
                )
            })
            .collect()
    }

    /// 窓の、ワークスペースの中での矩形（出力の中の論理座標。core の矩形と隙間）。
    fn window_frame_in_output(
        &self,
        output: &Output,
        key: ExternalKey,
    ) -> Option<contrast_core::Rect> {
        let view = self.core.view();
        let w = view.window(view.window_id(key)?)?;
        let tile = w.rect?;
        let frame = match w.state {
            WindowState::Fullscreen => tile,
            state => self.frame_of(tile, state, Some(output)),
        };
        Some(self.anim.displayed(w.id).unwrap_or(frame))
    }

    /// オーバービューの要素（前から: 上のレイヤー面、各ワークスペースの窓と背景、背景色）。開いていなければ `None`。
    pub fn overview_elements(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
        origin: Point<i32, Logical>,
    ) -> Option<Vec<SceneElement>> {
        if !self.overview.is_visible() && self.workspace_follow_offset().is_none() {
            return None;
        }
        let geo = self.space.output_geometry(output)?;
        let scale = output.current_scale().fractional_scale();
        let zoom = zoom_at(self.overview.value());
        let out_origin = geo.loc - origin;
        let mut out: Vec<SceneElement> = Vec::new();
        let layer_map = layer_map_for_output(output);
        // 上のレイヤー面（バー）は縮めない。
        for layer in layer_map
            .layers_on(Layer::Overlay)
            .chain(layer_map.layers_on(Layer::Top))
        {
            let Some(loc) = layer_map.layer_geometry(layer).map(|g| g.loc) else {
                continue;
            };
            out.extend(
                AsRenderElements::<GlesRenderer>::render_elements::<
                    WaylandSurfaceRenderElement<GlesRenderer>,
                >(
                    layer,
                    renderer,
                    (loc + out_origin).to_physical_precise_round(scale),
                    Scale::from(scale),
                    1.0,
                )
                .into_iter()
                .map(SceneElement::Surface),
            );
        }
        let lower: Vec<(Point<i32, Logical>, smithay::desktop::LayerSurface)> = layer_map
            .layers_on(Layer::Bottom)
            .chain(layer_map.layers_on(Layer::Background))
            .filter_map(|l| Some((layer_map.layer_geometry(l)?.loc, l.clone())))
            .collect();
        drop(layer_map);
        let view = self.core.view();
        let background = self.policy.background();
        let dragging = self.gestures.overview_drag;
        // ドラッグ中の窓は、ポインタについていく（縮めた見た目のまま。切り抜かず、一番前）。
        if let Some(d) = dragging.filter(|d| d.moving)
            && let Some(window) = self.shell.windows.get(&d.key).cloned()
        {
            let top_left = d.pos - d.grab;
            let target: Point<i32, Physical> = (top_left - geo.loc.to_f64() + out_origin.to_f64())
                .to_physical_precise_round(scale);
            let loc: Point<i32, Physical> =
                target - window.geometry().loc.to_physical_precise_round(scale);
            for e in AsRenderElements::<GlesRenderer>::render_elements::<
                WaylandSurfaceRenderElement<GlesRenderer>,
            >(&window, renderer, loc, Scale::from(scale), 1.0)
            {
                out.push(SceneElement::DragSurface(
                    RescaleRenderElement::from_element(e, target, Scale::from(zoom)),
                ));
            }
        }
        for (ws, rect) in self.overview_workspaces(output) {
            // 出力の中の、このワークスペースの左上（物理）と、切り抜きの矩形。
            let ws_loc: Point<f64, Logical> = rect.loc - geo.loc.to_f64() + out_origin.to_f64();
            let crop: Rectangle<i32, Physical> = Rectangle::new(
                ws_loc.to_physical_precise_round(scale),
                rect.size.to_physical_precise_round(scale),
            );
            // 要素は縮める前の大きさ・位置で作り（`ws_loc` が原点）、`shrink_and_crop` で `zoom` 倍に縮めて切り抜く。
            let place = |p: Point<i32, Logical>| -> Point<i32, Physical> {
                (ws_loc + p.to_f64()).to_physical_precise_round(scale)
            };
            let keys: Vec<ExternalKey> = view
                .workspace_windows_by_id(ws)
                .unwrap_or(&[])
                .iter()
                .filter_map(|id| view.window_key(*id))
                .collect();
            // 窓（上に描かれるものから。全画面と浮動が上）。
            let mut ordered: Vec<(u8, ExternalKey)> = keys
                .iter()
                .map(|k| {
                    let state = view
                        .window_id(*k)
                        .and_then(|id| view.window(id))
                        .map(|w| w.state);
                    let rank = match state {
                        Some(WindowState::Fullscreen) => 0,
                        Some(WindowState::Floating) => 1,
                        _ => 2,
                    };
                    (rank, *k)
                })
                .collect();
            ordered.sort();
            for (_, key) in ordered {
                if dragging.is_some_and(|d| d.key == key && d.moving) {
                    continue;
                }
                let (Some(window), Some(frame)) = (
                    self.shell.windows.get(&key).cloned(),
                    self.window_frame_in_output(output, key),
                ) else {
                    continue;
                };
                let local = Point::from((frame.x() - geo.loc.x, frame.y() - geo.loc.y))
                    - window.geometry().loc;
                let elements =
                    AsRenderElements::<GlesRenderer>::render_elements::<
                        WaylandSurfaceRenderElement<GlesRenderer>,
                    >(&window, renderer, place(local), Scale::from(scale), 1.0);
                out.extend(
                    elements
                        .into_iter()
                        .filter_map(|e| shrink_and_crop(e, crop.loc, zoom, scale, crop))
                        .map(SceneElement::CroppedSurface),
                );
            }
            // 背景のレイヤー面（壁紙など）を、ワークスペースの中に縮めて描く。
            for (loc, layer) in &lower {
                let elements =
                    AsRenderElements::<GlesRenderer>::render_elements::<
                        WaylandSurfaceRenderElement<GlesRenderer>,
                    >(layer, renderer, place(*loc), Scale::from(scale), 1.0);
                out.extend(
                    elements
                        .into_iter()
                        .filter_map(|e| shrink_and_crop(e, crop.loc, zoom, scale, crop))
                        .map(SceneElement::CroppedSurface),
                );
            }
            // ワークスペースの背景色。
            out.push(SceneElement::Solid(SolidColorRenderElement::new(
                Id::new(),
                crop,
                CommitCounter::default(),
                background,
                Kind::Unspecified,
            )));
        }
        // ワークスペースの影（niri の `workspace-shadow` の既定: やわらかさ 40・広がり 10・下へ 10、色 #00000050。
        // 出力の高さ / 1080 と、縮める率を掛け、開く進みを透明度にする）。全ワークスペースの後ろ、背景（暗い面）の前。
        if let Some(sh) = crate::render::look::Shaders::get(renderer) {
            let norm = f64::from(geo.size.h) / 1080.0 * zoom;
            let (blur, spread, off_y) = (40.0 * norm, 10.0 * norm, 10.0 * norm);
            let alpha = self.overview.value().clamp(0.0, 1.0) as f32;
            for (_, rect) in self.overview_workspaces(output) {
                let ws_loc: Point<f64, Logical> = rect.loc - geo.loc.to_f64() + out_origin.to_f64();
                let pad = blur + spread;
                let area = crate::render::logical(
                    (ws_loc.x - pad).round() as i32,
                    (ws_loc.y - pad + off_y).round() as i32,
                    (rect.size.w + 2.0 * pad).round() as i32,
                    (rect.size.h + 2.0 * pad).round() as i32,
                );
                out.push(SceneElement::Pixel(
                    smithay::backend::renderer::gles::element::PixelShaderElement::new(
                        sh.shadow.clone(),
                        area,
                        None,
                        alpha,
                        vec![
                            smithay::backend::renderer::gles::Uniform::new(
                                "color",
                                crate::render::premultiplied([0.0, 0.0, 0.0, 80.0 / 255.0]),
                            ),
                            smithay::backend::renderer::gles::Uniform::new("radius", 0.0_f32),
                            smithay::backend::renderer::gles::Uniform::new("blur", blur as f32),
                        ],
                        Kind::Unspecified,
                    ),
                ));
            }
        }
        out.push(SceneElement::Solid(SolidColorRenderElement::new(
            Id::new(),
            Rectangle::new(
                out_origin.to_physical_precise_round(scale),
                geo.size.to_physical_precise_round(scale),
            ),
            CommitCounter::default(),
            BACKDROP,
            Kind::Unspecified,
        )));
        Some(out)
    }

    /// 押した位置の窓と、窓（縮めた見た目）の左上からの位置。
    fn overview_window_at(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(ExternalKey, Point<f64, Logical>)> {
        let output = self.output_at(pos.x, pos.y)?;
        let geo = self.space.output_geometry(&output)?;
        let zoom = zoom_at(self.overview.value());
        let (ws, rect) = self
            .overview_workspaces(&output)
            .into_iter()
            .find(|(_, r)| r.contains(pos))?;
        let inner = (pos - rect.loc).downscale(zoom) + geo.loc.to_f64();
        let view = self.core.view();
        let key = view
            .workspace_windows_by_id(ws)
            .unwrap_or(&[])
            .iter()
            .filter_map(|id| view.window_key(*id))
            .find(|k| {
                self.window_frame_in_output(&output, *k).is_some_and(|f| {
                    f.contains(contrast_core::Point::new(
                        inner.x.floor() as i32,
                        inner.y.floor() as i32,
                    ))
                })
            })?;
        let f = self.window_frame_in_output(&output, key)?;
        let shown_origin = rect.loc
            + Point::<f64, Logical>::from((
                f64::from(f.x()) - f64::from(geo.loc.x),
                f64::from(f.y()) - f64::from(geo.loc.y),
            ))
            .upscale(zoom);
        Some((key, pos - shown_origin))
    }

    /// オーバービューで左ボタンを押した: 窓の上なら、ドラッグかクリックかを決めるまで覚える。窓でなければ、すぐクリックとして扱う。
    pub fn overview_press(&mut self, pos: Point<f64, Logical>) {
        match self.overview_window_at(pos) {
            Some((key, grab)) => {
                self.gestures.overview_drag = Some(crate::gestures::OverviewDrag {
                    key,
                    press: pos,
                    grab,
                    pos,
                    moving: false,
                });
            }
            None => self.overview_click(pos),
        }
    }

    /// オーバービューでのポインタの移動（ボタンを押している間）。
    pub fn overview_drag_motion(&mut self, pos: Point<f64, Logical>) {
        if let Some(d) = self.gestures.overview_drag.as_mut() {
            d.pos = pos;
            if !d.moving && crate::gestures::past_drag_threshold(d.press, pos) {
                d.moving = true;
                tracing::info!(key = ?d.key, "overview: window drag started");
            }
        }
    }

    /// オーバービューで左ボタンを離した: ドラッグでなければクリック。ドラッグなら、下のワークスペースへ窓を移す。
    pub fn overview_release(&mut self, pos: Point<f64, Logical>) {
        let Some(drag) = self.gestures.overview_drag.take() else {
            return;
        };
        if !drag.moving {
            self.overview_click(drag.press);
            return;
        }
        let Some(output) = self.output_at(pos.x, pos.y) else {
            return;
        };
        let target = self
            .overview_workspaces(&output)
            .into_iter()
            .find(|(_, r)| r.contains(pos))
            .map(|(ws, _)| ws);
        let view = self.core.view();
        let position = target.and_then(|ws| {
            self.core_output(&output)
                .and_then(|id| view.output_workspaces(id).iter().position(|w| *w == ws))
        });
        tracing::info!(key = ?drag.key, ?position, "overview: window dropped");
        let Some(p) = position.and_then(|p| u8::try_from(p).ok()) else {
            return;
        };
        if let Some(key) = self.key_of_output(&output) {
            self.submit_command(contrast_core::Command::OutputFocusRequested(key));
        }
        self.submit_command(contrast_core::Command::WindowFocusRequested(drag.key));
        self.core_action(contrast_core::Action::MoveToWorkspace(
            contrast_core::WorkspaceIndex::new(p),
        ));
    }

    /// オーバービューでのクリック: 窓ならその窓を、ワークスペースならそのワークスペースを選んで閉じる。どちらでもなければ何もしない。
    pub fn overview_click(&mut self, pos: Point<f64, Logical>) {
        let Some(output) = self.output_at(pos.x, pos.y) else {
            return;
        };
        let Some(geo) = self.space.output_geometry(&output) else {
            return;
        };
        let zoom = zoom_at(self.overview.value());
        let Some((ws, rect)) = self
            .overview_workspaces(&output)
            .into_iter()
            .find(|(_, r)| r.contains(pos))
        else {
            return;
        };
        // ワークスペースの中の点を、縮める前の出力の中の座標にする。
        let inner = (pos - rect.loc).downscale(zoom) + geo.loc.to_f64();
        let view = self.core.view();
        let hit = view
            .workspace_windows_by_id(ws)
            .unwrap_or(&[])
            .iter()
            .filter_map(|id| view.window_key(*id))
            .find(|k| {
                self.window_frame_in_output(&output, *k).is_some_and(|f| {
                    f.contains(contrast_core::Point::new(
                        inner.x.floor() as i32,
                        inner.y.floor() as i32,
                    ))
                })
            });
        let position = self
            .core_output(&output)
            .and_then(|id| view.output_workspaces(id).iter().position(|w| *w == ws));
        tracing::info!(?hit, ?position, "overview: click");
        if let Some(key) = self.key_of_output(&output) {
            self.submit_command(contrast_core::Command::OutputFocusRequested(key));
        }
        if let Some(p) = position.and_then(|p| u8::try_from(p).ok()) {
            self.core_action(contrast_core::Action::SwitchWorkspace(
                contrast_core::WorkspaceIndex::new(p),
            ));
        }
        if let Some(key) = hit {
            self.submit_command(contrast_core::Command::WindowFocusRequested(key));
        }
        self.toggle_overview();
    }

    /// オーバービューが開いているときのキー（niri の `hardcoded_overview_bind`）。扱ったら true。
    pub fn overview_key(&mut self, name: &str, any_mods: bool) -> bool {
        if !self.overview.open || any_mods {
            return false;
        }
        use contrast_core::{Action, Direction};
        match name {
            "Escape" | "Return" => self.toggle_overview(),
            "Left" => self.core_action(Action::FocusDirection(Direction::Left)),
            "Right" => self.core_action(Action::FocusDirection(Direction::Right)),
            "Up" | "Down" => {
                let up = name == "Up";
                let before = self.core.view().focus();
                self.core_action(Action::FocusDirection(if up {
                    Direction::Up
                } else {
                    Direction::Down
                }));
                // 窓が動かなければ（端・窓が無い）、ワークスペースを上下に（niri の focus-window-or-workspace-up/down）。
                if self.core.view().focus() == before {
                    self.run_action(if up {
                        contrast_policy::Action::WorkspacePrev
                    } else {
                        contrast_policy::Action::WorkspaceNext
                    });
                }
            }
            _ => return false,
        }
        true
    }

    /// ポインタの移動: 左上の隅（1×1 論理 px）に入ったら、オーバービューを切り替える（niri のホットコーナー。入ったときに 1 回）。
    pub fn hot_corner_motion(&mut self, pos: Point<f64, Logical>) {
        let inside = self
            .output_at(pos.x, pos.y)
            .and_then(|o| self.space.output_geometry(&o))
            .is_some_and(|g| {
                let local = pos - g.loc.to_f64();
                (0.0..1.0).contains(&local.x) && (0.0..1.0).contains(&local.y)
            });
        if inside && !self.pointer_in_hot_corner {
            let grabbed = self.seat.get_pointer().is_some_and(|p| p.is_grabbed());
            if !grabbed {
                tracing::info!("overview: hot corner");
                self.toggle_overview();
            }
        }
        self.pointer_in_hot_corner = inside;
    }
}

/// 要素を `origin`（物理座標）を中心に `zoom` 倍に縮め、`crop` で切り抜く。`scale` は出力の倍率。
/// 切り抜いて何も残らなければ `None`。
///
/// `WaylandSurfaceRenderElement` の大きさは、作るときの倍率でなく、描くときの出力の倍率で決まる
/// （作るときの倍率が効くのはサブサーフェスの位置だけ）。なので、縮めるには `RescaleRenderElement` で包む。
fn shrink_and_crop<E: Element>(
    element: E,
    origin: Point<i32, Physical>,
    zoom: f64,
    scale: f64,
    crop: Rectangle<i32, Physical>,
) -> Option<CropRenderElement<RescaleRenderElement<E>>> {
    CropRenderElement::from_element(
        RescaleRenderElement::from_element(element, origin, Scale::from(zoom)),
        Scale::from(scale),
        crop,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 要素（`geometry` が `rect`）を、`origin` を中心に `zoom` 倍に縮めて `crop` で切り抜いた矩形。
    fn shrunk_geometry(
        rect: Rectangle<i32, Physical>,
        origin: Point<i32, Physical>,
        zoom: f64,
        crop: Rectangle<i32, Physical>,
    ) -> Rectangle<i32, Physical> {
        let solid = SolidColorRenderElement::new(
            Id::new(),
            rect,
            CommitCounter::default(),
            [1.0; 4],
            Kind::Unspecified,
        );
        shrink_and_crop(solid, origin, zoom, 1.0, crop)
            .map(|e| e.geometry(Scale::from(1.0)))
            .unwrap_or_default()
    }

    #[test]
    fn a_window_is_shrunk_in_size_not_only_moved() {
        // ワークスペースのタイル（480,300 から 960x600）の左上を中心に半分にする。
        // 440x1044 の窓が (16,16) にあるとき、縮めた窓は (8,8) から 220x522（タイルの中では切れない）。
        let origin = Point::from((480, 300));
        let crop = Rectangle::new(origin, Size::from((960, 600)));
        let window = Rectangle::new(Point::from((480 + 16, 300 + 16)), Size::from((440, 1044)));
        let got = shrunk_geometry(window, origin, 0.5, crop);
        assert_eq!(got.loc, Point::from((480 + 8, 300 + 8)));
        assert_eq!(got.size, Size::from((220, 522)));
    }

    #[test]
    fn a_shrunk_window_is_still_cropped_to_the_tile() {
        let origin = Point::from((480, 300));
        let crop = Rectangle::new(origin, Size::from((960, 600)));
        // 縮めたあとも右へはみ出す窓は、タイルの右端で切れる。
        let window = Rectangle::new(Point::from((480 + 1800, 300)), Size::from((400, 100)));
        let got = shrunk_geometry(window, origin, 0.5, crop);
        assert_eq!(got.loc, Point::from((480 + 900, 300)));
        assert_eq!(got.size, Size::from((60, 50)));
    }

    #[test]
    fn zoom_goes_from_one_to_a_half() {
        assert_eq!(zoom_at(0.0), 1.0);
        assert_eq!(zoom_at(1.0), 0.5);
        assert_eq!(zoom_at(0.5), 0.75);
    }

    #[test]
    fn workspaces_stack_vertically_around_the_active_one() {
        let r = workspace_rects((1000.0, 800.0), 0.5, 1, 3);
        // 幅 500・高さ 400、間 40、アクティブ（1）が中央（x 250、y 200）。
        assert_eq!(
            r,
            [
                (250.0, -240.0, 500.0, 400.0),
                (250.0, 200.0, 500.0, 400.0),
                (250.0, 640.0, 500.0, 400.0)
            ]
        );
        let full = workspace_rects((1000.0, 800.0), 1.0, 0, 1);
        assert_eq!(full, [(0.0, 0.0, 1000.0, 800.0)]);
    }

    #[test]
    fn a_gesture_opens_past_half_and_closes_below() {
        let mut o = Overview::default();
        o.gesture_begin();
        o.gesture_update(0.4);
        assert_eq!(o.value(), 0.4);
        o.gesture_end(0.6);
        assert!(o.open);
        let mut c = Overview::default();
        c.gesture_begin();
        c.gesture_end(0.3);
        assert!(!c.open);
        let mut t = Overview::default();
        t.toggle(0.0);
        assert!(t.open && t.is_animating());
    }
}
