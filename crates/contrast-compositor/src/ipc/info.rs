//! `Contrast` の状態から作る、窓・出力・レイヤー面・キーボードの配列の情報（niri-ipc の型）。

use smithay::{
    desktop::layer_map_for_output,
    output::Output as SmOutput,
    utils::{Clock, Monotonic, Transform as SmTransform},
    wayland::shell::wlr_layer::{KeyboardInteractivity as KI, Layer as SmLayer},
};

use contrast_core::{IdKind, WindowState};
use smithay::reexports::wayland_server::Resource;

use super::protocol::{
    KeyboardInteractivity, KeyboardLayouts, Layer, LayerSurface, LogicalOutput, Mode, Output,
    Timestamp, Transform, Window, WindowLayout,
};
use super::state::Extra;
use crate::state::Contrast;

/// smithay の回転を niri-ipc の回転にする。
fn transform(t: SmTransform) -> Transform {
    match t {
        SmTransform::Normal => Transform::Normal,
        SmTransform::_90 => Transform::Rotate90,
        SmTransform::_180 => Transform::Rotate180,
        SmTransform::_270 => Transform::Rotate270,
        SmTransform::Flipped => Transform::Flipped,
        SmTransform::Flipped90 => Transform::Flipped90,
        SmTransform::Flipped180 => Transform::Flipped180,
        SmTransform::Flipped270 => Transform::Flipped270,
    }
}

/// 出力 1 つの情報。
fn output_info(contrast: &Contrast, output: &SmOutput) -> Output {
    let props = output.physical_properties();
    let current = output.current_mode();
    let preferred = output.preferred_mode();
    let modes: Vec<Mode> = output
        .modes()
        .iter()
        .map(|m| Mode {
            width: u16::try_from(m.size.w).unwrap_or(u16::MAX),
            height: u16::try_from(m.size.h).unwrap_or(u16::MAX),
            refresh_rate: u32::try_from(m.refresh).unwrap_or(0),
            is_preferred: Some(*m) == preferred,
        })
        .collect();
    let scale = output.current_scale().fractional_scale();
    let logical = contrast
        .space
        .output_geometry(output)
        .map(|g| LogicalOutput {
            x: g.loc.x,
            y: g.loc.y,
            width: u32::try_from(g.size.w).unwrap_or(0),
            height: u32::try_from(g.size.h).unwrap_or(0),
            scale,
            transform: transform(output.current_transform()),
        });
    Output {
        name: output.name(),
        make: props.make,
        model: props.model,
        serial: None,
        physical_size: u32::try_from(props.size.w)
            .ok()
            .zip(u32::try_from(props.size.h).ok()),
        current_mode: current.and_then(|c| output.modes().iter().position(|m| *m == c)),
        modes,
        is_custom_mode: false,
        vrr_supported: false,
        vrr_enabled: false,
        logical,
        max_bpc: None,
    }
}

impl Contrast {
    /// 出力の一覧（名前順）。
    pub(crate) fn ipc_output_list(&self) -> Vec<Output> {
        let mut list: Vec<Output> = self
            .shell
            .outputs
            .values()
            .map(|o| output_info(self, o))
            .collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }

    /// レイヤー面の一覧（出力の名前順、層の順）。
    pub(crate) fn ipc_layer_list(&self) -> Vec<LayerSurface> {
        let mut out = Vec::new();
        for output in self.shell.outputs.values() {
            let map = layer_map_for_output(output);
            for layer in map.layers() {
                out.push(LayerSurface {
                    namespace: layer.namespace().to_owned(),
                    output: output.name(),
                    layer: match layer.layer() {
                        SmLayer::Background => Layer::Background,
                        SmLayer::Bottom => Layer::Bottom,
                        SmLayer::Top => Layer::Top,
                        SmLayer::Overlay => Layer::Overlay,
                    },
                    keyboard_interactivity: match layer.cached_state().keyboard_interactivity {
                        KI::None => KeyboardInteractivity::None,
                        KI::Exclusive => KeyboardInteractivity::Exclusive,
                        KI::OnDemand => KeyboardInteractivity::OnDemand,
                    },
                });
            }
        }
        out
    }

    /// キーボードの配列（xkb の配列名と今の位置）。キーボードが無ければ `None`。
    fn ipc_keyboard(&mut self) -> Option<KeyboardLayouts> {
        let keyboard = self.seat.get_keyboard()?;
        keyboard.with_xkb_state(self, |ctx| {
            let xkb = ctx.xkb().lock().ok()?;
            let names: Vec<String> = xkb
                .layouts()
                .map(|l| xkb.layout_name(l).to_owned())
                .collect();
            Some(KeyboardLayouts {
                names,
                current_idx: u8::try_from(xkb.active_layout().0).unwrap_or(0),
            })
        })
    }

    /// 窓の一覧（id 昇順）と、フォーカス中の窓。`focus_times` に、フォーカスを得た時刻を覚える。
    fn ipc_window_list(&mut self) -> (Vec<Window>, Option<u64>) {
        let view = self.core.view();
        let focus = view.focus().map(|id| id.raw());
        let now = Clock::<Monotonic>::new().now();
        let now = Timestamp {
            secs: std::time::Duration::from(now).as_secs(),
            nanos: std::time::Duration::from(now).subsec_nanos(),
        };
        if let Some(id) = focus {
            self.ipc_focus_times.entry(id).or_insert(now);
        }
        if focus != self.ipc_last_focus {
            if let Some(id) = focus {
                self.ipc_focus_times.insert(id, now);
            }
            self.ipc_last_focus = focus;
        }
        let view = self.core.view();
        let mut tiled: Vec<(u64, u64, i32, i32)> = Vec::new();
        for (key, _) in &self.shell.windows {
            if let Some(id) = view.window_id(*key)
                && let Some(w) = view.window(id)
                && w.state == WindowState::Tiled
                && let Some(r) = w.rect
            {
                tiled.push((w.workspace_id.raw(), id.raw(), r.x(), r.y()));
            }
        }
        let mut out = Vec::new();
        for (key, window) in &self.shell.windows {
            let Some(id) = view.window_id(*key) else {
                continue;
            };
            let Some(w) = view.window(id) else {
                continue;
            };
            let meta = crate::shell::read_meta(window);
            let pid = window
                .toplevel()
                .and_then(|t| self.display_handle.get_client(t.wl_surface().id()).ok())
                .and_then(|c| c.get_credentials(&self.display_handle).ok())
                .map(|c| c.pid);
            let output = self.output_of_window(id);
            let origin = output
                .as_ref()
                .and_then(|o| self.space.output_geometry(o))
                .map(|g| g.loc)
                .unwrap_or_default();
            let (tile, frame) = match (w.rect, output.as_ref()) {
                (Some(t), o) => (Some(t), Some(self.frame_of(t, w.state, o))),
                _ => (None, None),
            };
            // 列と、列の中の位置（1 始まり）。core は列の情報を出さないので、タイルの矩形から数える。
            let pos = (w.state == WindowState::Tiled).then(|| {
                let mut xs: Vec<i32> = tiled
                    .iter()
                    .filter(|t| t.0 == w.workspace_id.raw())
                    .map(|t| t.2)
                    .collect();
                xs.sort_unstable();
                xs.dedup();
                let x = tile.map_or(0, |t| t.x());
                let col = xs.iter().position(|v| *v == x).map_or(1, |p| p + 1);
                let mut ys: Vec<(i32, u64)> = tiled
                    .iter()
                    .filter(|t| t.0 == w.workspace_id.raw() && t.2 == x)
                    .map(|t| (t.3, t.1))
                    .collect();
                ys.sort_unstable();
                let row = ys.iter().position(|v| v.1 == id.raw()).map_or(1, |p| p + 1);
                (col, row)
            });
            let layout = WindowLayout {
                pos_in_scrolling_layout: pos,
                tile_size: tile.map_or((0.0, 0.0), |t| {
                    (f64::from(t.width()), f64::from(t.height()))
                }),
                window_size: frame.map_or((0, 0), |f| (f.width(), f.height())),
                tile_pos_in_workspace_view: tile
                    .map(|t| (f64::from(t.x() - origin.x), f64::from(t.y() - origin.y))),
                window_offset_in_tile: match (tile, frame) {
                    (Some(t), Some(f)) => (f64::from(f.x() - t.x()), f64::from(f.y() - t.y())),
                    _ => (0.0, 0.0),
                },
            };
            out.push(Window {
                id: id.raw(),
                title: meta.title,
                app_id: meta.app_id,
                pid,
                workspace_id: Some(w.workspace_id.raw()),
                is_focused: focus == Some(id.raw()),
                is_floating: w.state == WindowState::Floating,
                is_urgent: false,
                is_fullscreen: w.state == WindowState::Fullscreen,
                is_minimized: false,
                layout,
                focus_timestamp: self.ipc_focus_times.get(&id.raw()).copied(),
            });
        }
        out.sort_by_key(|w| w.id);
        self.ipc_focus_times
            .retain(|id, _| out.iter().any(|w| w.id == *id));
        (out, focus)
    }

    /// 窓・キーボードの配列・オーバービューの状態。
    pub(crate) fn ipc_extra_state(&mut self) -> Extra {
        let (windows, focus) = self.ipc_window_list();
        Extra {
            windows,
            focus,
            keyboard: self.ipc_keyboard(),
            overview: self.overview.open,
        }
    }
}
