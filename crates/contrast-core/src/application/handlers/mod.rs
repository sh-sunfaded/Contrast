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
mod focus_monitor;
mod maximize_column;
mod move_column;
mod move_column_to_monitor;
mod move_column_to_workspace;
mod move_to_workspace;
mod move_window;
mod move_workspace;
mod output_added;
mod output_changed;
mod output_focus_requested;
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

use crate::application::command::{Action, Command, Direction};
use crate::application::effect::Effect;
use crate::domain::id::{OutputId, WindowId, WorkspaceId, WorkspaceIndex};
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
        Command::OutputFocusRequested(key) => output_focus_requested::handle(state, key),
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
        // D67・D68: 列ごとの移動と、ワークスペースの入れ替え。
        Action::MoveColumnToWorkspace(index) => move_column_to_workspace::handle(state, index),
        Action::MoveWorkspaceDown => move_workspace::handle(state, true),
        Action::MoveWorkspaceUp => move_workspace::handle(state, false),
        // D78・D79: 複数モニタ（`Dynamic` のみ）。
        Action::FocusMonitor(direction) => focus_monitor::handle(state, direction),
        Action::MoveColumnToMonitor(direction) => move_column_to_monitor::handle(state, direction),
    }
}

/// フォーカス窓。対象窓が必要な Action で使う。
///
/// # Errors
/// フォーカスが None なら `NoFocusedWindow`。
fn focused(state: &State) -> Result<WindowId, CoreError> {
    state.focus.ok_or(CoreError::NoFocusedWindow)
}

/// `index` が範囲内か確かめる（`Fixed`。全体の個数で）。
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

/// 出力 `output` が表示するワークスペースを `target` にする。`Fixed` で `target` を別の出力が表示していれば、
/// その出力には元のワークスペースを表示させる（D12 の入れ替え）。`Dynamic` の列は他の出力に表示されない（D81）。
fn show_workspace(state: &mut State, output: OutputId, target: WorkspaceId) {
    let Some(current) = state.outputs.get(&output).map(|o| o.shown) else {
        return;
    };
    let other = state.output_showing(target);
    if let Some(o) = other.and_then(|id| state.outputs.get_mut(&id)) {
        o.shown = current;
    }
    if let Some(o) = state.outputs.get_mut(&output) {
        o.shown = target;
    }
}

/// 命令の `index` を、ワークスペースの id にする（D66・D73・D81）。
/// `Dynamic` はアクティブな出力の列の中の位置で、末尾に丸める。`Fixed` は全体の位置で、範囲外ならエラー。
///
/// # Errors
/// `Fixed` で `workspace_count` 以上なら `WorkspaceIndexOutOfRange`。
fn resolve_index(state: &State, index: WorkspaceIndex) -> Result<WorkspaceId, CoreError> {
    if state.is_dynamic() {
        let list = state.active_list();
        let last = list.len().saturating_sub(1);
        return list
            .get(usize::from(index.get()).min(last))
            .copied()
            .ok_or(CoreError::WorkspaceIndexOutOfRange);
    }
    check_index(state, index)?;
    state
        .workspace_at(index)
        .map(|ws| ws.id)
        .ok_or(CoreError::WorkspaceIndexOutOfRange)
}

/// D9: ワークスペース `ws` の全画面窓のうち `except` 以外を元の状態に戻す。
fn demote_other_fullscreen(state: &mut State, ws: WorkspaceId, except: WindowId) {
    let ids = state
        .workspace(ws)
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

/// D76・D77 の最後: `Dynamic` で、フォーカスがアクティブな出力の表示中のワークスペースの窓でなければ（None を含む）、
/// そのワークスペースの D12 の規則のフォーカスにする（I13）。出力が無ければ None。
fn fix_focus_on_active_output(state: &mut State) {
    if !state.is_dynamic() {
        return;
    }
    let Some(active) = state.focused_output() else {
        state.set_focus(None);
        return;
    };
    let shown = state.outputs.get(&active).map(|o| o.shown);
    let ok = state
        .focus
        .and_then(|f| state.windows.get(&f))
        .is_some_and(|w| Some(w.workspace) == shown);
    if !ok {
        let next = state.preferred_focus_on(active);
        state.set_focus(next);
    }
}

/// D78: アクティブな出力から見て、`direction` の出力。中心がその方向にあり、直交する軸で（端が接するだけでなく）重なる
/// 出力のうち、その軸の中心の距離が最小のもの（同じなら OutputId が小さい方）。無ければ `None`。
fn neighbor_output(state: &State, direction: Direction) -> Option<OutputId> {
    let current = state.focused_output()?;
    let here = state.outputs.get(&current)?.geometry;
    let span = |start: i32, len: i32| {
        (
            i64::from(start),
            i64::from(start).saturating_add(i64::from(len)),
        )
    };
    let center =
        |start: i32, len: i32| i64::from(start).saturating_add(i64::from(len).div_euclid(2));
    let (cx, cy) = (
        center(here.x(), here.width()),
        center(here.y(), here.height()),
    );
    let (hx, hy) = (span(here.x(), here.width()), span(here.y(), here.height()));
    let overlaps = |a: (i64, i64), b: (i64, i64)| a.0 < b.1 && b.0 < a.1;
    let mut best: Option<(i64, OutputId)> = None;
    for o in state.outputs.values() {
        let g = o.geometry;
        let (ox, oy) = (center(g.x(), g.width()), center(g.y(), g.height()));
        let (gx, gy) = (span(g.x(), g.width()), span(g.y(), g.height()));
        let candidate = match direction {
            Direction::Left => (ox < cx && overlaps(gy, hy)).then(|| cx.saturating_sub(ox)),
            Direction::Right => (ox > cx && overlaps(gy, hy)).then(|| ox.saturating_sub(cx)),
            Direction::Up => (oy < cy && overlaps(gx, hx)).then(|| cy.saturating_sub(oy)),
            Direction::Down => (oy > cy && overlaps(gx, hx)).then(|| oy.saturating_sub(cy)),
        };
        if let Some(distance) = candidate
            && best.is_none_or(|(d, _)| distance < d)
        {
            best = Some((distance, o.id));
        }
    }
    best.map(|(_, id)| id)
}
