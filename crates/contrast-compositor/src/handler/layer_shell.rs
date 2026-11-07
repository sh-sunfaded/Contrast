//! wlr-layer-shell: レイヤー面（背景・バー・ランチャーなど）を出力ごとの `LayerMap` に置く。
//!
//! - 位置とサイズ（anchor・margin・要求サイズ）は smithay の `LayerMap::arrange` が計算し、configure を送る。
//! - 描画順は `Space` の描画が決める（background < bottom < 窓 < top < overlay）。
//! - exclusive な面はキーボードフォーカスを受け取り、消えたら core が持つフォーカス窓に戻す。
//!   フォーカスは core の Command には流さない（core は窓だけを扱う）。
//! - 排他領域（exclusive_zone）は計算してログに出すだけで、窓の配置には反映しない。

use smithay::{
    delegate_layer_shell,
    desktop::{LayerSurface as DesktopLayer, WindowSurfaceType, layer_map_for_output},
    output::Output,
    reexports::wayland_server::protocol::{wl_output::WlOutput, wl_surface::WlSurface},
    wayland::shell::wlr_layer::{
        KeyboardInteractivity, Layer, LayerSurface, WlrLayerShellHandler, WlrLayerShellState,
    },
};

use crate::state::Contrast;

impl WlrLayerShellHandler for Contrast {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(
        &mut self,
        surface: LayerSurface,
        output: Option<WlOutput>,
        layer: Layer,
        namespace: String,
    ) {
        let output = output
            .as_ref()
            .and_then(Output::from_resource)
            .or_else(|| self.space.outputs().next().cloned());
        let Some(output) = output else {
            tracing::warn!(namespace, "layer surface without an output, closing");
            surface.send_close();
            return;
        };
        tracing::info!(
            namespace,
            ?layer,
            output = output.name(),
            "layer surface created"
        );
        let desktop = DesktopLayer::new(surface, namespace);
        if let Err(err) = layer_map_for_output(&output).map_layer(&desktop) {
            tracing::warn!(?err, "map_layer failed");
        }
    }

    fn layer_destroyed(&mut self, surface: LayerSurface) {
        let wl = surface.wl_surface().clone();
        for output in self.space.outputs().cloned().collect::<Vec<_>>() {
            let mut map = layer_map_for_output(&output);
            let found = map
                .layer_for_surface(&wl, WindowSurfaceType::TOPLEVEL)
                .cloned();
            if let Some(layer) = found {
                map.unmap_layer(&layer);
                tracing::info!(
                    namespace = layer.namespace(),
                    zone = ?map.non_exclusive_zone(),
                    "layer surface destroyed"
                );
            }
        }
        // 排他ゾーンが消えたぶん、窓の領域を広げる。
        self.sync_output_insets();
        if self.layer_focus.as_ref() == Some(&wl) {
            self.layer_focus = None;
            self.restore_window_focus();
        }
    }
}

delegate_layer_shell!(Contrast);

/// commit 時の処理: レイヤー面なら再配置し、初回 configure を送り、exclusive ならフォーカスを与える。
pub fn handle_commit(state: &mut Contrast, surface: &WlSurface) {
    let Some(output) = state
        .space
        .outputs()
        .find(|o| {
            layer_map_for_output(o)
                .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
                .is_some()
        })
        .cloned()
    else {
        return;
    };
    let mut map = layer_map_for_output(&output);
    map.arrange();
    // 排他ゾーンが変わったかもしれない。窓の領域を取り直す（`map` を手放してから）。
    drop(map);
    state.sync_output_insets();
    let map = layer_map_for_output(&output);
    let Some(layer) = map
        .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
        .cloned()
    else {
        return;
    };
    let first = !smithay::wayland::compositor::with_states(surface, |states| {
        states
            .data_map
            .get::<smithay::wayland::shell::wlr_layer::LayerSurfaceData>()
            .map(|d| d.lock().unwrap().initial_configure_sent)
            .unwrap_or(false)
    });
    if first {
        layer.layer_surface().send_configure();
        let cached = layer.cached_state();
        tracing::info!(
            namespace = layer.namespace(),
            layer = ?layer.layer(),
            requested = ?cached.size,
            anchor = ?cached.anchor,
            margin = ?cached.margin,
            exclusive_zone = ?cached.exclusive_zone,
            interactivity = ?cached.keyboard_interactivity,
            geometry = ?map.layer_geometry(&layer),
            non_exclusive_zone = ?map.non_exclusive_zone(),
            "layer surface configure"
        );
        if cached.keyboard_interactivity == KeyboardInteractivity::Exclusive {
            drop(map);
            state.focus_layer(&layer);
        }
    }
}

impl Contrast {
    /// レイヤー面にキーボードフォーカスを与える（core には流さない）。
    pub fn focus_layer(&mut self, layer: &DesktopLayer) {
        let surface = layer.wl_surface().clone();
        tracing::info!(
            namespace = layer.namespace(),
            "keyboard focus -> layer surface"
        );
        self.layer_focus = Some(surface.clone());
        let keyboard = self.seat.get_keyboard().expect("seat has keyboard");
        keyboard.set_focus(
            self,
            Some(surface),
            smithay::utils::SERIAL_COUNTER.next_serial(),
        );
    }

    /// クリックされた位置のレイヤー面がキーボードを受け取れるならフォーカスを与える。
    /// 位置にレイヤー面があれば true（下の窓へはフォーカスを渡さない）。
    pub fn click_layer(
        &mut self,
        pos: smithay::utils::Point<f64, smithay::utils::Logical>,
    ) -> bool {
        let Some(output) = self.space.outputs().next().cloned() else {
            return false;
        };
        let hit = {
            let map = layer_map_for_output(&output);
            [Layer::Overlay, Layer::Top]
                .into_iter()
                .find_map(|l| map.layer_under(l, pos).cloned())
        };
        let Some(layer) = hit else {
            return false;
        };
        if layer.can_receive_keyboard_focus() {
            self.focus_layer(&layer);
        }
        true
    }
}
