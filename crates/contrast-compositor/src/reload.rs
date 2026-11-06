//! 設定の再読み込み（`reload-config` アクションと SIGUSR1）。
//!
//! 新しい版の読み込み・変換が成功したときだけ `ConfigStore` を差し替え、世代を進める。
//! 失敗したら旧版を保持し、診断をログに出す。差し替えたあと、枠の太さの変更を窓の配置に反映する。

use contrast_policy::ReloadOutcome;

use crate::state::Contrast;

impl Contrast {
    /// 設定ファイルを読み直す。
    pub fn reload_config(&mut self) {
        let generation = self.store.generation();
        let Some(path) = self.config_path.clone() else {
            tracing::warn!("config: reload ignored: no config path (generation={generation})");
            return;
        };
        match self.store.reload(self.config_loader.as_ref(), &path) {
            ReloadOutcome::Applied {
                generation,
                diagnostics,
            } => {
                for d in &diagnostics {
                    tracing::warn!("{d}");
                }
                tracing::info!(
                    "config: reloaded path={} generation={} bindings={}",
                    path.display(),
                    generation,
                    self.store.binding_count()
                );
                let mode = self.policy.layout_settings().mode;
                if mode != self.layout_mode {
                    tracing::warn!(
                        ?mode,
                        running = ?self.layout_mode,
                        "config: Layout mode changed, restart to apply (the running layout is kept)"
                    );
                }
                self.refresh_data_control_clients();
                self.reapply_layout();
                self.reapply_input_settings();
            }
            ReloadOutcome::Absent => tracing::warn!(
                "config: reload failed: {} not found, keeping generation={generation}",
                path.display()
            ),
            ReloadOutcome::Failed(diagnostics) => {
                tracing::error!(
                    "config: reload failed: {} errors, keeping generation={generation}",
                    diagnostics.len()
                );
                for d in &diagnostics {
                    tracing::error!("{d}");
                }
            }
        }
    }
}
