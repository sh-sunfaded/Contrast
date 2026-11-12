//! xdg-shell: トップレベルは管理して並べ、ポップアップは親に合わせて置く。

use smithay::{
    delegate_xdg_shell,
    desktop::{
        PopupKind, PopupManager, Window, find_popup_root_surface, get_popup_toplevel_coords,
    },
    reexports::wayland_server::protocol::{wl_seat, wl_surface::WlSurface},
    utils::Serial,
    wayland::shell::xdg::{
        PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
    },
};

use crate::state::Contrast;

impl XdgShellHandler for Contrast {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        self.window_created(Window::new_wayland_window(surface));
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        self.window_unmapped(surface.wl_surface());
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        self.unconstrain_popup(&surface);
        let _ = self.popups.track_popup(PopupKind::Xdg(surface));
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        surface.with_pending_state(|state| {
            state.geometry = positioner.get_geometry();
            state.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {}
}

delegate_xdg_shell!(Contrast);

/// commit 時の処理: 保留中の窓の map、初回 configure の送信、ポップアップの追跡。
pub fn handle_commit(state: &mut Contrast, surface: &WlSurface) {
    // 最初の commit なら core に WindowMapped を送る。Configure の大きさは初回 configure に乗る。
    state.map_pending(surface);
    if let Some(window) = state.window_of_surface(surface) {
        window.on_commit();
        let toplevel = window.toplevel().expect("wayland toplevel");
        if !toplevel.is_initial_configure_sent() {
            toplevel.send_configure();
        }
    }
    handle_popup_commit(&mut state.popups, surface);
}

/// ポップアップの commit。初回 configure を送る。
fn handle_popup_commit(popups: &mut PopupManager, surface: &WlSurface) {
    popups.commit(surface);
    if let Some(PopupKind::Xdg(xdg)) = popups.find_popup(surface)
        && !xdg.is_initial_configure_sent()
    {
        xdg.send_configure().expect("initial configure failed");
    }
}

impl Contrast {
    /// ポップアップを出力内に収める（smallvil と同じ方法）。
    fn unconstrain_popup(&self, popup: &PopupSurface) {
        let Ok(root) = find_popup_root_surface(&PopupKind::Xdg(popup.clone())) else {
            return;
        };
        let Some(window) = self.window_of_surface(&root) else {
            return;
        };
        // 窓を表示している出力（なければアクティブな出力）の中に収める。
        let output = self
            .key_of_surface(&root)
            .and_then(|k| self.core.view().window_id(k))
            .and_then(|id| self.output_of_window(id))
            .or_else(|| self.active_output());
        let Some(output) = output else {
            return;
        };
        let Some(output_geo) = self.space.output_geometry(&output) else {
            return;
        };
        let Some(window_geo) = self.space.element_geometry(&window) else {
            return;
        };
        let mut target = output_geo;
        target.loc -= get_popup_toplevel_coords(&PopupKind::Xdg(popup.clone()));
        target.loc -= window_geo.loc;
        popup.with_pending_state(|state| {
            state.geometry = state.positioner.get_unconstrained_geometry(target);
        });
    }
}
