//! `Contrast` の状態から作るワークスペースの一覧と、その差分（一覧の組み立てと差分は純粋）。
//!
//! core のワークスペースは全出力で共有され、数は変わらない。一覧の `id` と `idx` は `index + 1`（1 始まり）、
//! `name` は `None`。出力が表示しているワークスペースの `output` はその出力、表示していない（隠れた）ものは
//! 主出力（先頭の出力）。複数出力は対象外（プロトタイプは出力 1 つ）。

use super::protocol::{Event, Workspace};
use crate::state::Contrast;

/// 出力の表示状態から、ワークスペースの一覧を作る。
///
/// `count` はワークスペースの数。`shown` は出力ごとの（名前, 表示中のワークスペースの 0 始まりの位置）で、
/// 先頭が主出力。`focused_output` はフォーカス中の出力の名前。
pub(crate) fn build_workspaces(
    count: u8,
    shown: &[(String, u8)],
    focused_output: Option<&str>,
) -> Vec<Workspace> {
    let primary = shown.first().map(|(name, _)| name.clone());
    (0..count)
        .map(|index| {
            let owner = shown.iter().find(|(_, shown_index)| *shown_index == index);
            let idx = index.saturating_add(1);
            Workspace {
                id: u64::from(idx),
                idx,
                name: None,
                output: owner
                    .map(|(name, _)| name.clone())
                    .or_else(|| primary.clone()),
                is_urgent: false,
                is_active: owner.is_some(),
                is_focused: owner.is_some_and(|(name, _)| Some(name.as_str()) == focused_output),
                active_window_id: None,
            }
        })
        .collect()
}

/// 数・id・idx・名前・出力が同じか（表示・フォーカスの違いは見ない）。
fn same_structure(old: &[Workspace], new: &[Workspace]) -> bool {
    old.len() == new.len()
        && old.iter().zip(new).all(|(a, b)| {
            a.id == b.id && a.idx == b.idx && a.name == b.name && a.output == b.output
        })
}

/// 前に送った一覧 `old` から `new` になったときに送るイベント。
///
/// - 構成（数・id・idx・名前・出力）が変わった → `WorkspacesChanged`（完全な状態）。
/// - 構成は同じで、表示中・フォーカス中になったものがある → `WorkspaceActivated`
///   （フォーカスも移ったものを最後に並べる）。
/// - 何も変わらなければ空。
pub(crate) fn diff(old: &[Workspace], new: &[Workspace]) -> Vec<Event> {
    if !same_structure(old, new) {
        return vec![Event::WorkspacesChanged {
            workspaces: new.to_vec(),
        }];
    }
    let mut activated: Vec<(u64, bool)> = old
        .iter()
        .zip(new)
        .filter(|(a, b)| (b.is_active && !a.is_active) || (b.is_focused && !a.is_focused))
        .map(|(_, b)| (b.id, b.is_focused))
        .collect();
    // フォーカスの移ったものを最後に（ironbar は `focused:true` で「いまのフォーカス」を決める）。
    activated.sort_by_key(|(_, focused)| *focused);
    activated
        .into_iter()
        .map(|(id, focused)| Event::WorkspaceActivated { id, focused })
        .collect()
}

impl Contrast {
    /// いまのワークスペースの一覧。出力が無ければ、`output` が `None` で、どれも表示中でない一覧。
    pub(crate) fn ipc_workspaces(&self) -> Vec<Workspace> {
        let view = self.core.view();
        let shown: Vec<(String, u8)> = self
            .space
            .outputs()
            .filter_map(|output| {
                let id = view.output_id(self.shell.output_key?)?;
                let index = view.shown_workspace(id)?;
                Some((output.name(), index.get()))
            })
            .take(1)
            .collect();
        let focused = shown.first().map(|(name, _)| name.as_str());
        build_workspaces(view.workspace_count(), &shown, focused)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(name: &str, index: u8) -> Vec<(String, u8)> {
        vec![(name.to_owned(), index)]
    }

    #[test]
    fn the_list_is_one_based_with_the_shown_workspace_active_and_focused() {
        let list = build_workspaces(4, &shown("eDP-1", 1), Some("eDP-1"));
        let summary: Vec<(u64, u8, bool, bool)> = list
            .iter()
            .map(|w| (w.id, w.idx, w.is_active, w.is_focused))
            .collect();
        assert_eq!(
            summary,
            [
                (1, 1, false, false),
                (2, 2, true, true),
                (3, 3, false, false),
                (4, 4, false, false)
            ]
        );
        assert!(list.iter().all(|w| w.name.is_none() && !w.is_urgent));
        assert!(list.iter().all(|w| w.active_window_id.is_none()));
    }

    #[test]
    fn hidden_workspaces_belong_to_the_primary_output() {
        let list = build_workspaces(3, &shown("eDP-1", 0), Some("eDP-1"));
        assert!(list.iter().all(|w| w.output.as_deref() == Some("eDP-1")));
    }

    #[test]
    fn without_outputs_nothing_is_active_and_no_output_is_named() {
        let list = build_workspaces(2, &[], None);
        assert_eq!(list.len(), 2);
        assert!(list.iter().all(|w| w.output.is_none()));
        assert!(list.iter().all(|w| !w.is_active && !w.is_focused));
    }

    #[test]
    fn an_unfocused_output_keeps_its_active_workspace_unfocused() {
        let two = vec![("A".to_owned(), 0), ("B".to_owned(), 2)];
        let list = build_workspaces(3, &two, Some("A"));
        let flags: Vec<(bool, bool)> = list.iter().map(|w| (w.is_active, w.is_focused)).collect();
        assert_eq!(flags, [(true, true), (false, false), (true, false)]);
        assert_eq!(list.get(2).and_then(|w| w.output.as_deref()), Some("B"));
    }

    #[test]
    fn switching_the_workspace_sends_one_activation_with_focus() {
        let old = build_workspaces(4, &shown("eDP-1", 0), Some("eDP-1"));
        let new = build_workspaces(4, &shown("eDP-1", 2), Some("eDP-1"));
        assert_eq!(
            diff(&old, &new),
            [Event::WorkspaceActivated {
                id: 3,
                focused: true
            }]
        );
    }

    #[test]
    fn nothing_changed_sends_nothing() {
        let list = build_workspaces(4, &shown("eDP-1", 1), Some("eDP-1"));
        assert_eq!(diff(&list, &list), []);
        assert_eq!(diff(&[], &[]), []);
    }

    #[test]
    fn a_new_structure_sends_the_full_state() {
        let old = build_workspaces(4, &shown("eDP-1", 0), Some("eDP-1"));
        // 数が変わった。
        let more = build_workspaces(5, &shown("eDP-1", 0), Some("eDP-1"));
        assert_eq!(
            diff(&old, &more),
            [Event::WorkspacesChanged {
                workspaces: more.clone()
            }]
        );
        // 出力の名前が変わった。
        let renamed = build_workspaces(4, &shown("HDMI-A-1", 0), Some("HDMI-A-1"));
        assert_eq!(
            diff(&old, &renamed),
            [Event::WorkspacesChanged {
                workspaces: renamed
            }]
        );
        // 最初の一覧（空から）。
        assert_eq!(
            diff(&[], &old),
            [Event::WorkspacesChanged { workspaces: old }]
        );
    }

    #[test]
    fn a_secondary_output_switching_moves_ownership_so_the_full_state_is_sent() {
        let two = vec![("A".to_owned(), 0), ("B".to_owned(), 2)];
        let old = build_workspaces(3, &two, Some("A"));
        // B の表示が 2 → 1 に変わると、隠れたワークスペースの持ち主（主出力 A）が変わる。
        let two_after = vec![("A".to_owned(), 0), ("B".to_owned(), 1)];
        let new = build_workspaces(3, &two_after, Some("B"));
        assert_eq!(
            diff(&old, &new),
            [Event::WorkspacesChanged { workspaces: new }]
        );
    }

    #[test]
    fn focus_moving_between_outputs_with_the_same_display_sends_the_focused_activation() {
        let two = vec![("A".to_owned(), 0), ("B".to_owned(), 1)];
        let on_b = build_workspaces(3, &two, Some("B"));
        let on_a = build_workspaces(3, &two, Some("A"));
        assert_eq!(
            diff(&on_b, &on_a),
            [Event::WorkspaceActivated {
                id: 1,
                focused: true
            }]
        );
    }
}
