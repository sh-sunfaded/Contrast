//! 意味操作 `Action` の実行。
//!
//! 未使用（どこからも `mod` されていない）。input/bindings.rs と shell.rs に置き換えた。削除は人間が判断する。

use contrast_policy::Action;

use crate::state::Contrast;

impl Contrast {
    /// ポリシーが返した意味操作を実行する。
    pub fn run_action(&mut self, action: Action) {
        match action {
            Action::Spawn(program) => self.spawn(&program),
            Action::CloseFocused => {
                // フォーカス窓が無ければ何もしない。窓が実際に閉じたときの並べ直しと
                // フォーカス移動は toplevel_destroyed → `remove_window` が行う。
                if let Some(index) = self.focused_index() {
                    let m = &self.windows[index];
                    tracing::info!("close focused window={}", m.id);
                    m.window.toplevel().expect("wayland toplevel").send_close();
                }
            }
            Action::FocusPrev => self.focus_step(-1),
            Action::FocusNext => self.focus_step(1),
            Action::SwitchWorkspace(n) => self.switch_workspace(n),
            Action::Quit => {
                tracing::info!("quit requested");
                self.loop_signal.stop();
            }
        }
    }

    /// `program` をこのコンポジタのソケットにつないで起動する。
    fn spawn(&self, program: &str) {
        match std::process::Command::new(program)
            .env("WAYLAND_DISPLAY", &self.socket_name)
            .spawn()
        {
            Ok(mut child) => {
                tracing::info!(program, pid = child.id(), "spawned");
                // ゾンビを残さないよう、別スレッドで終了を待つ。
                std::thread::spawn(move || child.wait());
            }
            Err(err) => tracing::warn!(program, %err, "spawn failed"),
        }
    }

    /// 表示中の窓の並びで、フォーカスを `delta` だけ（端で折り返して）動かす。
    fn focus_step(&mut self, delta: isize) {
        let visible = self.visible_windows();
        if visible.is_empty() {
            return;
        }
        let current = self
            .focused_index()
            .and_then(|i| visible.iter().position(|w| *w == self.windows[i].window));
        let len = visible.len() as isize;
        let next = match current {
            Some(i) => (i as isize + delta).rem_euclid(len) as usize,
            None => 0,
        };
        self.focus_window(Some(&visible[next]));
    }

    /// 表示するワークスペースを切り替え、並べ直して、そこで最後にフォーカスした窓へ移る。
    fn switch_workspace(&mut self, n: u8) {
        if n >= self.policy.workspace_count() || n == self.active_ws {
            return;
        }
        tracing::info!(from = self.active_ws + 1, to = n + 1, "switch workspace");
        self.active_ws = n;
        self.relayout();
        let target = self.most_recent_visible();
        self.focus_window(target.as_ref());
    }
}
