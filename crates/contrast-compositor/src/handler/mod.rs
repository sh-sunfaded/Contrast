//! Wayland プロトコルのハンドラ実装（compositor / shm / seat / data device / output / xdg-shell。xdg-decoration は公開しない）。

mod decoration;
mod layer_shell;
mod xdg_shell;

pub use decoration::fmt_id;

use smithay::{
    backend::{
        allocator::{Buffer, dmabuf::Dmabuf},
        renderer::utils::on_commit_buffer_handler,
    },
    delegate_compositor, delegate_data_control, delegate_data_device, delegate_dmabuf,
    delegate_input_method_manager, delegate_output, delegate_primary_selection, delegate_seat,
    delegate_shm, delegate_text_input_manager, delegate_virtual_keyboard_manager,
    input::{Seat, SeatHandler, SeatState, pointer::CursorImageStatus},
    reexports::wayland_server::{
        Client, Resource,
        protocol::{wl_buffer, wl_surface::WlSurface},
    },
    wayland::{
        buffer::BufferHandler,
        compositor::{
            CompositorClientState, CompositorHandler, CompositorState, get_parent,
            is_sync_subsurface,
        },
        dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
        output::OutputHandler,
        selection::{
            SelectionHandler,
            data_device::{
                ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
                set_data_device_focus,
            },
            primary_selection::{
                PrimarySelectionHandler, PrimarySelectionState, set_primary_focus,
            },
            wlr_data_control::{DataControlHandler, DataControlState},
        },
        shm::{ShmHandler, ShmState},
    },
};

use crate::state::{ClientState, Contrast};

impl CompositorHandler for Contrast {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client
            .get_data::<ClientState>()
            .expect("client without ClientState")
            .compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if root != *surface
                && let Some(window) = self.window_of_surface(&root)
            {
                window.on_commit();
            }
        }
        xdg_shell::handle_commit(self, surface);
        layer_shell::handle_commit(self, surface);
    }
}

impl BufferHandler for Contrast {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl ShmHandler for Contrast {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl SeatHandler for Contrast {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.cursor_status = image;
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let dh = &self.display_handle;
        let client = focused.and_then(|s| dh.get_client(s.id()).ok());
        set_data_device_focus(dh, seat, client.clone());
        set_primary_focus(dh, seat, client);
        // 窓や別の面にフォーカスが移ったら、レイヤー面のフォーカスは外れている。
        if self.layer_focus.as_ref() != focused {
            self.layer_focus = None;
        }
    }
}

impl SelectionHandler for Contrast {
    /// コンポジタが出す選択（スクリーンショットの PNG）の中身。
    type SelectionUserData = std::sync::Arc<[u8]>;

    /// クライアントがコンポジタの選択を読む: 中身を別スレッドで fd に書く（大きい画像でループを止めない）。
    fn send_selection(
        &mut self,
        _ty: smithay::wayland::selection::SelectionTarget,
        mime_type: String,
        fd: std::os::fd::OwnedFd,
        _seat: smithay::input::Seat<Self>,
        user_data: &Self::SelectionUserData,
    ) {
        if mime_type != "image/png" {
            return;
        }
        let data = user_data.clone();
        let spawned = std::thread::Builder::new()
            .name("selection-send".into())
            .spawn(move || {
                use std::io::Write;
                let mut file = std::fs::File::from(fd);
                if let Err(e) = file.write_all(&data) {
                    tracing::warn!("selection: write failed: {e}");
                }
            });
        if let Err(e) = spawned {
            tracing::warn!("selection: cannot start the writer thread: {e}");
        }
    }
}

impl DataDeviceHandler for Contrast {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl PrimarySelectionHandler for Contrast {
    fn primary_selection_state(&self) -> &PrimarySelectionState {
        &self.primary_selection_state
    }
}

impl DataControlHandler for Contrast {
    fn data_control_state(&self) -> &DataControlState {
        &self.data_control_state
    }
}

impl ClientDndGrabHandler for Contrast {
    /// クライアントのドラッグが始まった（DnD の端スクロールを始める準備）。
    fn started(
        &mut self,
        _source: Option<smithay::reexports::wayland_server::protocol::wl_data_source::WlDataSource>,
        _icon: Option<WlSurface>,
        _seat: smithay::input::Seat<Self>,
    ) {
        self.dnd_started();
    }

    /// ドラッグが終わった（落とした・取りやめた）。
    fn dropped(
        &mut self,
        _target: Option<WlSurface>,
        _validated: bool,
        _seat: smithay::input::Seat<Self>,
    ) {
        self.dnd_ended();
    }
}
impl ServerDndGrabHandler for Contrast {}

impl OutputHandler for Contrast {}

delegate_compositor!(Contrast);
delegate_shm!(Contrast);
delegate_seat!(Contrast);
delegate_data_device!(Contrast);
delegate_primary_selection!(Contrast);
delegate_data_control!(Contrast);
delegate_output!(Contrast);
// 日本語入力（fcitx5）: ハンドラは `ime.rs`（`InputMethodHandler`）。
delegate_text_input_manager!(Contrast);
delegate_input_method_manager!(Contrast);
delegate_virtual_keyboard_manager!(Contrast);

impl DmabufHandler for Contrast {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: Dmabuf,
        notifier: ImportNotifier,
    ) {
        let imported = match self.dmabuf_importer.as_mut() {
            Some(import) => import(&dmabuf),
            None => false,
        };
        if imported {
            tracing::info!(format = ?dmabuf.format(), size = ?dmabuf.size(), "dmabuf: imported");
            if notifier.successful::<Contrast>().is_err() {
                tracing::debug!("dmabuf: the client is gone");
            }
        } else {
            tracing::warn!(format = ?dmabuf.format(), size = ?dmabuf.size(), "dmabuf: import failed");
            notifier.failed();
        }
    }
}

delegate_dmabuf!(Contrast);
smithay::delegate_viewporter!(Contrast);
smithay::delegate_fractional_scale!(Contrast);
smithay::delegate_pointer_gestures!(Contrast);

impl smithay::wayland::fractional_scale::FractionalScaleHandler for Contrast {
    /// 面が拡大率を問い合わせたら、その面がいる出力（無ければアクティブな出力、それも無ければ最初の出力）の拡大率を伝える。
    fn new_fractional_scale(&mut self, surface: WlSurface) {
        let scale = self
            .active_output()
            .or_else(|| self.shell.outputs.values().next().cloned())
            .map_or(1.0, |o| o.current_scale().fractional_scale());
        smithay::wayland::compositor::with_states(&surface, |states| {
            smithay::wayland::fractional_scale::with_fractional_scale(states, |fs| {
                fs.set_preferred_scale(scale);
            });
        });
    }
}
