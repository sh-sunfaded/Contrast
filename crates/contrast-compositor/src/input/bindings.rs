//! キーバインドの意味操作（`contrast_policy::Action`）の実行。
//!
//! 窓の配置・フォーカスに関わるものは contrast-core の `Action` に翻訳して `Core` に渡す。
//! 端末の起動と終了は core を通さない。

use contrast_core::{Direction, WorkspaceIndex};
use contrast_policy::Action;

use crate::state::Contrast;

impl Contrast {
    /// ポリシーが返した意味操作を実行する。
    pub fn run_action(&mut self, action: Action) {
        let core_action = match action {
            Action::Spawn(program) => return self.spawn(&program),
            Action::Quit => {
                tracing::info!("quit requested");
                return self.loop_signal.stop();
            }
            Action::ReloadConfig => return self.reload_config(),
            Action::ScreenshotScreen => {
                self.screenshot_pending = true;
                return;
            }
            Action::PowerOffMonitors => {
                if !self.monitors.power_off() {
                    tracing::info!("power-off-monitors ignored: not on a session backend");
                }
                return;
            }
            Action::CloseFocused => contrast_core::Action::CloseFocused,
            Action::FocusPrev => contrast_core::Action::FocusDirection(Direction::Left),
            Action::FocusNext => contrast_core::Action::FocusDirection(Direction::Right),
            Action::MoveLeft => contrast_core::Action::MoveWindow(Direction::Left),
            Action::MoveRight => contrast_core::Action::MoveWindow(Direction::Right),
            Action::FocusUp => contrast_core::Action::FocusDirection(Direction::Up),
            Action::FocusDown => contrast_core::Action::FocusDirection(Direction::Down),
            Action::MoveUp => contrast_core::Action::MoveWindow(Direction::Up),
            Action::MoveDown => contrast_core::Action::MoveWindow(Direction::Down),
            Action::FocusColumnFirst => contrast_core::Action::FocusColumnFirst,
            Action::FocusColumnLast => contrast_core::Action::FocusColumnLast,
            Action::MoveColumnFirst => contrast_core::Action::MoveColumnToFirst,
            Action::MoveColumnLast => contrast_core::Action::MoveColumnToLast,
            Action::ConsumeWindowIntoColumn => contrast_core::Action::ConsumeWindowIntoColumn,
            Action::ExpelWindowFromColumn => contrast_core::Action::ExpelWindowFromColumn,
            Action::ConsumeOrExpelWindowLeft => contrast_core::Action::ConsumeOrExpelWindowLeft,
            Action::ConsumeOrExpelWindowRight => contrast_core::Action::ConsumeOrExpelWindowRight,
            Action::SwitchPresetColumnWidth => contrast_core::Action::SwitchPresetColumnWidth,
            Action::MaximizeColumn => contrast_core::Action::MaximizeColumn,
            Action::SetColumnWidth(ratio) => {
                contrast_core::Action::SetColumnWidth(contrast_core::ColumnWidth::Proportion(ratio))
            }
            Action::AdjustColumnWidth(percent) => return self.adjust_column_width(percent),
            Action::CenterColumn => contrast_core::Action::CenterColumn,
            Action::SwitchPresetWindowHeight => contrast_core::Action::SwitchPresetWindowHeight,
            Action::ResetWindowHeight => contrast_core::Action::ResetWindowHeight,
            Action::SetWindowHeight(height) => contrast_core::Action::SetWindowHeight(height),
            Action::AdjustWindowHeight(percent) => return self.adjust_window_height(percent),
            Action::ToggleFloating => contrast_core::Action::ToggleFloating,
            Action::ToggleFullscreen => contrast_core::Action::ToggleFullscreen,
            Action::WorkspaceNext => return self.step_workspace(1, Step::Switch),
            Action::WorkspacePrev => return self.step_workspace(-1, Step::Switch),
            Action::MoveToWorkspaceNext => return self.step_workspace(1, Step::MoveWindow),
            Action::MoveToWorkspacePrev => return self.step_workspace(-1, Step::MoveWindow),
            Action::MoveColumnToWorkspaceNext => return self.step_workspace(1, Step::MoveColumn),
            Action::MoveColumnToWorkspacePrev => return self.step_workspace(-1, Step::MoveColumn),
            Action::MoveColumnToWorkspace(n) => {
                contrast_core::Action::MoveColumnToWorkspace(WorkspaceIndex::new(n))
            }
            Action::FocusMonitor(direction) => {
                return self.monitor_action(contrast_core::Action::FocusMonitor(direction));
            }
            Action::MoveColumnToMonitor(direction) => {
                return self.monitor_action(contrast_core::Action::MoveColumnToMonitor(direction));
            }
            Action::MoveWorkspaceDown => contrast_core::Action::MoveWorkspaceDown,
            Action::MoveWorkspaceUp => contrast_core::Action::MoveWorkspaceUp,
            Action::SwitchWorkspace(n) => {
                contrast_core::Action::SwitchWorkspace(WorkspaceIndex::new(n))
            }
            Action::MoveToWorkspace(n) => {
                contrast_core::Action::MoveToWorkspace(WorkspaceIndex::new(n))
            }
        };
        self.core_action(core_action);
    }

    /// 出力をまたぐ操作。アクティブな出力が変わったら、niri と同じくカーソルをその出力の中央へ動かす（`move_cursor_to_output`）。
    fn monitor_action(&mut self, action: contrast_core::Action) {
        let before = self.active_output();
        self.core_action(action);
        let after = self.active_output();
        if after != before
            && let Some(output) = after
        {
            self.warp_pointer_to(&output);
        }
    }

    /// フォーカス列の幅を `percent` パーセントぶん増減する（core は絶対幅しか受けないので、ここで今の割合から求める）。
    fn adjust_column_width(&mut self, percent: i8) {
        let Some(area) = self.shown_area_width() else {
            return;
        };
        let current = self.focused_window_width();
        let Some(next) = current.and_then(|w| adjusted_percent(w, area, percent)) else {
            return;
        };
        let Some(ratio) = contrast_core::Ratio::new(next, 100) else {
            return;
        };
        self.core_action(contrast_core::Action::SetColumnWidth(
            contrast_core::ColumnWidth::Proportion(ratio),
        ));
    }

    /// フォーカス窓の高さを `percent` パーセントぶん増減する（`adjust_column_width` と同じ。今の高さの割合に足して 1〜100 に収める）。
    fn adjust_window_height(&mut self, percent: i8) {
        let Some(area) = self.shown_area_height() else {
            return;
        };
        let current = self.focused_window_height();
        let Some(next) = current.and_then(|h| adjusted_percent(h, area, percent)) else {
            return;
        };
        let Some(ratio) = contrast_core::Ratio::new(next, 100) else {
            return;
        };
        self.core_action(contrast_core::Action::SetWindowHeight(
            contrast_core::WindowHeight::Proportion(ratio),
        ));
    }

    /// 表示中のワークスペースの出力の作業領域の高さ。
    fn shown_area_height(&self) -> Option<i32> {
        let output = self.active_output()?;
        Some(self.space.output_geometry(&output)?.size.h)
    }

    /// フォーカス窓の矩形の高さ。
    fn focused_window_height(&self) -> Option<i32> {
        let view = self.core.view();
        view.window(view.focus()?)?.rect.map(|r| r.height())
    }

    /// 表示中のワークスペースの出力の作業領域の幅（core が窓を並べる領域の幅）。
    fn shown_area_width(&self) -> Option<i32> {
        let output = self.active_output()?;
        Some(self.space.output_geometry(&output)?.size.w)
    }

    /// フォーカス窓の矩形の幅（列の幅と同じ）。
    fn focused_window_width(&self) -> Option<i32> {
        let view = self.core.view();
        view.window(view.focus()?)?.rect.map(|r| r.width())
    }

    /// アクティブな出力の表示中のワークスペースの、その出力の列の中の番号（0 始まり。core の D73）。出力が無ければ `None`。
    pub(crate) fn shown_workspace_index(&self) -> Option<u8> {
        self.shown_position(&self.active_output()?)
    }

    /// アクティブな出力の列のワークスペースの数（`Fixed` では全体の数）。
    fn active_workspace_count(&self) -> u8 {
        let view = self.core.view();
        view.active_output()
            .map(|o| view.output_workspaces(o).len())
            .and_then(|n| u8::try_from(n).ok())
            .unwrap_or_else(|| view.workspace_count())
    }

    /// 表示中の番号から `delta` だけ離れたワークスペースへ、`step` の操作をする。範囲外は何もしない。
    /// 範囲は core のいまの個数（動的なワークスペースでは増減する）。
    fn step_workspace(&mut self, delta: i8, step: Step) {
        let Some(shown) = self.shown_workspace_index() else {
            return;
        };
        let Some(target) = step_index(shown, delta, self.active_workspace_count()) else {
            tracing::info!(
                shown = shown + 1,
                delta,
                "workspace step: out of range, ignored"
            );
            return;
        };
        let index = WorkspaceIndex::new(target);
        self.core_action(match step {
            Step::Switch => contrast_core::Action::SwitchWorkspace(index),
            Step::MoveWindow => contrast_core::Action::MoveToWorkspace(index),
            Step::MoveColumn => contrast_core::Action::MoveColumnToWorkspace(index),
        });
    }

    /// キーバインドの `spawn`。環境変数などは `spawn_command` が決める。
    fn spawn(&self, command: &str) {
        self.spawn_command(command, "keybind", false);
    }
}

/// 隣のワークスペースに対する操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// 切り替える。
    Switch,
    /// フォーカス窓だけ移す。
    MoveWindow,
    /// フォーカス窓の列ごと移して切り替える。
    MoveColumn,
}

/// `shown` から `delta` だけ離れた番号。`0..count` を出るなら `None`（循環しない）。
fn step_index(shown: u8, delta: i8, count: u8) -> Option<u8> {
    let n = i16::from(shown).checked_add(i16::from(delta))?;
    if n < 0 || n >= i16::from(count) {
        return None;
    }
    u8::try_from(n).ok()
}

#[cfg(test)]
mod tests {
    use super::step_index;

    #[test]
    fn steps_within_range() {
        assert_eq!(step_index(0, 1, 4), Some(1));
        assert_eq!(step_index(2, -1, 4), Some(1));
        assert_eq!(step_index(2, 1, 4), Some(3));
    }

    #[test]
    fn out_of_range_does_nothing() {
        assert_eq!(step_index(3, 1, 4), None);
        assert_eq!(step_index(0, -1, 4), None);
        assert_eq!(step_index(0, 1, 0), None);
        assert_eq!(step_index(255, 1, 255), None);
    }
}

/// 幅 `width` を作業領域の幅 `area` に対する割合（整数パーセント、四捨五入）にし、`delta` を足して 1〜100 に収める。
fn adjusted_percent(width: i32, area: i32, delta: i8) -> Option<u32> {
    let width = i64::from(width.max(1));
    let area = i64::from(area.max(1));
    let current = width
        .checked_mul(100)?
        .checked_add(area.checked_div(2)?)?
        .checked_div(area)?;
    let next = current.checked_add(i64::from(delta))?.clamp(1, 100);
    u32::try_from(next).ok()
}

#[cfg(test)]
mod width_tests {
    use super::adjusted_percent;

    #[test]
    fn percent_is_rounded_and_clamped() {
        assert_eq!(adjusted_percent(450, 900, 10), Some(60));
        assert_eq!(adjusted_percent(450, 900, -10), Some(40));
        assert_eq!(adjusted_percent(300, 900, 10), Some(43));
        assert_eq!(adjusted_percent(900, 900, 10), Some(100));
        assert_eq!(adjusted_percent(90, 900, -50), Some(1));
        assert_eq!(adjusted_percent(1200, 900, -10), Some(100));
    }

    #[test]
    fn heights_use_the_same_rounding_against_the_area_height() {
        // 高さ 300 / 作業領域 600 = 50%。±10% で 60% / 40%。
        assert_eq!(adjusted_percent(300, 600, 10), Some(60));
        assert_eq!(adjusted_percent(300, 600, -10), Some(40));
        assert_eq!(adjusted_percent(600, 600, 10), Some(100));
        assert_eq!(adjusted_percent(6, 600, -50), Some(1));
    }
}
