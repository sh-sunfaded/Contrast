//! `State`: 出力・ワークスペース・窓・フォーカス・ID 対応表の集約。
//!
//! ここには状態を読む・小さく書き換える操作だけを置き、ユースケースの判断は
//! `application/handlers/` が行う。
//!
//! 守る不変条件（操作ごとに保つもの。検査は `invariant.rs`）:
//! - I1 / I2 窓の所属（`Window::workspace`、id で参照。D73）と順序列を `move_window_to` / `remove_window` で同時に更新する。
//! - I3 `Fixed` のワークスペース数は固定（D1）。`Dynamic` は出力ごとの列（D74）。
//! - I4 フォーカスは `set_focus` だけが変え、履歴も同時に更新する。`Dynamic` ではアクティブな出力も同時に直す（D75）。
//! - I6 ID は `IdGen` から払い出し、対応表は `KeyMap` で持つ。
//! - I12 / I13 `Dynamic` の列の末尾の空（`ensure_trailing_empty`）、全体の並びの順（`normalize_order`）、持ち主の出力。

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::domain::column::{LayoutSettings, WorkspaceMode};
use crate::domain::id::{IdGen, IdKind, KeyMap, OutputId, WindowId, WorkspaceId, WorkspaceIndex};
use crate::domain::output::{Output, OutputName};
use crate::domain::window::Window;
use crate::domain::workspace::Workspace;

/// ワークスペース数の下限。
pub const MIN_WORKSPACES: u8 = 1;
/// ワークスペース数の上限。
pub const MAX_WORKSPACES: u8 = 32;

/// core の全状態。`Clone` して変更し、成功時だけ差し替える（I7）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// 出力。OutputId 昇順。
    pub outputs: BTreeMap<OutputId, Output>,
    /// ワークスペースの全体の並び（D73）。`Fixed` は添字がそのまま `WorkspaceIndex`（D1: 数は固定）。
    /// `Dynamic` は、持ち主の出力の OutputId 昇順に、各出力の列を順に（持ち主の無いものは最後）。
    pub workspaces: Vec<Workspace>,
    /// 窓。WindowId 昇順。
    pub windows: BTreeMap<WindowId, Window>,
    /// フォーカス中の窓。
    pub focus: Option<WindowId>,
    /// 出力の ExternalKey 対応表。
    pub output_keys: KeyMap<OutputId>,
    /// 窓の ExternalKey 対応表。
    pub window_keys: KeyMap<WindowId>,
    /// 出力 ID の払い出し器。
    pub output_ids: IdGen<OutputId>,
    /// 窓 ID の払い出し器。
    pub window_ids: IdGen<WindowId>,
    /// 窓の並べ方と、スクロール列レイアウトの設定（`Core::new` が決め、以後変わらない）。
    pub layout: LayoutSettings,
    /// 次に払い出すワークスペースの id（D63。`Dynamic` で増やす。使った id は再利用しない）。
    pub next_workspace_id: u64,
    /// アクティブな出力（`Dynamic` のみ。D75）。`Fixed` では使わない（`None`）。
    pub active_output: Option<OutputId>,
    /// 主の出力（`Dynamic` のみ。最初に足した出力。D77）。
    pub primary_output: Option<OutputId>,
    /// 出力の名前ごとの、最後に表示していたワークスペース（`Dynamic` のみ。D76・D77）。
    pub last_shown: BTreeMap<OutputName, WorkspaceId>,
    /// 直前の命令が `OutputAdded` で、末尾の空の補充（D74）を次の命令まで延ばしている（D84。niri の `add_output`）。
    pub trailing_deferred: bool,
}

/// `Dynamic` のワークスペースの個数の上限（`WorkspaceIndex` が `u8` なので）。
pub const MAX_DYNAMIC_WORKSPACES: usize = 255;

impl State {
    /// `workspace_count` 個（1〜32 に丸める）の空ワークスペースを持つ状態を作る。
    #[must_use]
    pub fn new(workspace_count: u8) -> Self {
        let count = workspace_count.clamp(MIN_WORKSPACES, MAX_WORKSPACES);
        let workspaces = (0..count)
            .map(|i| Workspace::new(WorkspaceId::from_raw(u64::from(i).saturating_add(1))))
            .collect();
        Self {
            outputs: BTreeMap::new(),
            workspaces,
            windows: BTreeMap::new(),
            focus: None,
            output_keys: KeyMap::new(),
            window_keys: KeyMap::new(),
            output_ids: IdGen::new(),
            window_ids: IdGen::new(),
            layout: LayoutSettings::default(),
            next_workspace_id: u64::from(count).saturating_add(1),
            active_output: None,
            primary_output: None,
            last_shown: BTreeMap::new(),
            trailing_deferred: false,
        }
    }

    /// ワークスペースの方式が `Dynamic` か（D62）。
    #[must_use]
    pub fn is_dynamic(&self) -> bool {
        self.layout.workspaces == WorkspaceMode::Dynamic
    }

    // ---- 位置と id -------------------------------------------------------------

    /// id のワークスペース。
    #[must_use]
    pub fn workspace(&self, id: WorkspaceId) -> Option<&Workspace> {
        self.workspaces.iter().find(|ws| ws.id == id)
    }

    /// id のワークスペース（可変）。
    pub fn workspace_mut(&mut self, id: WorkspaceId) -> Option<&mut Workspace> {
        self.workspaces.iter_mut().find(|ws| ws.id == id)
    }

    /// 全体の並びで位置 `index` のワークスペース（D73）。
    #[must_use]
    pub fn workspace_at(&self, index: WorkspaceIndex) -> Option<&Workspace> {
        self.workspaces.get(usize::from(index.get()))
    }

    /// ワークスペース `id` の、全体の並びでの位置（D73）。
    #[must_use]
    pub fn index_of(&self, id: WorkspaceId) -> Option<WorkspaceIndex> {
        self.workspaces
            .iter()
            .position(|ws| ws.id == id)
            .and_then(|p| u8::try_from(p).ok())
            .map(WorkspaceIndex::new)
    }

    /// ワークスペースの数（全体）。
    #[must_use]
    pub fn workspace_count(&self) -> u8 {
        u8::try_from(self.workspaces.len()).unwrap_or(u8::MAX)
    }

    /// 持ち主が `owner` のワークスペースの列（D74）。`Fixed` では全体の並び。
    #[must_use]
    pub fn list_of(&self, owner: Option<OutputId>) -> Vec<WorkspaceId> {
        if !self.is_dynamic() {
            return self.workspaces.iter().map(|ws| ws.id).collect();
        }
        self.workspaces
            .iter()
            .filter(|ws| ws.output == owner)
            .map(|ws| ws.id)
            .collect()
    }

    /// Action と Placement の `WorkspaceIndex` が指す列（D73）。`Dynamic` はアクティブな出力の列
    /// （出力が 0 台なら持ち主の無い列）、`Fixed` は全体の並び。
    #[must_use]
    pub fn active_list(&self) -> Vec<WorkspaceId> {
        self.list_of(if self.is_dynamic() {
            self.focused_output()
        } else {
            None
        })
    }

    /// 全体の並びを D73 の順（持ち主の OutputId 昇順、持ち主の無いものは最後。列の中の順は保つ）にする。
    pub fn normalize_order(&mut self) {
        if self.is_dynamic() {
            // 安定な並べ替えなので、同じ持ち主の中の順は変わらない。
            self.workspaces
                .sort_by_key(|ws| (ws.output.is_none(), ws.output));
        }
    }

    // ---- 出力 ------------------------------------------------------------------

    /// ワークスペース `id` を表示している出力。
    #[must_use]
    pub fn output_showing(&self, id: WorkspaceId) -> Option<OutputId> {
        self.outputs.values().find(|o| o.shown == id).map(|o| o.id)
    }

    /// 窓が表示中のワークスペースにあれば true。
    #[must_use]
    pub fn is_window_visible(&self, id: WindowId) -> bool {
        self.windows
            .get(&id)
            .is_some_and(|w| self.output_showing(w.workspace).is_some())
    }

    /// フォーカス出力。`Dynamic` はアクティブな出力（D75）。`Fixed`（と、アクティブな出力が無いとき）は D5:
    /// フォーカス窓のある出力、なければ OutputId 最小の出力。
    #[must_use]
    pub fn focused_output(&self) -> Option<OutputId> {
        let active = self
            .active_output
            .filter(|o| self.is_dynamic() && self.outputs.contains_key(o));
        active.or_else(|| {
            self.focus
                .and_then(|id| self.windows.get(&id))
                .and_then(|w| self.output_showing(w.workspace))
                .or_else(|| self.outputs.keys().next().copied())
        })
    }

    /// 出力 `output` の表示中のワークスペースの、D12 の規則のフォーカス（履歴の最新、なければ順序列の先頭）。
    #[must_use]
    pub fn preferred_focus_on(&self, output: OutputId) -> Option<WindowId> {
        self.outputs
            .get(&output)
            .and_then(|o| self.workspace(o.shown))
            .and_then(Workspace::preferred_focus)
    }

    // ---- フォーカスと窓 ----------------------------------------------------------

    /// フォーカスを `focus` にし、その窓のワークスペースの履歴を更新する。
    /// `Dynamic` で窓が表示中なら、アクティブな出力をその出力にする（D75）。
    pub fn set_focus(&mut self, focus: Option<WindowId>) {
        self.focus = focus;
        let Some(id) = focus else { return };
        let Some(ws_id) = self.windows.get(&id).map(|w| w.workspace) else {
            return;
        };
        if let Some(ws) = self.workspace_mut(ws_id) {
            ws.record_focus(id);
        }
        if self.is_dynamic()
            && let Some(o) = self.output_showing(ws_id)
        {
            self.active_output = Some(o);
        }
    }

    /// D74: 窓が入ったワークスペース `id` の元の出力を、持ち主の出力の名前にする（持ち主が無ければ変えない）。
    pub fn claim_workspace(&mut self, id: WorkspaceId) {
        let name = self
            .workspace(id)
            .and_then(|ws| ws.output)
            .and_then(|o| self.outputs.get(&o))
            .map(|o| o.name.clone());
        if let (Some(name), Some(ws)) = (name, self.workspace_mut(id)) {
            ws.original_output = Some(name);
        }
    }

    /// 窓をワークスペース `to` の順序列末尾へ移す（I1・I2 を保つ）。`to` の元の出力も直す（D74）。
    pub fn move_window_to(&mut self, id: WindowId, to: WorkspaceId) {
        let Some(from) = self.windows.get(&id).map(|w| w.workspace) else {
            return;
        };
        if let Some(ws) = self.workspace_mut(from) {
            ws.remove_window(id);
        }
        if let Some(ws) = self.workspace_mut(to) {
            ws.push_window(id);
        }
        if let Some(w) = self.windows.get_mut(&id) {
            w.workspace = to;
        }
        self.claim_workspace(to);
    }

    /// 窓を状態から除く。所属していたワークスペースと、順序列での位置を返す。
    pub fn remove_window(&mut self, id: WindowId) -> Option<(WorkspaceId, Option<usize>)> {
        let window = self.windows.remove(&id)?;
        let position = self
            .workspace_mut(window.workspace)
            .and_then(|ws| ws.remove_window(id));
        if self.focus == Some(id) {
            self.focus = None;
        }
        Some((window.workspace, position))
    }

    // ---- `Dynamic` の列 ------------------------------------------------------------

    /// 持ち主 `owner` の列の末尾に、新しい空のワークスペースを足す（新しい id。元の出力は持ち主の名前）。
    /// 個数の上限なら足さず `None`。
    pub fn append_empty_workspace(&mut self, owner: Option<OutputId>) -> Option<WorkspaceId> {
        if self.workspaces.len() >= MAX_DYNAMIC_WORKSPACES {
            return None;
        }
        let id = WorkspaceId::from_raw(self.next_workspace_id);
        self.next_workspace_id = self.next_workspace_id.saturating_add(1);
        let mut ws = Workspace::new(id);
        ws.output = owner;
        ws.original_output = owner
            .and_then(|o| self.outputs.get(&o))
            .map(|o| o.name.clone());
        // 持ち主の列の最後の後ろ（無ければ末尾）に入れ、全体の並びを D73 の順に保つ。
        let at = self
            .workspaces
            .iter()
            .rposition(|w| w.output == owner)
            .map_or(self.workspaces.len(), |p| p.saturating_add(1));
        self.workspaces.insert(at, ws);
        self.normalize_order();
        Some(id)
    }

    /// D64（`Dynamic`）: どの列も、末尾のワークスペースが空でなければ、新しい空を足す。
    /// 出力が 0 台なら、持ち主の無い列について同じことをする。
    pub fn ensure_trailing_empty(&mut self) {
        for owner in self.owners_lacking_trailing_empty() {
            self.append_empty_workspace(owner);
        }
    }

    /// 末尾が空でない（または空っぽの）列の持ち主（`Dynamic` のときだけ。D64）。
    #[must_use]
    pub fn owners_lacking_trailing_empty(&self) -> Vec<Option<OutputId>> {
        if !self.is_dynamic() {
            return Vec::new();
        }
        let owners: Vec<Option<OutputId>> = if self.outputs.is_empty() {
            alloc::vec![None]
        } else {
            self.outputs.keys().copied().map(Some).collect()
        };
        owners
            .into_iter()
            .filter(|owner| {
                !self
                    .list_of(*owner)
                    .last()
                    .and_then(|id| self.workspace(*id))
                    .is_some_and(|ws| ws.windows.is_empty())
            })
            .collect()
    }

    /// D65・D74（`Dynamic`）: 持ち主 `owner` の列で、末尾以外の、窓が無く表示されていないワークスペースを消す。
    pub fn clean_up(&mut self, owner: Option<OutputId>) {
        if !self.is_dynamic() {
            return;
        }
        let list = self.list_of(owner);
        let Some((_, rest)) = list.split_last() else {
            return;
        };
        for id in rest.iter().rev() {
            let removable = self.workspace(*id).is_some_and(|ws| ws.windows.is_empty())
                && self.output_showing(*id).is_none();
            if removable {
                self.workspaces.retain(|ws| ws.id != *id);
            }
        }
    }

    /// D65（`Dynamic`）: アクティブな出力の列（出力が 0 台なら持ち主の無い列）を整理する。
    pub fn clean_up_active(&mut self) {
        let owner = self.focused_output();
        self.clean_up(owner);
    }

    /// ワークスペース `a` と `b` の位置を入れ替える（D68。同じ列の 2 つ）。窓・出力は id で参照するので直さなくてよい。
    pub fn swap_workspaces(&mut self, a: WorkspaceId, b: WorkspaceId) {
        let pa = self.workspaces.iter().position(|ws| ws.id == a);
        let pb = self.workspaces.iter().position(|ws| ws.id == b);
        if let (Some(pa), Some(pb)) = (pa, pb) {
            self.workspaces.swap(pa, pb);
        }
    }
}
