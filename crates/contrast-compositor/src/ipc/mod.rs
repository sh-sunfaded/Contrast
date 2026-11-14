//! niri 互換の IPC（ironbar の workspaces 用の最小実装）。計画は `docs/ipc-plan.md`。
//!
//! - `protocol`: JSON（純粋）。
//! - `state`: `Contrast` の状態から作るワークスペースの一覧と、その差分（純粋）。
//! - `server`: calloop の Unix ソケットのサーバー。

mod info;
pub(crate) mod protocol;
mod server;
pub(crate) mod state;

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

    fn ipc_focus_name(&mut self, name: &str) -> Option<bool> {
        if self.policy.named_workspaces().is_empty() {
            return None;
        }
        Some(self.switch_workspace_by_name(name))
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
        if self.ipc.is_none() && self.recorder.is_none() {
            return;
        }
        // 状態は 1 回だけ作り、IPC と記録の両方に渡す（記録のために新しい差分の計算や状態の読み出しを足さない）。
        let extra = self.ipc_extra_state();
        let workspaces = self.ipc_workspaces();
        let stamp = self.record_stamp();
        if let Some(recorder) = &mut self.recorder {
            recorder.record_state(&workspaces, &extra, stamp);
        }
        if let Some(server) = &self.ipc {
            server.publish(workspaces, extra);
        }
    }

    /// 記録の時刻（起動からの単調な経過と壁時計）。時計はここだけが読む。
    pub(crate) fn record_stamp(&self) -> crate::record::Stamp {
        let wall = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        crate::record::Stamp {
            mono_ms: u64::try_from(self.start_time.elapsed().as_millis()).unwrap_or(u64::MAX),
            wall_ms: u64::try_from(wall.as_millis()).unwrap_or(u64::MAX),
        }
    }

    /// 設定を読み込んだことを記録する（`profile` つき。記録が無効なら何もしない）。
    pub(crate) fn record_config_loaded(&mut self, failed: bool) {
        let stamp = self.record_stamp();
        let profile = self.policy.profile();
        if let Some(recorder) = &mut self.recorder {
            recorder.record_config(&profile, failed, stamp);
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
