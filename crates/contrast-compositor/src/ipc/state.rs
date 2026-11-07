//! `Contrast` の状態から作るワークスペースの一覧と、その差分（一覧の組み立てと差分は純粋）。
//!
//! 固定のワークスペース（core の `Fixed`）は全出力で共有される。一覧の `idx` は位置 + 1、`name` は `None`。
//! 出力が表示しているワークスペースの `output` はその出力、表示していない（隠れた）ものは主出力（先頭の出力）。
//! 動的なワークスペース（`Dynamic`）は出力ごとの列（core の D74）。niri と同じく、`idx` は出力の列の中の位置 + 1、
//! `output` は持ち主の出力、`is_active` はその出力の表示中、`is_focused` はアクティブな出力の表示中（niri の
//! `ipc_refresh_workspaces`）。

use contrast_core::{IdKind, WorkspaceIndex};

use super::protocol::{Event, Workspace};
use crate::state::Contrast;

/// 出力の表示状態から、ワークスペースの一覧を作る。
///
/// `ids` はワークスペースの id を位置の順に並べたもの（core の `workspace_id`。動的なワークスペースでは位置と無関係）。
/// `idx` は位置 + 1。`shown` は出力ごとの（名前, 表示中のワークスペースの 0 始まりの位置）で、先頭が主出力。
/// `focused_output` はフォーカス中の出力の名前。
pub(crate) fn build_workspaces(
    ids: &[u64],
    shown: &[(String, u8)],
    focused_output: Option<&str>,
) -> Vec<Workspace> {
    let primary = shown.first().map(|(name, _)| name.clone());
    ids.iter()
        .enumerate()
        .map(|(position, id)| {
            let index = u8::try_from(position).unwrap_or(u8::MAX);
            let owner = shown.iter().find(|(_, shown_index)| *shown_index == index);
            Workspace {
                id: *id,
                idx: index.saturating_add(1),
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

/// 出力ごとの列から、ワークスペースの一覧を作る（`Dynamic`）。
///
/// `outputs` は出力ごとの（名前, 列の id の並び, 表示中の位置）を OutputId 昇順に。`focused_output` はアクティブな出力の名前。
/// 出力が無いときは `unowned`（持ち主の無い列）を、`output` が `None` で、どれも表示中でない一覧にする。
pub(crate) fn build_per_output(
    outputs: &[(String, Vec<u64>, usize)],
    unowned: &[u64],
    focused_output: Option<&str>,
) -> Vec<Workspace> {
    let entry = |id: u64, position: usize, output: Option<&str>, active: bool| Workspace {
        id,
        idx: u8::try_from(position.saturating_add(1)).unwrap_or(u8::MAX),
        name: None,
        output: output.map(str::to_owned),
        is_urgent: false,
        is_active: active,
        is_focused: active && output.is_some() && output == focused_output,
        active_window_id: None,
    };
    if outputs.is_empty() {
        return unowned
            .iter()
            .enumerate()
            .map(|(i, id)| entry(*id, i, None, false))
            .collect();
    }
    outputs
        .iter()
        .flat_map(|(name, ids, shown)| {
            ids.iter()
                .enumerate()
                .map(move |(i, id)| entry(*id, i, Some(name.as_str()), i == *shown))
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

/// `old` にもある（id が同じ）ワークスペースのうち、`new` で表示中・フォーカス中になったものの `WorkspaceActivated`。
/// フォーカスも移ったものを最後に並べる（ironbar は `focused:true` で「いまのフォーカス」を決める）。
fn activations(old: &[Workspace], new: &[Workspace]) -> Vec<Event> {
    let mut activated: Vec<(u64, bool)> = new
        .iter()
        .filter_map(|b| {
            let a = old.iter().find(|a| a.id == b.id)?;
            ((b.is_active && !a.is_active) || (b.is_focused && !a.is_focused))
                .then_some((b.id, b.is_focused))
        })
        .collect();
    activated.sort_by_key(|(_, focused)| *focused);
    activated
        .into_iter()
        .map(|(id, focused)| Event::WorkspaceActivated { id, focused })
        .collect()
}

/// 前に送った一覧 `old` から `new` になったときに送るイベント。
///
/// - 表示中・フォーカス中になった（前からある）ワークスペースがあれば、まず `WorkspaceActivated`。
/// - 構成（数・id・idx・名前・出力）が変わったら、そのあとに `WorkspacesChanged`（完全な状態）。
///   niri は切り替えの時点で `WorkspaceActivated` を出し、空のワークスペースの整理（`WorkspacesChanged`）はアニメーションが終わってから
///   出す。Contrast は整理が即時なので、同じ順序で続けて出す（ironbar は `WorkspacesChanged` では強調を更新しない）。
/// - 何も変わらなければ空。
pub(crate) fn diff(old: &[Workspace], new: &[Workspace]) -> Vec<Event> {
    let mut events = activations(old, new);
    if !same_structure(old, new) {
        events.push(Event::WorkspacesChanged {
            workspaces: new.to_vec(),
        });
    }
    events
}

impl Contrast {
    /// いまのワークスペースの一覧。出力が無ければ、`output` が `None` で、どれも表示中でない一覧。
    pub(crate) fn ipc_workspaces(&self) -> Vec<Workspace> {
        let view = self.core.view();
        let focused = self.active_output().map(|o| o.name());
        // 出力を OutputId 昇順（core の全体の並びと同じ順）に。
        let mut outputs: Vec<(contrast_core::OutputId, String)> = self
            .shell
            .outputs
            .iter()
            .filter_map(|(k, o)| Some((view.output_id(*k)?, o.name())))
            .collect();
        outputs.sort();
        let ids: Vec<u64> = (0..view.workspace_count())
            .filter_map(|i| view.workspace_id(WorkspaceIndex::new(i)))
            .map(|id| id.raw())
            .collect();
        if self.policy.workspace_mode() == contrast_core::WorkspaceMode::Dynamic {
            let lists: Vec<(String, Vec<u64>, usize)> = outputs
                .iter()
                .map(|(id, name)| {
                    let list: Vec<u64> = view
                        .output_workspaces(*id)
                        .iter()
                        .map(|w| w.raw())
                        .collect();
                    let shown = view
                        .shown_workspace(*id)
                        .and_then(|i| view.workspace_id(i))
                        .and_then(|w| list.iter().position(|x| *x == w.raw()))
                        .unwrap_or(0);
                    (name.clone(), list, shown)
                })
                .collect();
            return build_per_output(&lists, &ids, focused.as_deref());
        }
        let shown: Vec<(String, u8)> = outputs
            .iter()
            .filter_map(|(id, name)| Some((name.clone(), view.shown_workspace(*id)?.get())))
            .collect();
        build_workspaces(&ids, &shown, focused.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 位置 + 1 を id にした id の並び（固定のワークスペース）。
    fn seq(n: u8) -> Vec<u64> {
        (1..=u64::from(n)).collect()
    }

    fn shown(name: &str, index: u8) -> Vec<(String, u8)> {
        vec![(name.to_owned(), index)]
    }

    #[test]
    fn the_list_is_one_based_with_the_shown_workspace_active_and_focused() {
        let list = build_workspaces(&seq(4), &shown("eDP-1", 1), Some("eDP-1"));
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
        let list = build_workspaces(&seq(3), &shown("eDP-1", 0), Some("eDP-1"));
        assert!(list.iter().all(|w| w.output.as_deref() == Some("eDP-1")));
    }

    #[test]
    fn without_outputs_nothing_is_active_and_no_output_is_named() {
        let list = build_workspaces(&seq(2), &[], None);
        assert_eq!(list.len(), 2);
        assert!(list.iter().all(|w| w.output.is_none()));
        assert!(list.iter().all(|w| !w.is_active && !w.is_focused));
    }

    #[test]
    fn an_unfocused_output_keeps_its_active_workspace_unfocused() {
        let two = vec![("A".to_owned(), 0), ("B".to_owned(), 2)];
        let list = build_workspaces(&seq(3), &two, Some("A"));
        let flags: Vec<(bool, bool)> = list.iter().map(|w| (w.is_active, w.is_focused)).collect();
        assert_eq!(flags, [(true, true), (false, false), (true, false)]);
        assert_eq!(list.get(2).and_then(|w| w.output.as_deref()), Some("B"));
    }

    #[test]
    fn switching_the_workspace_sends_one_activation_with_focus() {
        let old = build_workspaces(&seq(4), &shown("eDP-1", 0), Some("eDP-1"));
        let new = build_workspaces(&seq(4), &shown("eDP-1", 2), Some("eDP-1"));
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
        let list = build_workspaces(&seq(4), &shown("eDP-1", 1), Some("eDP-1"));
        assert_eq!(diff(&list, &list), []);
        assert_eq!(diff(&[], &[]), []);
    }

    #[test]
    fn a_new_structure_sends_the_full_state() {
        let old = build_workspaces(&seq(4), &shown("eDP-1", 0), Some("eDP-1"));
        // 数が変わった。
        let more = build_workspaces(&seq(5), &shown("eDP-1", 0), Some("eDP-1"));
        assert_eq!(
            diff(&old, &more),
            [Event::WorkspacesChanged {
                workspaces: more.clone()
            }]
        );
        // 出力の名前が変わった。
        let renamed = build_workspaces(&seq(4), &shown("HDMI-A-1", 0), Some("HDMI-A-1"));
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
        let old = build_workspaces(&seq(3), &two, Some("A"));
        // B の表示が 2 → 1 に変わると、隠れたワークスペースの持ち主（主出力 A）が変わる。
        let two_after = vec![("A".to_owned(), 0), ("B".to_owned(), 1)];
        let new = build_workspaces(&seq(3), &two_after, Some("B"));
        // 切り替わったワークスペース（id 2）の `WorkspaceActivated` のあとに、完全な状態。
        assert_eq!(
            diff(&old, &new),
            [
                Event::WorkspaceActivated {
                    id: 2,
                    focused: true
                },
                Event::WorkspacesChanged { workspaces: new }
            ]
        );
    }

    #[test]
    fn ids_are_independent_of_positions_in_dynamic_workspaces() {
        // 動的なワークスペース: id は位置と無関係（空のワークスペースが消えても残りの id は変わらない）。
        let list = build_workspaces(&[1, 3, 4], &shown("eDP-1", 1), Some("eDP-1"));
        let summary: Vec<(u64, u8, bool)> =
            list.iter().map(|w| (w.id, w.idx, w.is_active)).collect();
        assert_eq!(summary, [(1, 1, false), (3, 2, true), (4, 3, false)]);
    }

    #[test]
    fn leaving_an_empty_workspace_sends_the_activation_then_the_removal() {
        // [A(1), 空(2)(表示), 末尾(3)] から A へ切り替えると、空(2)が消える。niri と同じ順序で、先に Activated、あとに Changed。
        let old = build_workspaces(&[1, 2, 3], &shown("eDP-1", 1), Some("eDP-1"));
        let new = build_workspaces(&[1, 3], &shown("eDP-1", 0), Some("eDP-1"));
        assert_eq!(
            diff(&old, &new),
            [
                Event::WorkspaceActivated {
                    id: 1,
                    focused: true
                },
                Event::WorkspacesChanged {
                    workspaces: new.clone()
                }
            ]
        );
    }

    #[test]
    fn a_new_trailing_workspace_alone_sends_only_the_full_state() {
        let old = build_workspaces(&[1], &shown("eDP-1", 0), Some("eDP-1"));
        let new = build_workspaces(&[1, 2], &shown("eDP-1", 0), Some("eDP-1"));
        assert_eq!(
            diff(&old, &new),
            [Event::WorkspacesChanged { workspaces: new }]
        );
    }

    #[test]
    fn focus_moving_between_outputs_with_the_same_display_sends_the_focused_activation() {
        let two = vec![("A".to_owned(), 0), ("B".to_owned(), 1)];
        let on_b = build_workspaces(&seq(3), &two, Some("B"));
        let on_a = build_workspaces(&seq(3), &two, Some("A"));
        assert_eq!(
            diff(&on_b, &on_a),
            [Event::WorkspaceActivated {
                id: 1,
                focused: true
            }]
        );
    }

    #[test]
    fn per_output_lists_number_each_output_from_one_and_focus_only_the_active_output() {
        let outputs = vec![
            ("A".to_owned(), vec![1, 3], 0),
            ("B".to_owned(), vec![2, 5, 4], 1),
        ];
        let list = build_per_output(&outputs, &[], Some("B"));
        let summary: Vec<(u64, u8, Option<&str>, bool, bool)> = list
            .iter()
            .map(|w| (w.id, w.idx, w.output.as_deref(), w.is_active, w.is_focused))
            .collect();
        assert_eq!(
            summary,
            [
                (1, 1, Some("A"), true, false),
                (3, 2, Some("A"), false, false),
                (2, 1, Some("B"), false, false),
                (5, 2, Some("B"), true, true),
                (4, 3, Some("B"), false, false),
            ]
        );
    }

    #[test]
    fn without_outputs_the_unowned_list_has_no_output_and_nothing_active() {
        let list = build_per_output(&[], &[7, 8], None);
        let summary: Vec<(u64, u8, bool)> =
            list.iter().map(|w| (w.id, w.idx, w.is_active)).collect();
        assert_eq!(summary, [(7, 1, false), (8, 2, false)]);
        assert!(list.iter().all(|w| w.output.is_none() && !w.is_focused));
    }
}
