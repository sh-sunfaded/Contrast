//! ユースケースごとの handler（1 ファイル 1 ユースケース）。
//!
//! handler は複製された `State` を `&mut` で受け、Registered / RequestClose だけを返す。
//! 再配置と残りの Effect は `Core::apply` が行う。Err を返したら複製は捨てられる（I7）。

mod center_column;
mod close_focused;
mod consume_or_expel;
mod consume_window;
mod expel_window;
mod focus_column;
mod focus_direction;
mod maximize_column;
mod move_column;
mod move_to_workspace;
mod move_window;
mod output_added;
mod output_changed;
mod output_insets_changed;
mod output_removed;
mod reset_window_height;
mod set_column_width;
mod set_window_height;
mod switch_preset_height;
mod switch_preset_width;
mod switch_workspace;
mod toggle_floating;
mod toggle_fullscreen;
mod window_focus_requested;
mod window_mapped;
mod window_meta_changed;
mod window_unmapped;

use alloc::vec::Vec;

use crate::application::command::{Action, Command};
use crate::application::effect::Effect;
use crate::domain::id::WorkspaceIndex;
use crate::domain::state::State;
use crate::domain::window::WindowState;
use crate::error::CoreError;
use crate::policy::Policy;

/// Command を対応する handler に振り分ける。
///
/// # Errors
/// 各 handler の `CoreError`。
pub fn dispatch<P: Policy>(
    state: &mut State,
    policy: &P,
    command: Command,
) -> Result<Vec<Effect>, CoreError> {
    match command {
        Command::OutputAdded {
            key,
            name,
            geometry,
        } => output_added::handle(state, key, name, geometry),
        Command::OutputRemoved(key) => output_removed::handle(state, key),
        Command::OutputChanged { key, geometry } => output_changed::handle(state, key, geometry),
        Command::OutputInsetsChanged { key, insets } => {
            output_insets_changed::handle(state, key, insets)
        }
        Command::WindowMapped { key, meta } => window_mapped::handle(state, policy, key, meta),
        Command::WindowUnmapped(key) => window_unmapped::handle(state, key),
        Command::WindowMetaChanged { key, meta } => window_meta_changed::handle(state, key, meta),
        Command::WindowFocusRequested(key) => window_focus_requested::handle(state, key),
        Command::Action(action) => dispatch_action(state, action),
    }
}

/// Action を対応する handler に振り分ける。
///
/// # Errors
/// 各 handler の `CoreError`。
fn dispatch_action(state: &mut State, action: Action) -> Result<Vec<Effect>, CoreError> {
    match action {
        Action::FocusDirection(direction) => focus_direction::handle(state, direction),
        Action::MoveWindow(direction) => move_window::handle(state, direction),
        Action::SwitchWorkspace(index) => switch_workspace::handle(state, index),
        Action::MoveToWorkspace(index) => move_to_workspace::handle(state, index),
        Action::ToggleFloating => toggle_floating::handle(state),
        Action::ToggleFullscreen => toggle_fullscreen::handle(state),
        Action::CloseFocused => close_focused::handle(state),
        // D42〜D45: `Scrolling` 専用の Action。`Equal` では各 handler が状態不変で `Ok` を返す。
        Action::FocusColumnFirst => focus_column::handle(state, focus_column::End::First),
        Action::FocusColumnLast => focus_column::handle(state, focus_column::End::Last),
        Action::MoveColumnToFirst => move_column::handle(state, focus_column::End::First),
        Action::MoveColumnToLast => move_column::handle(state, focus_column::End::Last),
        Action::ConsumeWindowIntoColumn => consume_window::handle(state),
        Action::ExpelWindowFromColumn => expel_window::handle(state),
        Action::SwitchPresetColumnWidth => switch_preset_width::handle(state),
        Action::MaximizeColumn => maximize_column::handle(state),
        Action::SetColumnWidth(width) => set_column_width::handle(state, width),
        // D51〜D54: 窓の高さと CenterColumn。`Equal` では各 handler が状態不変で `Ok` を返す。
        Action::SetWindowHeight(height) => set_window_height::handle(state, height),
        Action::SwitchPresetWindowHeight => switch_preset_height::handle(state),
        Action::ResetWindowHeight => reset_window_height::handle(state),
        Action::CenterColumn => center_column::handle(state),
        // D57〜D61: 隣の列への取り込みと、隣への押し出し。`Equal` では状態不変で `Ok`。
        Action::ConsumeOrExpelWindowLeft => consume_or_expel::handle(state, true),
        Action::ConsumeOrExpelWindowRight => consume_or_expel::handle(state, false),
    }
}

/// フォーカス窓。対象窓が必要な Action で使う。
///
/// # Errors
/// フォーカスが None なら `NoFocusedWindow`。
fn focused(state: &State) -> Result<crate::domain::id::WindowId, CoreError> {
    state.focus.ok_or(CoreError::NoFocusedWindow)
}

/// `index` が範囲内か確かめる。
///
/// # Errors
/// `workspace_count` 以上なら `WorkspaceIndexOutOfRange`。
fn check_index(state: &State, index: WorkspaceIndex) -> Result<(), CoreError> {
    if index.get() < state.workspace_count() {
        Ok(())
    } else {
        Err(CoreError::WorkspaceIndexOutOfRange)
    }
}

/// D9: ワークスペース `index` の全画面窓のうち `except` 以外を元の状態に戻す。
fn demote_other_fullscreen(
    state: &mut State,
    index: WorkspaceIndex,
    except: crate::domain::id::WindowId,
) {
    let ids = state
        .workspace(index)
        .map(|ws| ws.windows.clone())
        .unwrap_or_default();
    for id in ids {
        if id == except {
            continue;
        }
        let fullscreen = state
            .windows
            .get_mut(&id)
            .filter(|w| w.state == WindowState::Fullscreen);
        if let Some(w) = fullscreen {
            w.state = w.restore_state;
        }
    }
}
