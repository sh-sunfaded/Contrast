//! スクリーンショットの範囲を選ぶ UI（niri の `screenshot`）を、描画・入力・保存につなぐ。状態の規則は `screenshot_ui`。
//!
//! 開くと、すべての出力の今の絵をテクスチャに描いて止め（オフスクリーン。フレームの描画の前に行う）、
//! 出力ごとに、止めた絵・選択の外を暗くする帯・選択の白い枠を描く（niri の `render_output`。開く 200ms で帯と枠が濃くなる）。
//! キー: Escape で閉じる、Space・Enter で保存とクリップボード、Ctrl+C でクリップボードだけ、P でポインタの切り替え。
//! 開いている間のキーバインドは、niri の `allowed_during_screenshot` と同じものだけ（列・窓の移動は選択の移動、幅・高さは選択の大きさ）。
//!
//! 下の中央に説明のパネル（niri の `render_panel`。撮影のボタンと 2 行の説明）を出す。ボタンを押して離すと保存する。
//!
//! niri との差: ポインタの絵（P は状態と説明の文だけ切り替える）、タッチとタブレットは無い。

use std::collections::BTreeMap;
use std::time::Instant;

use smithay::{
    backend::renderer::{
        Renderer,
        element::{Id, Kind, solid::SolidColorRenderElement, texture::TextureRenderElement},
        gles::{GlesRenderer, GlesTexture},
        utils::CommitCounter,
    },
    output::Output,
    utils::{Logical, Physical, Point, Rectangle, Size, Transform},
};

use contrast_policy::Action;

use crate::render::SceneElement;
use crate::screenshot_ui::{OutputInfo, Px, SizeChange, Ui, UiKey, key_action};
use crate::state::Contrast;

/// 開いている UI と、止めた絵。
pub struct Open {
    /// 選択などの状態。
    pub ui: Ui,
    /// 出力ごとの止めた絵（物理 px の大きさ）。
    frozen: BTreeMap<String, (GlesTexture, Size<i32, Physical>)>,
    /// 開いた時刻（開くアニメーション）。
    opened: Instant,
    /// 確定の要求（`Some(ディスクに保存するか)`）。次のフレームの前に処理する。
    confirm: Option<bool>,
    /// 開いた瞬間のポインタの絵（透明な背景に、出力の大きさで描いたもの）。ポインタのある出力だけ。`P` で出す・隠す（niri と同じ）。
    pointers: BTreeMap<String, GlesTexture>,
    /// 説明のパネル（ポインタを出す・隠すの 2 通り）。出力ごと: `(出す, 隠す, 大きさ（物理 px）)`。
    panels: BTreeMap<String, (GlesTexture, GlesTexture, (i32, i32))>,
    /// 撮影のボタンを押した出力（離したときにボタンの上なら保存する）。
    capture_down: Option<String>,
}

/// パネルの定数（論理 px。niri と同じ）。
const PANEL_PADDING: f32 = 8.0;
const PANEL_RADIUS: f32 = 16.0;
const PANEL_BORDER: f32 = 4.0;

/// 説明のパネルを描く（niri の `render_panel`）。`show_pointer` はポインタを出している状態（文は「隠す」になる）。
fn draw_panel(scale: f64, show_pointer: bool) -> Option<crate::text::Canvas> {
    use crate::text::{Canvas, fonts, line_height, text_width};
    let f = fonts()?;
    let p = |v: f32| (f64::from(v) * scale).round() as i32;
    let px = (14.0 * scale).round() as f32;
    let padding = p(PANEL_PADDING);
    let radius = p(PANEL_RADIUS);
    let border = ((f64::from(PANEL_BORDER) / 2.0 * scale).round() * 2.0) as i32;
    let half_border = border / 2;
    let spacing = p(2.0);
    let pointer_word = if show_pointer { "hide" } else { "show" };
    let lines: [[(&str, bool); 3]; 2] = [
        [
            ("Press ", false),
            (" Space ", true),
            (" to save the screenshot.", false),
        ],
        [("Press ", false), (" P ", true), ("", false)],
    ];
    let second_tail = format!(" to {pointer_word} the pointer.");
    let line_h = line_height(&f.regular, px).max(line_height(&f.mono, px));
    let width_of = |segs: &[(&str, bool)], tail: &str| -> i32 {
        segs.iter()
            .map(|(t, mono)| text_width(if *mono { &f.mono } else { &f.regular }, px, t))
            .sum::<i32>()
            + text_width(&f.regular, px, tail)
    };
    let text_w = width_of(&lines[0], "").max(width_of(&lines[1], &second_tail));
    let text_h = line_h * 2 + spacing;
    let width = text_w + padding + radius * 2 + padding - half_border + padding;
    let height = text_h.max(radius * 2) + padding * 2;
    let dark = [0.1, 0.1, 0.1, 1.0];
    let white = [1.0, 1.0, 1.0, 1.0];
    let mut c = Canvas::new(width, height, dark);
    // 撮影のボタン: 白い円、暗い輪、白い円（niri と同じ太さ 2 px の輪）。
    let (r, yc) = (radius as f32, (height / 2) as f32);
    let cx = padding as f32 + r;
    let stroke = p(2.0) as f32;
    c.circle(cx, yc, r, white);
    c.circle(cx, yc, r - stroke, dark);
    c.circle(cx, yc, r - stroke * 2.0, white);
    let x0 = padding + radius * 2 + padding - half_border;
    for (i, segs) in lines.iter().enumerate() {
        let y = padding + i32::try_from(i).unwrap_or(0) * (line_h + spacing);
        let mut x = x0;
        for (t, mono) in segs {
            let font = if *mono { &f.mono } else { &f.regular };
            if *mono {
                // niri の `bgcolor='#2C2C2C'`。
                let bg = 0x2C as f32 / 255.0;
                c.fill(x, y, text_width(font, px, t), line_h, [bg, bg, bg, 1.0]);
            }
            x += c.text(font, px, x, y, t, white);
        }
        if i == 1 {
            c.text(&f.regular, px, x, y, &second_tail, white);
        }
    }
    c.border(half_border.max(1), [0.3, 0.3, 0.3, 1.0]);
    Some(c)
}

/// パネルの位置（出力の中の物理 px。下の中央、下から余白 2 つ分）。
fn panel_location(output: (i32, i32), panel: (i32, i32), scale: f64) -> (i32, i32) {
    let padding = (f64::from(PANEL_PADDING) * scale).round() as i32;
    (
        ((output.0 - panel.0) / 2).max(0),
        (output.1 - panel.1 - padding * 2).max(0),
    )
}

/// パネルの中の点 `pos` が撮影のボタンの上か（niri の `is_within_capture_button`）。
fn within_capture_button(scale: f64, panel: (i32, i32), pos: (i32, i32)) -> bool {
    let padding = (f64::from(PANEL_PADDING) * scale).round() as i32;
    let radius = (f64::from(PANEL_RADIUS) * scale).round() as i32 - 2;
    let (xc, yc) = (padding + radius, panel.1 / 2);
    let (dx, dy) = (pos.0 - xc, pos.1 - yc);
    dx * dx + dy * dy <= radius * radius
}

/// 出力の中の論理座標のポインタの位置から、そのときのカーソルの描画要素を作るもの（バックエンドが持つテーマを使う）。
pub type CursorSource<'a> =
    dyn FnMut(&mut GlesRenderer, Point<f64, Logical>) -> Vec<SceneElement> + 'a;

/// UI の状態（閉じているときは、前回の選択だけを覚える）。
#[derive(Default)]
pub struct ScreenshotUiState {
    /// 開いている UI。
    pub open: Option<Open>,
    /// `screenshot` が押された（次のフレームの前に開く）。
    pub requested: bool,
    /// 前回の選択（出力の名前と矩形）。
    last: Option<(String, Px)>,
    /// UI の選択に使っている指（タッチのスロット）。
    pub touch_slot: Option<smithay::backend::input::TouchSlot>,
}

impl ScreenshotUiState {
    /// 開いているか。
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

/// 開くアニメーションの進み（niri の既定: 200ms・ease-out-quad）。
fn open_progress(opened: Instant) -> f32 {
    let t = (opened.elapsed().as_secs_f64() / crate::screenshot_ui::OPEN_DURATION.as_secs_f64())
        .clamp(0.0, 1.0);
    crate::window_anim::ease_out_quad(t) as f32
}

impl Contrast {
    /// フレームの描画の前に呼ぶ: 開く要求があれば絵を止めて開き、確定の要求があれば選択を読み出して渡す。
    /// `cursor` は、出力の中の論理座標のポインタの位置から、そのときのカーソルの描画要素を作る（バックエンドが持つテーマを使う）。
    pub fn process_screenshot_ui(
        &mut self,
        renderer: &mut GlesRenderer,
        cursor: &mut CursorSource,
    ) {
        if std::mem::take(&mut self.screenshot_ui.requested) && !self.screenshot_ui.is_open() {
            self.open_screenshot_ui(renderer, cursor);
        }
        let confirm = self
            .screenshot_ui
            .open
            .as_mut()
            .and_then(|o| o.confirm.take());
        if let Some(write_to_disk) = confirm {
            self.confirm_screenshot_ui(renderer, write_to_disk);
        }
    }

    /// 出力ごとの今の絵を止めて、UI を開く。既定の出力は、ポインタのある出力、なければアクティブな出力。
    fn open_screenshot_ui(&mut self, renderer: &mut GlesRenderer, cursor: &mut CursorSource) {
        let outputs: Vec<Output> = self.shell.outputs.values().cloned().collect();
        let mut frozen = BTreeMap::new();
        let mut pointers = BTreeMap::new();
        let pointer_now = self.seat.get_pointer().map(|p| p.current_location());
        let mut infos = BTreeMap::new();
        for output in &outputs {
            let (Some(geo), Some(mode)) =
                (self.space.output_geometry(output), output.current_mode())
            else {
                continue;
            };
            let scale = output.current_scale().fractional_scale();
            let elements =
                match crate::render::scene_elements(self, renderer, output, geo.loc, Vec::new()) {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::warn!("screenshot ui: {e}");
                        return;
                    }
                };
            // 出力の背景色で塗る（背景色はフレームの描画の塗りつぶしの色で、要素には無い）。
            let background = self.policy.background();
            // ポインタの絵（このフレームで見えているカーソル。ポインタのある出力だけ）。
            if let Some(pos) = pointer_now.filter(|p| geo.to_f64().contains(*p)) {
                let items = cursor(renderer, pos - geo.loc.to_f64());
                if !items.is_empty()
                    && let Ok(texture) = crate::render::offscreen::render_to_texture(
                        renderer,
                        mode.size,
                        scale,
                        &items,
                        [0.0, 0.0, 0.0, 0.0],
                    )
                {
                    pointers.insert(output.name(), texture);
                }
            }
            match crate::render::offscreen::render_to_texture(
                renderer, mode.size, scale, &elements, background,
            ) {
                Ok(texture) => {
                    frozen.insert(output.name(), (texture, mode.size));
                    infos.insert(
                        output.name(),
                        OutputInfo {
                            size: (mode.size.w, mode.size.h),
                            scale,
                        },
                    );
                }
                Err(e) => {
                    tracing::warn!("screenshot ui: cannot freeze the output: {e}");
                    return;
                }
            }
        }
        let pointer = self.seat.get_pointer().map(|p| p.current_location());
        let default_output = pointer
            .and_then(|p| self.output_at(p.x, p.y))
            .or_else(|| self.active_output());
        let Some(default_output) = default_output else {
            return;
        };
        // niri の `screenshot` の show-pointer の既定は true。
        let last = self.screenshot_ui.last.take();
        let Some(ui) = Ui::open(infos, &default_output.name(), last, true) else {
            return;
        };
        tracing::info!(output = ui.output, rect = ?ui.rect(), "screenshot ui: opened");
        let mut panels = BTreeMap::new();
        for (name, info) in &ui.outputs {
            let upload =
                |renderer: &mut GlesRenderer, show: bool| -> Option<(GlesTexture, (i32, i32))> {
                    let c = draw_panel(info.scale, show)?;
                    let t = smithay::backend::renderer::ImportMem::import_memory(
                        renderer,
                        &c.rgba,
                        smithay::backend::allocator::Fourcc::Abgr8888,
                        Size::from((c.width, c.height)),
                        false,
                    )
                    .ok()?;
                    Some((t, (c.width, c.height)))
                };
            if let (Some((show, size)), Some((hide, _))) =
                (upload(renderer, false), upload(renderer, true))
            {
                panels.insert(name.clone(), (show, hide, size));
            }
        }
        self.screenshot_ui.open = Some(Open {
            ui,
            frozen,
            opened: Instant::now(),
            confirm: None,
            pointers,
            panels,
            capture_down: None,
        });
        self.cursor_status = smithay::input::pointer::CursorImageStatus::Named(
            smithay::input::pointer::CursorIcon::Crosshair,
        );
    }

    /// UI を閉じる（選択を覚える）。
    fn close_screenshot_ui(&mut self) {
        if let Some(open) = self.screenshot_ui.open.take() {
            self.screenshot_ui.last = Some((open.ui.output.clone(), open.ui.rect()));
            tracing::info!("screenshot ui: closed");
        }
        self.cursor_status = smithay::input::pointer::CursorImageStatus::default_named();
    }

    /// 選択の範囲を、止めた絵から読み出して渡し（クリップボードと、`write_to_disk` なら保存）、閉じる。
    fn confirm_screenshot_ui(&mut self, renderer: &mut GlesRenderer, write_to_disk: bool) {
        let Some(open) = self.screenshot_ui.open.as_ref() else {
            return;
        };
        let (x, y, w, h) = open.ui.rect();
        tracing::info!(rect = ?(x, y, w, h), output = open.ui.output, write_to_disk, "screenshot ui: confirm");
        let Some((texture, size)) = open.frozen.get(&open.ui.output) else {
            self.close_screenshot_ui();
            return;
        };
        let scale = open
            .ui
            .outputs
            .get(&open.ui.output)
            .map_or(1.0, |i| i.scale);
        let element = TextureRenderElement::from_static_texture(
            Id::new(),
            renderer.context_id(),
            Point::<f64, Physical>::from((f64::from(-x), f64::from(-y))),
            texture.clone(),
            1,
            Transform::Normal,
            None,
            None,
            Some(size.to_f64().to_logical(scale).to_i32_round()),
            None,
            Kind::Unspecified,
        );
        // 前から: ポインタ（出していれば）、止めた絵。
        let mut elements = Vec::with_capacity(2);
        if open.ui.show_pointer
            && let Some(pointer) = open.pointers.get(&open.ui.output)
        {
            elements.push(TextureRenderElement::from_static_texture(
                Id::new(),
                renderer.context_id(),
                Point::<f64, Physical>::from((f64::from(-x), f64::from(-y))),
                pointer.clone(),
                1,
                Transform::Normal,
                None,
                None,
                Some(size.to_f64().to_logical(scale).to_i32_round()),
                None,
                Kind::Unspecified,
            ));
        }
        elements.push(element);
        let result = crate::render::offscreen::render_to_rgba(
            renderer,
            Size::from((w, h)),
            scale,
            &elements,
            [0.0, 0.0, 0.0, 0.0],
        );
        self.close_screenshot_ui();
        match (result, u32::try_from(w), u32::try_from(h)) {
            (Ok(rgba), Ok(w), Ok(h)) => {
                crate::screenshot::deliver(self, w, h, &rgba, write_to_disk)
            }
            (Err(e), _, _) => tracing::warn!("screenshot ui: {e}"),
            _ => {}
        }
    }

    /// UI が開いていれば、出力 `output` に描く要素（前から: 枠、暗い帯、止めた絵）。描画先の左上が全体の論理座標の `origin`。
    pub fn screenshot_ui_elements(
        &self,
        renderer: &GlesRenderer,
        output: &Output,
        origin: Point<i32, Logical>,
    ) -> Option<Vec<SceneElement>> {
        let open = self.screenshot_ui.open.as_ref()?;
        let name = output.name();
        let geo = self.space.output_geometry(output)?;
        let scale = output.current_scale().fractional_scale();
        let offset: Point<i32, Physical> = (geo.loc - origin).to_physical_precise_round(scale);
        let p = open_progress(open.opened);
        let solid = |(x, y, w, h): Px, color: [f32; 4]| -> Option<SceneElement> {
            (w > 0 && h > 0).then(|| {
                SceneElement::Solid(SolidColorRenderElement::new(
                    Id::new(),
                    Rectangle::new(offset + Point::from((x, y)), Size::from((w, h))),
                    CommitCounter::default(),
                    color,
                    Kind::Unspecified,
                ))
            })
        };
        let (shades, border) = open.ui.shades_and_border(&name);
        let mut out: Vec<SceneElement> = Vec::new();
        // 説明のパネルが一番上（選択を描いている間は薄く。niri: 0.3、ふだん 0.9）。
        if let (Some((show, hide, size)), Some(info)) =
            (open.panels.get(&name), open.ui.outputs.get(&name))
        {
            let texture = if open.ui.show_pointer { hide } else { show };
            let (x, y) = panel_location(info.size, *size, scale);
            let alpha = if open.ui.is_button_down() && open.capture_down.is_none() {
                0.3
            } else {
                0.9
            };
            out.push(SceneElement::Frozen(
                TextureRenderElement::from_static_texture(
                    Id::new(),
                    renderer.context_id(),
                    (offset + Point::from((x, y))).to_f64(),
                    texture.clone(),
                    1,
                    Transform::Normal,
                    Some(alpha * p),
                    None,
                    Some(
                        Size::<f64, Physical>::from((f64::from(size.0), f64::from(size.1)))
                            .to_logical(scale)
                            .to_i32_round(),
                    ),
                    None,
                    Kind::Unspecified,
                ),
            ));
        }
        // 色は乗算済み。白い枠と、半透明の黒い帯（niri と同じ色）に、開く進みを掛ける。
        out.extend(border.into_iter().filter_map(|r| solid(r, [p, p, p, p])));
        out.extend(
            shades
                .into_iter()
                .filter_map(|r| solid(r, [0.0, 0.0, 0.0, 0.5 * p])),
        );
        if open.ui.show_pointer
            && let (Some(pointer), Some((_, size))) =
                (open.pointers.get(&name), open.frozen.get(&name))
        {
            out.push(SceneElement::Frozen(
                TextureRenderElement::from_static_texture(
                    Id::new(),
                    renderer.context_id(),
                    offset.to_f64(),
                    pointer.clone(),
                    1,
                    Transform::Normal,
                    None,
                    None,
                    Some(size.to_f64().to_logical(scale).to_i32_round()),
                    None,
                    Kind::Unspecified,
                ),
            ));
        }
        if let Some((texture, size)) = open.frozen.get(&name) {
            out.push(SceneElement::Frozen(
                TextureRenderElement::from_static_texture(
                    Id::new(),
                    renderer.context_id(),
                    offset.to_f64(),
                    texture.clone(),
                    1,
                    Transform::Normal,
                    None,
                    None,
                    Some(size.to_f64().to_logical(scale).to_i32_round()),
                    None,
                    Kind::Unspecified,
                ),
            ));
        }
        Some(out)
    }

    /// UI が開いているときのキー（押した）。UI が扱ったら true。Space はボタンを押している間は移動に使う。
    pub fn screenshot_ui_key(&mut self, name: &str, ctrl: bool, alt: bool, shift: bool) -> bool {
        let Some(open) = self.screenshot_ui.open.as_mut() else {
            return false;
        };
        if name == "space" && open.ui.is_button_down() {
            open.ui.set_space_down(true);
            return true;
        }
        match key_action(name, ctrl, alt, shift) {
            Some(UiKey::Cancel) => self.close_screenshot_ui(),
            Some(UiKey::Confirm) => open.confirm = Some(true),
            Some(UiKey::ConfirmToClipboard) => open.confirm = Some(false),
            Some(UiKey::TogglePointer) => {
                open.ui.show_pointer = !open.ui.show_pointer;
                tracing::info!(
                    show_pointer = open.ui.show_pointer,
                    "screenshot ui: pointer toggled"
                );
            }
            None => return false,
        }
        true
    }

    /// UI が開いているときの Space を離した。
    pub fn screenshot_ui_space_released(&mut self) {
        if let Some(open) = self.screenshot_ui.open.as_mut() {
            open.ui.set_space_down(false);
        }
    }

    /// UI が開いているときのキーバインド。niri の `allowed_during_screenshot` にあるものは、選択の操作か、そのまま実行（`false`）。
    /// それ以外は何もしない（`true`）。UI が閉じていれば `false`。
    pub fn screenshot_ui_action(&mut self, action: &Action) -> bool {
        if !self.screenshot_ui.is_open() {
            return false;
        }
        let neighbor = |me: &Self, d: contrast_core::Direction| -> Option<String> {
            let open = me.screenshot_ui.open.as_ref()?;
            let current = me
                .shell
                .outputs
                .values()
                .find(|o| o.name() == open.ui.output)?;
            me.output_in_direction(current, d).map(|o| o.name())
        };
        match action {
            Action::Quit | Action::PowerOffMonitors | Action::Spawn(_) => return false,
            Action::MoveLeft => self.with_ui(|ui| ui.move_by(-1, 0)),
            Action::MoveRight => self.with_ui(|ui| ui.move_by(1, 0)),
            Action::MoveUp => self.with_ui(|ui| ui.move_by(0, -1)),
            Action::MoveDown => self.with_ui(|ui| ui.move_by(0, 1)),
            Action::MoveColumnToMonitor(d) => {
                if let Some(name) = neighbor(self, *d) {
                    self.with_ui(|ui| ui.move_to_output(&name));
                }
            }
            Action::AdjustColumnWidth(p) => {
                let p = f64::from(*p);
                self.with_ui(|ui| ui.set_width(SizeChange::AdjustProportion(p)));
            }
            Action::SetColumnWidth(r) => {
                let p = f64::from(r.num()) * 100.0 / f64::from(r.den());
                self.with_ui(|ui| ui.set_width(SizeChange::SetProportion(p)));
            }
            Action::AdjustWindowHeight(p) => {
                let p = f64::from(*p);
                self.with_ui(|ui| ui.set_height(SizeChange::AdjustProportion(p)));
            }
            Action::SetWindowHeight(h) => {
                let change = match h {
                    contrast_core::WindowHeight::Proportion(r) => Some(SizeChange::SetProportion(
                        f64::from(r.num()) * 100.0 / f64::from(r.den()),
                    )),
                    contrast_core::WindowHeight::Fixed(px) => Some(SizeChange::SetFixed(*px)),
                    _ => None,
                };
                if let Some(change) = change {
                    self.with_ui(|ui| ui.set_height(change));
                }
            }
            _ => tracing::info!(?action, "screenshot ui: binding ignored while open"),
        }
        true
    }

    /// 開いている UI の状態を変える。
    fn with_ui(&mut self, f: impl FnOnce(&mut Ui)) {
        if let Some(open) = self.screenshot_ui.open.as_mut() {
            f(&mut open.ui);
        }
    }

    /// UI が開いているときのボタン（左）。押した・離した位置は全体の論理座標。`mod_down` は Mod を押したまま。
    pub fn screenshot_ui_button(
        &mut self,
        pressed: bool,
        pos: Point<f64, Logical>,
        mod_down: bool,
    ) {
        if pressed {
            let output = if mod_down {
                self.screenshot_ui.open.as_ref().and_then(|o| {
                    self.shell
                        .outputs
                        .values()
                        .find(|x| x.name() == o.ui.output)
                        .cloned()
                })
            } else {
                self.output_at(pos.x, pos.y)
            };
            let Some(output) = output else { return };
            let point = self.local_physical(&output, pos);
            let name = output.name();
            if !mod_down && self.on_capture_button(&name, point) {
                if let Some(open) = self.screenshot_ui.open.as_mut() {
                    open.capture_down = Some(name);
                }
                return;
            }
            tracing::info!(?pos, ?point, mod_down, "screenshot ui: pointer down");
            self.with_ui(|ui| {
                ui.pointer_down(&name, point, mod_down);
            });
        } else {
            // 撮影のボタンを押していたなら、離したのがボタンの上のときだけ保存する。
            if let Some(name) = self
                .screenshot_ui
                .open
                .as_mut()
                .and_then(|o| o.capture_down.take())
            {
                let on_button = self
                    .shell
                    .outputs
                    .values()
                    .find(|o| o.name() == name)
                    .cloned()
                    .is_some_and(|o| {
                        let point = self.local_physical(&o, pos);
                        self.on_capture_button(&name, point)
                    });
                if on_button && let Some(open) = self.screenshot_ui.open.as_mut() {
                    open.confirm = Some(true);
                }
                return;
            }
            self.with_ui(|ui| {
                ui.pointer_up();
            });
            if let Some(open) = self.screenshot_ui.open.as_ref() {
                tracing::info!(?pos, rect = ?open.ui.rect(), "screenshot ui: pointer up");
            }
        }
    }

    /// 出力 `name` の点 `point`（物理 px）が、説明のパネルの撮影のボタンの上か。
    fn on_capture_button(&self, name: &str, point: (i32, i32)) -> bool {
        let Some(open) = self.screenshot_ui.open.as_ref() else {
            return false;
        };
        let (Some((_, _, size)), Some(info)) = (open.panels.get(name), open.ui.outputs.get(name))
        else {
            return false;
        };
        let (x, y) = panel_location(info.size, *size, info.scale);
        within_capture_button(info.scale, *size, (point.0 - x, point.1 - y))
    }

    /// UI が開いているときのポインタの移動（全体の論理座標）。
    pub fn screenshot_ui_motion(&mut self, pos: Point<f64, Logical>) {
        let Some(name) = self
            .screenshot_ui
            .open
            .as_ref()
            .map(|o| o.ui.output.clone())
        else {
            return;
        };
        let Some(output) = self
            .shell
            .outputs
            .values()
            .find(|o| o.name() == name)
            .cloned()
        else {
            return;
        };
        let point = self.local_physical(&output, pos);
        self.with_ui(|ui| ui.pointer_motion(point));
    }

    /// 全体の論理座標を、出力 `output` の中の物理 px にする（出力の外でもよい）。
    fn local_physical(&self, output: &Output, pos: Point<f64, Logical>) -> (i32, i32) {
        let loc = self
            .space
            .output_geometry(output)
            .map(|g| g.loc.to_f64())
            .unwrap_or_default();
        let scale = output.current_scale().fractional_scale();
        let p = (pos - loc).to_physical(scale);
        (p.x.round() as i32, p.y.round() as i32)
    }

    /// 出力 `current` から見て、方向 `d` の出力（niri の `output_left_of` ほか。core の D78 と同じ規則）。
    pub fn output_in_direction(
        &self,
        current: &Output,
        d: contrast_core::Direction,
    ) -> Option<Output> {
        let g = self.space.output_geometry(current)?;
        let center = |r: Rectangle<i32, Logical>| (r.loc.x + r.size.w / 2, r.loc.y + r.size.h / 2);
        let (cx, cy) = center(g);
        let mut best: Option<(i32, Output)> = None;
        for o in self.shell.outputs.values() {
            let Some(r) = self.space.output_geometry(o) else {
                continue;
            };
            let (ox, oy) = center(r);
            let overlap_y = r.loc.y < g.loc.y + g.size.h && g.loc.y < r.loc.y + r.size.h;
            let overlap_x = r.loc.x < g.loc.x + g.size.w && g.loc.x < r.loc.x + r.size.w;
            let distance = match d {
                contrast_core::Direction::Left if ox < cx && overlap_y => cx - ox,
                contrast_core::Direction::Right if ox > cx && overlap_y => ox - cx,
                contrast_core::Direction::Up if oy < cy && overlap_x => cy - oy,
                contrast_core::Direction::Down if oy > cy && overlap_x => oy - cy,
                _ => continue,
            };
            if best.as_ref().is_none_or(|(b, _)| distance < *b) {
                best = Some((distance, o.clone()));
            }
        }
        best.map(|(_, o)| o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_sits_at_the_bottom_center_and_the_button_is_a_circle() {
        assert_eq!(panel_location((1000, 800), (400, 60), 1.0), (300, 724));
        // ボタンの中心は (8 + 14, 30)。半径 14。
        assert!(within_capture_button(1.0, (400, 60), (22, 30)));
        assert!(within_capture_button(1.0, (400, 60), (22, 44)));
        assert!(!within_capture_button(1.0, (400, 60), (22, 45)));
        assert!(!within_capture_button(1.0, (400, 60), (100, 30)));
    }

    #[test]
    fn the_panel_is_drawn_when_fonts_are_available() {
        if crate::text::fonts().is_none() {
            return;
        }
        let shown = draw_panel(1.0, true).map(|c| (c.width, c.height));
        let hidden = draw_panel(1.0, false).map(|c| (c.width, c.height));
        assert!(shown.is_some_and(|(w, h)| w > 200 && h >= 48), "{shown:?}");
        assert!(hidden.is_some());
    }
}
