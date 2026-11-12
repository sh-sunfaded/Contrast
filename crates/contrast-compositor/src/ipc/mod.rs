//! niri 互換の IPC（ironbar の workspaces 用の最小実装）。計画は `docs/ipc-plan.md`。
//!
//! - `protocol`: JSON（純粋）。
//! - `state`: `Contrast` の状態から作るワークスペースの一覧と、その差分（純粋）。
//! - `server`: calloop の Unix ソケットのサーバー。

mod info;
mod protocol;
mod server;
mod state;

use std::path::PathBuf;

use smithay::reexports::calloop::LoopHandle;

use contrast_core::WorkspaceIndex;

pub(crate) use protocol::Timestamp;
use server::Host;
pub(crate) use server::IpcServer;

use crate::state::Contrast;

impl Host for Contrast {
    fn ipc_workspaces(&self) -> Vec<protocol::Workspace> {
        Contrast::ipc_workspaces(self)
    }

    fn ipc_focus(&mut self, index: u8) {
        self.core_action(contrast_core::Action::SwitchWorkspace(WorkspaceIndex::new(
            index,
        )));
    }

    fn ipc_extra(&mut self) -> state::Extra {
        self.ipc_extra_state()
    }

    fn ipc_outputs(&self) -> Vec<protocol::Output> {
        self.ipc_output_list()
    }

    fn ipc_focused_output(&self) -> Option<String> {
        self.active_output().map(|o| o.name())
    }

    fn ipc_layers(&self) -> Vec<protocol::LayerSurface> {
        self.ipc_layer_list()
    }

    fn ipc_version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_owned()
    }
}

impl Contrast {
    /// IPC のソケットを作る（`$XDG_RUNTIME_DIR` が無い・同じパスがある、などは、エラーのログを出して IPC なしで続ける）。
    /// `Startup` の `ipc="niri"` の子が使うので、`run_startup` より前に呼ぶ。
    pub fn start_ipc(&mut self, handle: &LoopHandle<'static, Contrast>) {
        let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) else {
            tracing::error!("ipc: XDG_RUNTIME_DIR is not set, the IPC is disabled");
            return;
        };
        let extra = self.ipc_extra_state();
        match IpcServer::start(handle, &dir, self.ipc_workspaces(), extra) {
            Ok(server) => {
                tracing::info!("ipc: socket={}", server.path().display());
                self.ipc_socket = Some(server.path().to_owned());
                self.ipc = Some(server);
            }
            Err(err) => {
                tracing::error!("ipc: cannot create the socket in {}: {err}", dir.display())
            }
        }
    }

    /// 状態が変わったかもしれないとき（core のコマンドのあと）に呼ぶ: ワークスペースの変化を購読者に送る。
    pub(crate) fn ipc_sync(&mut self) {
        if self.ipc.is_none() {
            return;
        }
        let extra = self.ipc_extra_state();
        let workspaces = self.ipc_workspaces();
        if let Some(server) = &self.ipc {
            server.publish(workspaces, extra);
        }
    }

    /// 状態の差分でないイベント（`ConfigLoaded`・`ScreenshotCaptured`）を購読者に送る。
    pub(crate) fn ipc_event_config_loaded(&self, failed: bool) {
        if let Some(server) = &self.ipc {
            server.publish_event(&protocol::Event::ConfigLoaded { failed });
        }
    }

    /// スクリーンショットを撮ったことを購読者に送る。`path` は保存したパス。
    pub(crate) fn ipc_event_screenshot(&self, path: Option<String>) {
        if let Some(server) = &self.ipc {
            server.publish_event(&protocol::Event::ScreenshotCaptured { path });
        }
    }
}
