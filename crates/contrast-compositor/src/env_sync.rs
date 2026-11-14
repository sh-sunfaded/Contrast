//! `Env`（読み取り専用の入力。`docs/ui-spec.md` §4.3）の組み立てと、設定への反映。キー層の遷移。名前つきワークスペースとシーン。
//!
//! `Env` は `title` を持たない（変化が頻繁なため）。`Env` が前回と等しければ、ポリシーは再 `render` しない
//! （`Policy::update_env`）。`Env` で変わるのは `Appearance`・`Motion`・`Keybind` だけ（`Layout`・`Workspaces` は変わらない）。

use contrast_core::{WindowState, WorkspaceIndex};
use contrast_policy::{EnvUpdate, FocusEnv, FocusState, NORMAL_MODE, PolicyEnv, WorkspaceEnv};

use crate::input::layer::{KeyRoute, route_key};
use crate::state::Contrast;

/// 窓の状態を `Env.focus.state` にする。
fn focus_state(state: WindowState) -> FocusState {
    match state {
        WindowState::Floating => FocusState::Floating,
        WindowState::Fullscreen => FocusState::Fullscreen,
        _ => FocusState::Tiled,
    }
}

/// 名前表（位置順）の `index`（0 始まり）番目の名前。
pub(crate) fn name_at(names: &[contrast_policy::NamedWorkspace], index: u8) -> Option<String> {
    names.get(usize::from(index)).map(|w| w.name.clone())
}

/// 名前 `name` のワークスペースの位置（0 始まり）と、そのシーン（`Launch` のコマンド）。名前表に無ければ `None`。
pub(crate) fn scene_of(
    names: &[contrast_policy::NamedWorkspace],
    name: &str,
) -> Option<(u8, Vec<String>)> {
    let index = names.iter().position(|w| w.name == name)?;
    let scene = names.get(index)?;
    Some((u8::try_from(index).ok()?, scene.launch.clone()))
}

impl Contrast {
    /// 押されたキーの行き先（今のキー層で）。実機の入力とデバッグの入力の両方がこれを使う（経路を 1 つにする）。
    pub(crate) fn route_key(&self, mods: contrast_policy::Mods, name: &str) -> KeyRoute {
        route_key(self.policy.as_ref(), &self.key_mode, mods, name)
    }

    /// キー層を `mode` にする（`normal` に戻すのも同じ）。`Env.mode` が変わるので、設定を作り直す。
    pub(crate) fn set_key_mode(&mut self, mode: &str) {
        if self.key_mode == mode {
            return;
        }
        tracing::info!(from = %self.key_mode, to = mode, "key mode");
        self.key_mode = mode.to_owned();
        self.sync_env();
    }

    /// いまの状態から `Env` を作る。
    pub(crate) fn build_env(&self) -> PolicyEnv {
        let view = self.core.view();
        let output = view.active_output();
        let output_name = output
            .and_then(|o| self.output_of_id(o))
            .map(|o| o.name())
            .unwrap_or_default();
        let shown = output.and_then(|o| view.shown_workspace(o));
        let window_count = shown
            .and_then(|i| view.workspace_windows(i))
            .map_or(0, <[_]>::len);
        let names = self.policy.named_workspaces();
        let focus = view.focus().and_then(|id| {
            let state = view.window(id)?.state;
            let key = view.window_key(id)?;
            let window = self.shell.windows.get(&key)?;
            let meta = crate::shell::read_meta(window);
            Some(FocusEnv {
                app_id: meta.app_id.unwrap_or_default(),
                is_dialog: meta.is_dialog,
                state: focus_state(state),
            })
        });
        PolicyEnv {
            profile: self.policy.profile(),
            mode: self.key_mode.clone(),
            output_name,
            workspace: WorkspaceEnv {
                index: shown.map_or(1, |i| u32::from(i.get()).saturating_add(1)),
                name: shown.and_then(|i| name_at(&names, i.get())),
                window_count: u32::try_from(window_count).unwrap_or(u32::MAX),
            },
            focus,
        }
    }

    /// `Env` が変わったことをポリシーに伝える（等しければポリシーは何もしない）。反映されたら、ホットキーのオーバーレイを
    /// 描き直し、枠の太さが変わっていれば配置に反映する。失敗は警告（同じ `Env` では 1 回だけ）。
    pub(crate) fn sync_env(&mut self) {
        let border = self.policy.border_width();
        match self.policy.update_env(self.build_env()) {
            EnvUpdate::Unchanged => {}
            EnvUpdate::Applied(notes) => {
                for note in notes {
                    if self.env_logged.insert(note.message.clone()) {
                        tracing::warn!("{note}");
                    }
                }
                self.hotkey_overlay.invalidate();
                if self.policy.border_width() != border {
                    self.reapply_layout();
                }
            }
            EnvUpdate::Failed(d) => tracing::warn!("{d}"),
        }
    }

    /// 名前つきワークスペースへ切り替える（名前表 → 位置 → core の `SwitchWorkspace`）。未知の名前は何もしない。
    pub(crate) fn switch_workspace_by_name(&mut self, name: &str) -> bool {
        let names = self.policy.named_workspaces();
        let Some((index, _)) = scene_of(&names, name) else {
            tracing::warn!(name, "CT084: unknown workspace name");
            return false;
        };
        self.core_action(contrast_core::Action::SwitchWorkspace(WorkspaceIndex::new(
            index,
        )));
        true
    }

    /// シーンを開く: 名前つきワークスペースへ切り替えてから、その `Launch` を spawn する（手動の命令。自動起動はしない）。
    /// 未知のシーンは何もせず警告（CT085）。
    pub(crate) fn open_scene(&mut self, name: &str) {
        let names = self.policy.named_workspaces();
        let Some((_, launch)) = scene_of(&names, name) else {
            tracing::warn!(name, "CT085: unknown scene");
            return;
        };
        if !self.switch_workspace_by_name(name) {
            return;
        }
        tracing::info!(name, commands = launch.len(), "scene opened");
        for command in launch {
            self.spawn_command(&command, "scene", false);
        }
    }

    /// 設定を読み直したあと: キー層を `normal` に戻し、警告の重複抑止を忘れ、`Env` を作り直す。
    pub(crate) fn reset_env_after_reload(&mut self) {
        self.key_mode = NORMAL_MODE.to_owned();
        self.env_logged.clear();
        self.sync_env();
    }
}

#[cfg(test)]
mod tests {
    use contrast_policy::NamedWorkspace;

    use super::*;

    #[test]
    fn names_are_looked_up_by_position() {
        let names = vec![
            NamedWorkspace {
                name: "web".to_owned(),
                launch: Vec::new(),
            },
            NamedWorkspace {
                name: "code".to_owned(),
                launch: Vec::new(),
            },
        ];
        assert_eq!(name_at(&names, 0).as_deref(), Some("web"));
        assert_eq!(name_at(&names, 1).as_deref(), Some("code"));
        assert_eq!(name_at(&names, 2), None);
        assert_eq!(name_at(&[], 0), None);
    }

    #[test]
    fn a_scene_is_the_named_workspace_position_and_its_launch_commands() {
        let names = vec![
            NamedWorkspace {
                name: "web".to_owned(),
                launch: vec!["firefox".to_owned()],
            },
            NamedWorkspace {
                name: "code".to_owned(),
                launch: vec!["foot".to_owned(), "foot -e htop".to_owned()],
            },
        ];
        assert_eq!(
            scene_of(&names, "code"),
            Some((1, vec!["foot".to_owned(), "foot -e htop".to_owned()]))
        );
        assert_eq!(
            scene_of(&names, "web"),
            Some((0, vec!["firefox".to_owned()]))
        );
        // 未知の名前は何も起こさない。
        assert_eq!(scene_of(&names, "nope"), None);
        assert_eq!(scene_of(&[], "web"), None);
    }

    #[test]
    fn window_states_map_to_the_env_states() {
        assert_eq!(focus_state(WindowState::Tiled), FocusState::Tiled);
        assert_eq!(focus_state(WindowState::Floating), FocusState::Floating);
        assert_eq!(focus_state(WindowState::Fullscreen), FocusState::Fullscreen);
    }
}
