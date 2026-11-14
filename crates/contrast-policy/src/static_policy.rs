//! 静的な既定ポリシー。
//!
//! `default_placement`（フックを使わないときの配置）と、組み込みの `Settings` だけで答える `StaticPolicy`。
//! 配置は `Rect::split_columns` による横一列。

use std::rc::Rc;

use contrast_core::{Rect, WindowMeta, WindowState};

use crate::port::{Action, Mods, Policy, WindowPlacement};
use crate::store::{Config, ConfigStore};

/// フックを使わないときの配置: ダイアログは Floating、他は Tiled、
/// 配置先はフォーカス出力の表示ワークスペース、フォーカスを与える。contrast-core の `DefaultPolicy` と同じ。
pub fn default_placement(meta: &WindowMeta) -> WindowPlacement {
    let state = if meta.is_dialog {
        WindowState::Floating
    } else {
        WindowState::Tiled
    };
    WindowPlacement {
        workspace: None,
        state,
        focus: true,
        fullscreen: false,
        column_width: None,
    }
}

/// 組み込みの既定値だけで答えるポリシー（設定ファイルを使わないとき・テスト用）。
pub struct StaticPolicy {
    /// 組み込みの設定を持つストア。
    store: Rc<ConfigStore>,
}

impl StaticPolicy {
    /// 端末のプログラム名を指定して作る。Alt+Return で起動する。
    pub fn new(terminal: impl Into<String>) -> Self {
        let settings = crate::settings::Settings::builtin_with_terminal(terminal);
        Self {
            store: Rc::new(ConfigStore::new(Config::new(settings, None))),
        }
    }
}

impl Policy for StaticPolicy {
    fn appearance(&self) -> crate::Appearance {
        self.store.appearance()
    }

    fn layout_settings(&self) -> crate::LayoutSettings {
        self.store.layout_settings()
    }

    fn screenshot_path(&self) -> Option<String> {
        self.store.screenshot_path()
    }

    fn hotkey_overlay_settings(&self) -> crate::HotkeyOverlaySettings {
        self.store.hotkey_overlay_settings()
    }

    fn cursor_settings(&self) -> crate::CursorSettings {
        self.store.cursor_settings()
    }

    fn input_settings(&self) -> crate::InputSettings {
        self.store.input_settings()
    }

    fn startup_commands(&self) -> Vec<crate::StartupCommand> {
        self.store.startup_commands()
    }

    fn data_control_clients(&self) -> Vec<String> {
        self.store.data_control_clients()
    }

    fn environment(&self) -> Vec<(String, String)> {
        self.store.environment()
    }

    fn background(&self) -> [f32; 4] {
        self.store.background()
    }

    fn border_width(&self) -> i32 {
        self.store.border_width()
    }

    fn focus_border_color(&self) -> Option<[f32; 4]> {
        self.store.focus_border_color()
    }

    fn unfocused_border_color(&self) -> Option<[f32; 4]> {
        self.store.unfocused_border_color()
    }

    fn tiers(&self) -> crate::tier::Tiers {
        self.store.tiers()
    }

    fn workspace_count(&self) -> u8 {
        self.store.workspace_count()
    }

    fn workspace_mode(&self) -> contrast_core::WorkspaceMode {
        self.store.workspace_mode()
    }

    fn layout(&self, area: Rect, count: usize) -> Vec<Rect> {
        self.store.layout(area, count)
    }

    fn place_window(&self, meta: &WindowMeta, focused_workspace: Option<u8>) -> WindowPlacement {
        self.store.place_window(meta, focused_workspace)
    }

    fn resolve_key(&self, mods: Mods, keysym: &str) -> Option<Action> {
        self.store.resolve_key(mods, keysym)
    }

    fn key_cooldown_ms(&self, mods: Mods, keysym: &str) -> Option<u32> {
        self.store.key_cooldown_ms(mods, keysym)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alt(shift: bool) -> Mods {
        Mods {
            alt: true,
            shift,
            ..Mods::default()
        }
    }

    #[test]
    fn keybindings() {
        let p = StaticPolicy::new("foot");
        assert_eq!(
            p.resolve_key(alt(false), "Return"),
            Some(Action::Spawn("foot".into()))
        );
        assert_eq!(p.resolve_key(alt(false), "q"), Some(Action::CloseFocused));
        assert_eq!(p.resolve_key(alt(true), "q"), Some(Action::Quit));
        for (key, ws) in [("1", 0), ("2", 1), ("3", 2), ("4", 3)] {
            assert_eq!(
                p.resolve_key(alt(false), key),
                Some(Action::SwitchWorkspace(ws))
            );
            assert!(ws < p.workspace_count());
        }
        assert_eq!(p.resolve_key(alt(false), "5"), None);
        assert_eq!(p.resolve_key(alt(true), "Return"), None);
        assert_eq!(p.resolve_key(Mods::default(), "q"), None);
    }

    #[test]
    fn layout_fills_area() -> Result<(), &'static str> {
        let p = StaticPolicy::new("foot");
        let area = Rect::new(0, 0, 1001, 600).ok_or("rect")?;
        let rects = p.layout(area, 3);
        assert_eq!(rects.len(), 3);
        assert_eq!(rects.iter().map(Rect::width).sum::<i32>(), 1001);
        assert!(p.layout(area, 0).is_empty());
        Ok(())
    }
}
