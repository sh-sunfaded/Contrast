//! contrast-core への薄い翻訳層。
//!
//! - Wayland のイベント（窓の map/unmap、出力の追加・サイズ変更、キーバインド、クリック）を `Command` にする。
//! - `Core::apply` が返す `Effect` を Smithay の操作として実行する。
//! - `ExternalKey` はここで払い出し、`ExternalKey` ⇔ Smithay の `Window` の対応を持つ。
//!
//! 配置・フォーカスの判断は core だけが行う。ここに残すのは見た目の都合だけ:
//! タイルからフォーカス枠の幅だけ内側に縮めて窓を置くこと（枠の描画は `render`）。

use std::collections::BTreeMap;

use smithay::{
    desktop::Window,
    output::Output,
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::protocol::wl_surface::WlSurface,
    },
    utils::SERIAL_COUNTER,
    wayland::{compositor::with_states, shell::xdg::XdgToplevelSurfaceData},
};

use contrast_core::{
    Columns, Command, Core, CoreError, Effect, EntityId, ExternalKey, IdKind, OutputId, OutputName,
    Point, Rect, StateView, WindowId, WindowMeta, WindowState, WindowView,
};

use contrast_policy::{Policy, PortPlacement};

use crate::handler::fmt_id;
use crate::state::Contrast;

/// コンポジタが使う Core の型。窓の配置は `contrast_policy::Policy` に問い合わせる。
pub type ContrastCore = Core<PortPlacement<dyn Policy>, Columns>;

/// core とシェル側の対応。
#[derive(Default)]
pub struct Shell {
    /// 次に払い出す ExternalKey の raw 値。
    next_key: u64,
    /// 窓の ExternalKey ⇔ Smithay の窓（core に登録済みのもの）。
    pub windows: BTreeMap<ExternalKey, Window>,
    /// 作られたが最初の commit をまだ受けていない窓（core には未登録）。
    pub pending: Vec<Window>,
    /// 出力の ExternalKey ⇔ Smithay の出力（core に登録済みのもの）。
    pub outputs: BTreeMap<ExternalKey, Output>,
    /// 幅を窓に決めさせる窓（niri の `default-column-width {}`）で、最初のバッファを待っているもの。
    pub awaiting_buffer: Vec<Window>,
}

impl Shell {
    /// 新しい ExternalKey を払い出す。
    fn issue_key(&mut self) -> ExternalKey {
        self.next_key += 1;
        ExternalKey::new(self.next_key)
    }
}

/// ログ用の窓番号（WindowId の raw 値）。
fn raw(id: WindowId) -> u64 {
    id.raw()
}

/// Smithay の窓のトップレベル surface。
fn surface_of(window: &Window) -> WlSurface {
    window
        .toplevel()
        .expect("only wayland toplevels are managed")
        .wl_surface()
        .clone()
}

impl Contrast {
    // ---- 対応表の参照 ----

    /// surface に対応する窓の ExternalKey。
    pub fn key_of_surface(&self, surface: &WlSurface) -> Option<ExternalKey> {
        self.shell
            .windows
            .iter()
            .find(|(_, w)| &surface_of(w) == surface)
            .map(|(k, _)| *k)
    }

    /// surface に対応する Smithay の窓。
    pub fn window_of_surface(&self, surface: &WlSurface) -> Option<Window> {
        self.key_of_surface(surface)
            .and_then(|k| self.shell.windows.get(&k).cloned())
    }

    /// surface に対応する窓のログ用番号。
    pub fn log_id_of_surface(&self, surface: &WlSurface) -> Option<u64> {
        self.key_of_surface(surface)
            .and_then(|k| self.core.view().window_id(k))
            .map(raw)
    }

    /// core の WindowId に対応する Smithay の窓。
    fn window_of_id(&self, id: WindowId) -> Option<Window> {
        self.core
            .view()
            .window_key(id)
            .and_then(|k| self.shell.windows.get(&k).cloned())
    }

    // ---- 出力 ----

    /// 出力の ExternalKey の Smithay の出力。
    pub fn output_of_key(&self, key: ExternalKey) -> Option<&Output> {
        self.shell.outputs.get(&key)
    }

    /// Smithay の出力の ExternalKey。
    pub fn key_of_output(&self, output: &Output) -> Option<ExternalKey> {
        self.shell
            .outputs
            .iter()
            .find(|(_, o)| *o == output)
            .map(|(k, _)| *k)
    }

    /// core の OutputId の Smithay の出力。
    pub fn output_of_id(&self, id: OutputId) -> Option<Output> {
        let view = self.core.view();
        self.shell
            .outputs
            .iter()
            .find(|(k, _)| view.output_id(**k) == Some(id))
            .map(|(_, o)| o.clone())
    }

    /// Smithay の出力の core の OutputId。
    pub fn core_output(&self, output: &Output) -> Option<OutputId> {
        self.core.view().output_id(self.key_of_output(output)?)
    }

    /// アクティブな出力（core の D75。`Fixed` では D5 のフォーカス出力）。出力が無ければ `None`。
    pub fn active_output(&self) -> Option<Output> {
        self.output_of_id(self.core.view().active_output()?)
    }

    /// 点 `(x, y)`（全体の論理座標）を含む出力。
    pub fn output_at(&self, x: f64, y: f64) -> Option<Output> {
        self.space.output_under((x, y)).next().cloned()
    }

    /// 窓 `id` を表示している出力（表示中でなければ `None`）。
    pub fn output_of_window(&self, id: WindowId) -> Option<Output> {
        let view = self.core.view();
        let w = view.window(id)?;
        if !w.visible {
            return None;
        }
        self.output_of_id(view.workspace_output(w.workspace_id)?)
    }

    /// 出力 `output` に表示中の窓。
    pub fn windows_on(&self, output: &Output) -> Vec<WindowView> {
        let Some(id) = self.core_output(output) else {
            return Vec::new();
        };
        let view = self.core.view();
        view.visible_windows()
            .into_iter()
            .filter(|w| view.workspace_output(w.workspace_id) == Some(id))
            .collect()
    }

    /// ポインタを出力 `output` の中央（niri の `center`: 位置 + 大きさ / 2）へ動かす。
    pub fn warp_pointer_to(&mut self, output: &Output) {
        let Some(geo) = self.space.output_geometry(output) else {
            return;
        };
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let location = (
            f64::from(geo.loc.x + geo.size.w / 2),
            f64::from(geo.loc.y + geo.size.h / 2),
        )
            .into();
        let under = self.surface_under_point(location);
        pointer.motion(
            self,
            under,
            &smithay::input::pointer::MotionEvent {
                location,
                serial: SERIAL_COUNTER.next_serial(),
                time: 0,
            },
        );
        pointer.frame(self);
        tracing::info!(output = output.name(), "pointer warped to output");
    }

    /// 出力 `output` の表示中のワークスペースの、その出力の列の中の位置（0 始まり）。
    pub fn shown_position(&self, output: &Output) -> Option<u8> {
        let id = self.core_output(output)?;
        let view = self.core.view();
        let shown = view.workspace_id(view.shown_workspace(id)?)?;
        let at = view
            .output_workspaces(id)
            .iter()
            .position(|w| *w == shown)?;
        u8::try_from(at).ok()
    }

    /// フォーカス枠を描くべきタイル（出力 `output` に表示中のフォーカス窓のもの。全画面なら枠なし）。
    /// 窓が開く途中なら、窓と同じく中心を基準に縮めた矩形（niri はタイルの枠ごと拡大する）。
    pub fn focused_tile(&self, output: &Output) -> Option<Rect> {
        let view = self.core.view();
        let w = view.window(view.focus()?)?;
        if !w.visible || w.state == WindowState::Fullscreen {
            return None;
        }
        if self.output_of_window(w.id).as_ref() != Some(output) {
            return None;
        }
        let frame = w
            .rect
            .map(|tile| self.displayed_frame(w.id, self.frame_of(tile, w.state, Some(output))))?;
        Some(match self.focused_open_look() {
            Some((_, scale)) => crate::geometry::scaled_about_center(frame, scale),
            None => frame,
        })
    }

    /// 窓 `id` の枠（窓の外側の矩形、太さ、色）。全画面・枠なし（フォーカス外で `borderColor` が無い）は `None`。
    /// 閉じる窓の絵に枠を含めるのに使う（フレームの描画の枠と同じ規則）。
    pub fn window_border(&self, id: WindowId, output: &Output) -> Option<(Rect, i32, [f32; 4])> {
        let view = self.core.view();
        let w = view.window(id)?;
        if !w.visible || w.state == WindowState::Fullscreen {
            return None;
        }
        let frame = self.displayed_frame(id, self.frame_of(w.rect?, w.state, Some(output)));
        let b = self.policy.border_width();
        let (width, color) = if view.focus() == Some(id) {
            (b, self.policy.focus_border_color())
        } else {
            let thin =
                crate::render::unfocused_width(b, self.policy.appearance().inactive_border_width);
            (thin, self.policy.unfocused_border_color()?)
        };
        Some((crate::geometry::outset(frame, width), width, color))
    }

    /// フォーカス窓が開く途中なら、その `(透明度, 拡大率)`。
    pub fn focused_open_look(&self) -> Option<(f32, f64)> {
        let view = self.core.view();
        let key = view.window_key(view.focus()?)?;
        self.window_anims.open_look_of(key)
    }

    /// 窓 `id` を今描くフレーム。補間の途中なら途中の矩形、そうでなければ行き先の `target`。
    fn displayed_frame(&self, id: WindowId, target: Rect) -> Rect {
        self.anim.displayed(id).unwrap_or(target)
    }

    /// 出力 `output` のフォーカス外の表示中の窓のタイル（全画面は除く）。フォーカス外の枠を描くのに使う。
    pub fn unfocused_tiles(&self, output: &Output) -> Vec<Rect> {
        let focus = self.core.view().focus();
        self.windows_on(output)
            .iter()
            .filter(|w| Some(w.id) != focus && w.state != WindowState::Fullscreen)
            .filter_map(|w| {
                w.rect.map(|tile| {
                    self.displayed_frame(w.id, self.frame_of(tile, w.state, Some(output)))
                })
            })
            .collect()
    }

    /// core が窓を並べる領域（出力の論理矩形から、layer-shell の排他ゾーンを引いたもの）。出力がなければ `None`。
    pub fn work_area(&self, output: &Output) -> Option<Rect> {
        let geo = self.space.output_geometry(output)?;
        let zone = smithay::desktop::layer_map_for_output(output).non_exclusive_zone();
        Rect::new(
            geo.loc.x.saturating_add(zone.loc.x),
            geo.loc.y.saturating_add(zone.loc.y),
            zone.size.w,
            zone.size.h,
        )
    }

    /// layer-shell の排他ゾーン（バーなど）を core の出力の余白として渡す。layer の追加・削除・サイズ変更（`arrange` のあと）と、
    /// 出力の大きさが変わったあとに呼ぶ。同じ値なら core は何もしない。
    pub fn sync_output_insets(&mut self, output: &Output) {
        let Some(key) = self.key_of_output(output) else {
            return;
        };
        let Some(geo) = self.space.output_geometry(output) else {
            return;
        };
        let zone = smithay::desktop::layer_map_for_output(output).non_exclusive_zone();
        let (top, right, bottom, left) = crate::geometry::insets_from_zone(
            (geo.size.w, geo.size.h),
            (zone.loc.x, zone.loc.y, zone.size.w, zone.size.h),
        );
        let Some(insets) = contrast_core::Insets::new(top, right, bottom, left) else {
            return;
        };
        // 窓が出力の変更に追いついて動くので、補間は付けない（バーが出る・消えるときは窓を直ちに置く）。
        self.anim.suppress(true);
        self.submit(Command::OutputInsetsChanged { key, insets });
        self.anim.suppress(false);
    }

    /// すべての出力の余白を取り直す。
    pub fn sync_all_output_insets(&mut self) {
        let outputs: Vec<Output> = self.shell.outputs.values().cloned().collect();
        for output in outputs {
            self.sync_output_insets(&output);
        }
    }

    /// 出力 `output` に表示中の窓に全画面のものがあるか。
    pub fn has_visible_fullscreen(&self, output: &Output) -> bool {
        self.windows_on(output)
            .iter()
            .any(|w| w.state == WindowState::Fullscreen)
    }

    /// タイルから、隙間（`gap`・`outerGap`）を引いたフレーム（枠線の外側の矩形）。浮動・全画面は隙間なし。
    /// `output` は窓を表示している出力（スクロール列で、画面の端に接する辺に `outerGap` を引く基準）。
    pub fn frame_of(&self, tile: Rect, state: WindowState, output: Option<&Output>) -> Rect {
        let layout = self.policy.layout_settings();
        if state != WindowState::Tiled || (layout.gap <= 0 && layout.outer_gap <= 0) {
            return tile;
        }
        let bounds = if self.layout_mode == contrast_core::LayoutMode::Scrolling {
            // スクロール列では、タイルが画面の外まで続く。画面（作業領域）の端に接する辺に `outerGap` を引く。
            output.and_then(|o| self.work_area(o))
        } else {
            let tiles = output
                .map(|o| self.windows_on(o))
                .unwrap_or_default()
                .into_iter()
                .filter(|w| w.state == WindowState::Tiled)
                .filter_map(|w| w.rect);
            crate::geometry::bounds_of(tiles)
        };
        crate::geometry::frame_rect(tile, bounds.unwrap_or(tile), layout.gap, layout.outer_gap)
    }

    /// 表示中の窓の配置をポリシーの今の値（枠の太さなど）で取り直す。設定のリロードのあとに呼ぶ。
    pub fn reapply_layout(&mut self) {
        let placed: Vec<(WindowId, Rect, WindowState)> = self
            .core
            .view()
            .visible_windows()
            .iter()
            .filter_map(|w| w.rect.map(|r| (w.id, r, w.state)))
            .collect();
        for (id, rect, state) in placed {
            self.configure(id, rect, state);
        }
        self.sync_space();
    }

    /// 点 `(x, y)` の出力に表示中で、タイル（枠を含む）がその点を含む窓の ExternalKey。上に描かれている窓を優先する。
    /// 隣の出力の範囲にはみ出したスクロール列の窓は、その出力では当たらない。
    pub fn visible_key_at(&self, x: i32, y: i32) -> Option<ExternalKey> {
        let p = Point::new(x, y);
        let output = self.output_at(f64::from(x), f64::from(y))?;
        let view = self.core.view();
        let mut visible = self.windows_on(&output);
        visible.sort_by_key(|w| stacking(w.state));
        visible
            .iter()
            .rev()
            .find(|w| w.rect.is_some_and(|r| r.contains(p)))
            .and_then(|w| view.window_key(w.id))
    }

    // ---- Wayland のイベント → Command ----

    /// 出力ができた（Space に置いたあとに呼ぶ）。
    pub fn output_added(&mut self, output: &Output, geometry: Rect) {
        let key = self.shell.issue_key();
        self.shell.outputs.insert(key, output.clone());
        // 出力が増えると、別の出力のワークスペースが移ってくることがある（D76）。窓はすぐ新しい矩形に置く。
        self.anim.suppress(true);
        let ok = self.submit(Command::OutputAdded {
            key,
            name: OutputName::new(crate::edid::identity_of(output).core_name()),
            geometry,
        });
        self.anim.suppress(false);
        if !ok {
            self.shell.outputs.remove(&key);
        }
    }

    /// 出力が外れた（Space から外す前に呼ぶ）。窓のあるワークスペースは、ほかの出力へ移る（D77）。
    pub fn output_removed(&mut self, output: &Output) {
        let Some(key) = self.key_of_output(output) else {
            return;
        };
        self.shell.outputs.remove(&key);
        self.anim.suppress(true);
        self.submit(Command::OutputRemoved(key));
        self.anim.suppress(false);
    }

    /// 出力の大きさか位置が変わった。
    pub fn output_changed(&mut self, output: &Output, geometry: Rect) {
        let Some(key) = self.key_of_output(output) else {
            return;
        };
        // 出力の大きさが変わるときは、窓をすぐ新しい矩形に置く（大きさの変更を追いかけて補間しない）。
        self.anim.suppress(true);
        self.submit(Command::OutputChanged { key, geometry });
        self.anim.suppress(false);
    }

    /// トップレベル窓ができた。
    ///
    /// core にはまだ送らず、最初の commit（`map_pending`）まで保留する。
    pub fn window_created(&mut self, window: Window) {
        self.shell.pending.push(window);
    }

    /// 保留中の窓の最初の commit。app_id と title をここで確定させて core に WindowMapped を送る。
    ///
    /// xdg-shell ではクライアントが app_id・title を設定してから最初の commit をするので、
    /// この時点で無ければ「無い」と確定する。保留中の窓でなければ何もしない。
    pub fn map_pending(&mut self, surface: &WlSurface) {
        // 幅を窓に決めさせる窓: 最初のバッファが来たら、その大きさで登録する（niri は列の幅を窓の幅にする）。
        if let Some(index) = self
            .shell
            .awaiting_buffer
            .iter()
            .position(|w| &surface_of(w) == surface)
        {
            let Some(window) = self.shell.awaiting_buffer.get(index).cloned() else {
                return;
            };
            window.on_commit();
            let size = window.geometry().size;
            if size.w <= 0 || size.h <= 0 {
                return;
            }
            self.shell.awaiting_buffer.remove(index);
            let mut meta = read_meta(&window);
            meta.requested_size = contrast_core::Size::new(size.w, size.h);
            tracing::info!(app_id = ?meta.app_id, w = size.w, h = size.h, "window chose its width (default-column-width {{}})");
            if let Some(key) = self.window_mapped(window, meta) {
                self.fit_column_to_window_width(key, size.w);
            }
            return;
        }
        let Some(index) = self
            .shell
            .pending
            .iter()
            .position(|w| &surface_of(w) == surface)
        else {
            return;
        };
        let window = self.shell.pending.remove(index);
        let meta = read_meta(&window);
        tracing::info!(app_id = ?meta.app_id, title = ?meta.title, is_dialog = meta.is_dialog, "window first commit");
        let decided = self
            .policy
            .place_window(&meta, self.shown_workspace_index());
        if decided.column_width == Some(contrast_policy::PlacementWidth::Window)
            && decided.state == WindowState::Tiled
        {
            // 最初の configure の幅を 0 にして、窓に決めさせる（高さは作業領域から上下の隙間を引いた高さ）。
            let height = self
                .active_output()
                .and_then(|o| self.work_area(&o))
                .map_or(0, |a| {
                    a.height().saturating_sub(
                        self.policy
                            .layout_settings()
                            .outer_gap
                            .max(0)
                            .saturating_mul(2),
                    )
                });
            if let Some(toplevel) = window.toplevel() {
                toplevel.with_pending_state(|s| s.size = Some((0, height.max(0)).into()));
                toplevel.send_configure();
            }
            self.shell.awaiting_buffer.push(window);
            return;
        }
        self.window_mapped(window, meta);
    }

    /// 窓を core に登録する。ポリシーが「開いた直後に全画面」を求めていれば、配置のあとに全画面にする。
    fn window_mapped(&mut self, window: Window, meta: WindowMeta) -> Option<ExternalKey> {
        let key = self.shell.issue_key();
        self.shell.windows.insert(key, window);
        // core の配置（`PortPlacement`）と同じ問い合わせ。フックは純粋なので、答えは同じになる。
        let want_fullscreen = self
            .policy
            .place_window(&meta, self.shown_workspace_index())
            .fullscreen;
        if !self.submit(Command::WindowMapped { key, meta }) {
            // core に登録できなかった窓は管理しない（docs/questions.md Q9）。
            self.shell.windows.remove(&key);
            return None;
        }
        if want_fullscreen {
            match fullscreen_command(&self.core.view(), key) {
                Some(command) => {
                    tracing::info!("open-fullscreen: window requested fullscreen");
                    self.submit(command);
                }
                None => tracing::warn!(
                    "open-fullscreen ignored: the window did not get the focus (focus=false or hidden workspace)"
                ),
            }
        }
        Some(key)
    }

    /// 窓に決めさせた列（`default-column-width {}`）の幅を、フレーム（隙間と外側の隙間を引いた矩形）の幅が `want` になるよう直す。
    /// core の列の幅は隙間を含む（`PortPlacement` は `want + gap` を渡す）が、画面の端の列は外側の隙間が `gap / 2` と違うので、
    /// 位置が決まったあとで、実際のフレームとの差だけ列の幅を直す（niri の列の幅は隙間を含まない）。フォーカス列にだけ効く。
    fn fit_column_to_window_width(&mut self, key: ExternalKey, want: i32) {
        // 幅を変えると端の判定が変わりうるので、2 回まで。
        for _ in 0..2 {
            let view = self.core.view();
            let Some(id) = view.window_id(key) else {
                return;
            };
            let Some(w) = view.window(id) else {
                return;
            };
            let (Some(tile), true) = (
                w.rect,
                w.state == WindowState::Tiled && view.focus() == Some(id),
            ) else {
                return;
            };
            let output = self.output_of_window(id);
            let delta = self.frame_of(tile, w.state, output.as_ref()).width() - want;
            let Some(width) = contrast_core::ColumnWidth::fixed(tile.width().saturating_sub(delta))
            else {
                return;
            };
            if delta == 0 {
                return;
            }
            self.core_action(contrast_core::Action::SetColumnWidth(width));
        }
    }

    /// トップレベル窓が消えた（クライアントの終了を含む）。
    ///
    /// 最初の commit の前に閉じられた窓は保留から外すだけで、core には何も送らない。
    pub fn window_unmapped(&mut self, surface: &WlSurface) {
        // 最初のバッファを待っていた窓は、待つのをやめるだけ（core には登録していない）。
        self.shell
            .awaiting_buffer
            .retain(|w| &surface_of(w) != surface);
        if let Some(index) = self
            .shell
            .pending
            .iter()
            .position(|w| &surface_of(w) == surface)
        {
            self.shell.pending.remove(index);
            tracing::info!("window closed before map");
            return;
        }
        let Some(key) = self.key_of_surface(surface) else {
            return;
        };
        let id = self.core.view().window_id(key);
        tracing::info!("window removed window={}", fmt_id(id.map(raw)));
        if let Some(id) = id {
            self.anim.forget(id);
        }
        // niri の window-close: 最後のフレームの絵を、縮めながら消す。
        let enabled = self.policy.layout_settings().animation_ms != 0;
        self.window_anims.start_closing(key, enabled);
        if let Some(window) = self.shell.windows.remove(&key) {
            self.space.unmap_elem(&window);
        }
        self.submit(Command::WindowUnmapped(key));
    }

    /// 座標の下の窓へのフォーカス要求（クリック）。窓の外なら何もしない。
    pub fn focus_at(&mut self, x: f64, y: f64) {
        let Some(key) = self.visible_key_at(x.floor() as i32, y.floor() as i32) else {
            // 窓の無い所: その出力をアクティブにする（niri の `focus_output`。D80）。
            if let Some(output) = self.output_at(x, y)
                && let Some(key) = self.key_of_output(&output)
            {
                self.submit(Command::OutputFocusRequested(key));
            }
            return;
        };
        let id = self.core.view().window_id(key);
        tracing::info!("click focus window={} x={} y={}", fmt_id(id.map(raw)), x, y);
        self.submit(Command::WindowFocusRequested(key));
    }

    /// ポインタが座標の下の窓に入った。その窓がフォーカスされていなければ、フォーカスを要求する
    /// （`Input.focusFollowsMouse`）。窓の外（空き領域）では何もしない。
    pub fn hover_focus(&mut self, x: f64, y: f64) {
        let Some(key) = self.visible_key_at(x.floor() as i32, y.floor() as i32) else {
            return;
        };
        let need = {
            let view = self.core.view();
            needs_focus(view.focus(), view.window_id(key))
        };
        if need {
            tracing::info!("focus follows mouse x={} y={}", x, y);
            self.submit(Command::WindowFocusRequested(key));
        }
    }

    /// core の Action を適用する。
    pub fn core_action(&mut self, action: contrast_core::Action) {
        self.submit(Command::Action(action));
    }

    /// Command を core に渡す（ほかのモジュールから使う口）。成功なら true。
    pub(crate) fn submit_command(&mut self, command: Command) -> bool {
        self.submit(command)
    }

    /// Command を core に渡し、返った Effect を実行する。成功なら true。
    fn submit(&mut self, command: Command) -> bool {
        match self.core.apply(command.clone()) {
            Ok(applied) => {
                for effect in applied.effects {
                    self.run_effect(effect);
                }
                self.anim.end_slide();
                // ワークスペースの表示・フォーカスの変化を、IPC の購読者に伝える。
                self.ipc_sync();
                true
            }
            Err(CoreError::NoFocusedWindow) => {
                tracing::info!(?command, "no focused window, ignored");
                false
            }
            Err(err) => {
                tracing::warn!(?command, ?err, "core rejected command");
                false
            }
        }
    }

    // ---- Effect → Smithay ----

    /// Effect を 1 つ実行する。
    fn run_effect(&mut self, effect: Effect) {
        match effect {
            Effect::Registered {
                id: EntityId::Window(id),
                ..
            } => {
                let ws = self.core.view().window(id).map(|w| w.workspace.get() + 1);
                tracing::info!(
                    "window added window={} workspace={}",
                    raw(id),
                    fmt_id(ws.map(u64::from))
                );
            }
            Effect::Registered {
                id: EntityId::Output(id),
                ..
            } => tracing::info!(output = ?id, "output registered"),
            Effect::ShowWorkspace { output, .. } => {
                let smithay_output = self.output_of_id(output);
                let shown = smithay_output.as_ref().and_then(|o| self.shown_position(o));
                tracing::info!(
                    output = smithay_output.as_ref().map(|o| o.name()),
                    "show workspace={}",
                    fmt_id(shown.map(|i| u64::from(i) + 1))
                );
                // 新しく見える窓が、切り替えの向きに合わせて画面の外から入ってくる（`Configure` の補間）。
                let area = smithay_output.as_ref().and_then(|o| self.work_area(o));
                if let (Some(o), Some(shown), Some(area)) = (smithay_output.as_ref(), shown, area) {
                    self.anim.workspace_shown(&o.name(), shown, area.height());
                }
                self.sync_space();
            }
            Effect::Configure {
                window,
                rect,
                state,
            } => self.configure(window, rect, state),
            Effect::SetFocus(focus) => self.set_focus(focus),
            Effect::RequestClose(id) => {
                tracing::info!("close focused window={}", raw(id));
                if let Some(t) = self
                    .window_of_id(id)
                    .as_ref()
                    .and_then(|w| w.toplevel().cloned())
                {
                    t.send_close();
                }
            }
            Effect::Damage(_) => self.sync_space(),
            _ => tracing::warn!(?effect, "unhandled effect"),
        }
    }

    /// Configure: 窓の行き先の矩形を受け取る。補間が有効なら、いまの矩形から行き先へ近づける
    /// （`tick_animations` が毎フレーム進める）。無効なら、すぐ行き先に置く。
    fn configure(&mut self, id: WindowId, tile: Rect, state: WindowState) {
        let Some(window) = self.window_of_id(id) else {
            return;
        };
        let border = self.policy.border_width().max(0);
        let output = self.output_of_window(id);
        let frame = match state {
            WindowState::Fullscreen => tile,
            _ => self.frame_of(tile, state, output.as_ref()),
        };
        // 窓はフレームそのもの（枠は外側に描く）。
        let to_window = |frame: Rect| frame;
        // 全画面にするときは補間しない（全画面から戻るときは、全画面の矩形から補間する）。
        let motion = self.animation_motion();
        let animate = state != WindowState::Fullscreen;
        let now = std::time::Instant::now();
        let shown = self.anim.retarget(id, frame, now, motion, animate);
        self.apply_rect(&window, to_window(shown), state);
        let rect = to_window(frame);
        tracing::info!(
            "layout window={} x={} y={} w={} h={} border={} state={:?}",
            raw(id),
            rect.x(),
            rect.y(),
            rect.width(),
            rect.height(),
            border,
            state
        );
    }

    /// 補間の曲線（`Layout.animation` と `Layout.animationMs`）。`animationMs` が 0 なら補間しない。
    fn animation_motion(&self) -> crate::anim::Motion {
        use contrast_policy::AnimationKind;
        let settings = self.policy.layout_settings();
        let off = settings.animation_ms == 0;
        match settings.animation {
            AnimationKind::Spring if !off => crate::anim::Motion::Spring,
            _ => crate::anim::Motion::Easing(std::time::Duration::from_millis(u64::from(
                settings.animation_ms,
            ))),
        }
    }

    /// 窓に大きさと状態を伝え、Space 上の位置を合わせる（ログなし。補間の毎フレームからも呼ぶ）。
    fn apply_rect(&mut self, window: &Window, rect: Rect, state: WindowState) {
        let toplevel = window.toplevel().expect("wayland toplevel");
        toplevel.with_pending_state(|s| {
            s.size = Some((rect.width(), rect.height()).into());
            // Tiled 状態は送らない（niri の `prefer-no-csd` が無効のときと同じ。クライアントが CSD の角丸などを保つ）。
            if state == WindowState::Fullscreen {
                s.states.set(xdg_toplevel::State::Fullscreen);
            } else {
                s.states.unset(xdg_toplevel::State::Fullscreen);
            }
        });
        // 初回 configure は最初の commit で送る（handler::xdg_shell）。
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
        self.space
            .map_element(window.clone(), (rect.x(), rect.y()), false);
    }

    /// 補間を 1 フレーム進める（描画の前に呼ぶ）。まだ続くなら `true`（次のフレームも描く）。
    pub fn tick_animations(&mut self) -> bool {
        for (id, frame) in self.anim.tick(std::time::Instant::now()) {
            let Some(window) = self.window_of_id(id) else {
                continue;
            };
            let Some(state) = self.core.view().window(id).map(|w| w.state) else {
                continue;
            };
            // 窓はフレームそのもの（枠は外側に描く）。
            let rect = frame;
            tracing::trace!(
                "anim step window={} x={} y={} w={} h={}",
                raw(id),
                rect.x(),
                rect.y(),
                rect.width(),
                rect.height()
            );
            self.apply_rect(&window, rect, state);
        }
        self.anim.is_running()
    }

    /// キーボードフォーカスを core のフォーカス窓に戻す（レイヤー面が消えたあと）。
    pub fn restore_window_focus(&mut self) {
        let focus = self.core.view().focus();
        self.set_focus(focus);
    }

    /// SetFocus: キーボードフォーカスと activated 状態を合わせる。
    fn set_focus(&mut self, focus: Option<WindowId>) {
        let target = focus.and_then(|id| self.window_of_id(id));
        for w in self.shell.windows.values() {
            w.set_activated(Some(w) == target.as_ref());
            let toplevel = w.toplevel().expect("wayland toplevel");
            if toplevel.is_initial_configure_sent() {
                toplevel.send_pending_configure();
            }
        }
        let keyboard = self.seat.get_keyboard().expect("seat has keyboard");
        keyboard.set_focus(
            self,
            target.as_ref().map(surface_of),
            SERIAL_COUNTER.next_serial(),
        );
        tracing::info!("focus window={}", fmt_id(focus.map(raw)));
    }

    /// 表示中の窓だけを core の矩形で Space に置き、それ以外は外す。
    fn sync_space(&mut self) {
        let view = self.core.view();
        let mut visible = view.visible_windows();
        visible.sort_by_key(|w| stacking(w.state));
        let mut shown = Vec::new();
        for w in &visible {
            let (Some(tile), Some(key)) = (w.rect, view.window_key(w.id)) else {
                continue;
            };
            let Some(window) = self.shell.windows.get(&key).cloned() else {
                continue;
            };
            // 補間の途中の窓は、途中の矩形に置く（行き先ではない）。
            let rect = match w.state {
                WindowState::Fullscreen => tile,
                _ => {
                    let output = self.output_of_window(w.id);
                    self.displayed_frame(w.id, self.frame_of(tile, w.state, output.as_ref()))
                }
            };
            shown.push((window, rect));
        }
        let visible_ids: std::collections::BTreeSet<WindowId> =
            visible.iter().map(|w| w.id).collect();
        self.anim.retain(&visible_ids);
        let hidden: Vec<Window> = self
            .shell
            .windows
            .values()
            .filter(|w| !shown.iter().any(|(s, _)| s == *w))
            .cloned()
            .collect();
        for w in hidden {
            self.space.unmap_elem(&w);
        }
        for (window, rect) in shown {
            self.space.map_element(window, (rect.x(), rect.y()), false);
        }
    }
}

/// ポインタの下の窓 `hovered` にフォーカスを移すべきか（窓があり、今のフォーカスと違うとき）。
fn needs_focus(current: Option<WindowId>, hovered: Option<WindowId>) -> bool {
    hovered.is_some() && hovered != current
}

/// 登録した直後の窓 `key` を全画面にする `Command`。窓がフォーカスを得たときだけ（`ToggleFullscreen` はフォーカス窓に効く）。
fn fullscreen_command(view: &StateView<'_>, key: ExternalKey) -> Option<Command> {
    let id = view.window_id(key)?;
    (view.focus() == Some(id)).then_some(Command::Action(contrast_core::Action::ToggleFullscreen))
}

/// 重なり順（小さいほど下）: Tiled → Floating → Fullscreen。
fn stacking(state: WindowState) -> u8 {
    match state {
        WindowState::Tiled => 0,
        WindowState::Floating => 1,
        _ => 2,
    }
}

/// トップレベルの app_id・title・親の有無から WindowMeta を作る。
pub(crate) fn read_meta(window: &Window) -> WindowMeta {
    let toplevel = window.toplevel().expect("wayland toplevel");
    let (app_id, title) = with_states(toplevel.wl_surface(), |states| {
        let data = states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .expect("toplevel role data")
            .lock()
            .expect("toplevel data lock");
        (data.app_id.clone(), data.title.clone())
    });
    let mut meta = WindowMeta::default();
    meta.app_id = app_id;
    meta.title = title;
    meta.is_dialog = toplevel.parent().is_some();
    meta
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use contrast_core::{CoreConfig, OutputName};
    use contrast_policy::{
        Config, ConfigDiagnostic, ConfigStore, HookPlacement, PlacementHook, Settings, WindowInfo,
    };

    use super::*;

    /// 毎回同じ配置を返すフック。
    struct Hook {
        focus: bool,
        fullscreen: bool,
    }

    impl PlacementHook for Hook {
        fn window_open(&self, _w: &WindowInfo) -> Result<Option<HookPlacement>, ConfigDiagnostic> {
            Ok(Some(HookPlacement {
                workspace: None,
                floating: false,
                focus: self.focus,
                fullscreen: self.fullscreen,
                column_width: None,
            }))
        }
    }

    /// 出力 1 つの core。窓の配置は `hook` に従う。
    fn core_with(hook: Hook) -> Result<(ContrastCore, Rc<dyn Policy>), String> {
        let store = Rc::new(ConfigStore::new(Config::new(
            Settings::builtin(),
            Some(Box::new(hook)),
        )));
        let policy: Rc<dyn Policy> = store;
        let mut core = ContrastCore::new(
            CoreConfig::new(policy.workspace_count()),
            PortPlacement::new(Rc::clone(&policy)),
            Columns::new(),
        );
        let geometry = Rect::new(0, 0, 800, 600).ok_or("rect")?;
        core.apply(Command::OutputAdded {
            key: ExternalKey::new(1),
            name: OutputName::new("test"),
            geometry,
        })
        .map_err(|e| format!("{e:?}"))?;
        Ok((core, policy))
    }

    /// 窓 `n` を登録する。
    fn map(core: &mut ContrastCore, n: u64) -> Result<ExternalKey, String> {
        let key = ExternalKey::new(n);
        core.apply(Command::WindowMapped {
            key,
            meta: WindowMeta::default(),
        })
        .map_err(|e| format!("{e:?}"))?;
        Ok(key)
    }

    #[test]
    fn open_fullscreen_makes_the_focused_new_window_fullscreen() -> Result<(), String> {
        let (mut core, policy) = core_with(Hook {
            focus: true,
            fullscreen: true,
        })?;
        assert!(
            policy
                .place_window(&WindowMeta::default(), Some(0))
                .fullscreen
        );
        let key = map(&mut core, 2)?;
        let command = fullscreen_command(&core.view(), key).ok_or("focused: expected a command")?;
        core.apply(command).map_err(|e| format!("{e:?}"))?;
        let view = core.view();
        let id = view.window_id(key).ok_or("id")?;
        assert_eq!(
            view.window(id).map(|w| w.state),
            Some(WindowState::Fullscreen)
        );
        Ok(())
    }

    #[test]
    fn open_fullscreen_is_skipped_when_the_window_is_not_focused() -> Result<(), String> {
        let (mut core, _) = core_with(Hook {
            focus: true,
            fullscreen: false,
        })?;
        let first = map(&mut core, 2)?;
        // 2 枚目は focus=false の配置にしたいので、別の core を作る代わりに、フォーカスが first のままなことを使う。
        let (mut core2, _) = core_with(Hook {
            focus: false,
            fullscreen: true,
        })?;
        let a = map(&mut core2, 2)?;
        // focus=false の窓はフォーカスを得ない。
        assert!(fullscreen_command(&core2.view(), a).is_none());
        // 知らない key には何もしない。
        assert!(fullscreen_command(&core.view(), ExternalKey::new(99)).is_none());
        assert!(fullscreen_command(&core.view(), first).is_some());
        Ok(())
    }

    #[test]
    fn focus_moves_only_to_a_different_window() -> Result<(), String> {
        let (mut core, _) = core_with(Hook {
            focus: true,
            fullscreen: false,
        })?;
        let a = map(&mut core, 2)?;
        let b = map(&mut core, 3)?;
        let view = core.view();
        let (ida, idb) = (view.window_id(a), view.window_id(b));
        // 新しい窓 b がフォーカスを持っている。
        assert_eq!(view.focus(), idb);
        assert!(needs_focus(view.focus(), ida));
        assert!(!needs_focus(view.focus(), idb));
        assert!(!needs_focus(view.focus(), None));
        assert!(needs_focus(None, ida));
        Ok(())
    }
}
