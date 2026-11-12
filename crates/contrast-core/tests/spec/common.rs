//! core の仕様テスト共通の補助。公開 API だけを使う。
//! `check_invariants` と `snapshot` を使うので、`invariants` feature が前提。
#![allow(
    dead_code,
    clippy::field_reassign_with_default,
    reason = "補助関数はファイルごとに使うものが違う。WindowMeta は non_exhaustive なので Default から作る"
)]

use contrast_core::{
    Applied, Columns, Command, Core, CoreConfig, CoreError, DefaultPolicy, Effect, ExternalKey,
    OutputId, OutputName, Policy, Rect, WindowId, WindowMeta, WindowState, WorkspaceIndex,
};

/// 既定のポリシーと Columns レイアウトの Core。
pub type TestCore = Core<DefaultPolicy, Columns>;

/// ワークスペース数を指定した、空の Core。
pub fn new_core(workspaces: u8) -> TestCore {
    Core::new(
        CoreConfig::new(workspaces),
        DefaultPolicy::new(),
        Columns::new(),
    )
}

pub fn key(n: u64) -> ExternalKey {
    ExternalKey::new(n)
}

pub fn ws(n: u8) -> WorkspaceIndex {
    WorkspaceIndex::new(n)
}

pub fn rect(x: i32, y: i32, w: i32, h: i32) -> Rect {
    Rect::new(x, y, w, h).expect("有効な矩形")
}

pub fn meta() -> WindowMeta {
    WindowMeta::default()
}

pub fn dialog() -> WindowMeta {
    let mut m = WindowMeta::default();
    m.is_dialog = true;
    m
}

/// 出力の追加。幅は引数、高さは 600、原点は (0, 0)。
pub fn output_added(k: u64, width: i32) -> Command {
    Command::OutputAdded {
        key: key(k),
        name: OutputName::new("out"),
        geometry: rect(0, 0, width, 600),
    }
}

pub fn window_mapped(k: u64) -> Command {
    Command::WindowMapped {
        key: key(k),
        meta: meta(),
    }
}

/// ワークスペース 4 つ、出力 1 台（キー 1、幅 `width`、高さ 600）、窓 `windows` 枚（キー 11 から）。
pub fn setup(width: i32, windows: u64) -> TestCore {
    let mut c = new_core(4);
    apply_ok(&mut c, output_added(1, width));
    for i in 0..windows {
        apply_ok(&mut c, window_mapped(11 + i));
    }
    c
}

/// 成功を期待して適用する。不変条件と Effect の順序も確かめる。
pub fn apply_ok<P: Policy>(core: &mut Core<P, Columns>, cmd: Command) -> Applied {
    let applied = core.apply(cmd).expect("apply が成功する");
    core.check_invariants().expect("不変条件が保たれる");
    assert_effect_order(&applied.effects);
    applied
}

/// 失敗を期待して適用する。状態が変わらないことも確かめる。
pub fn apply_err<P: Policy>(core: &mut Core<P, Columns>, cmd: Command) -> CoreError {
    let before = core.snapshot();
    let err = core.apply(cmd).expect_err("apply が失敗する");
    assert_eq!(core.snapshot(), before, "Err のとき状態は変わらない");
    core.check_invariants().expect("不変条件が保たれる");
    err
}

/// 成功し、Effect が空で、状態も変わらないことを期待して適用する。
pub fn apply_noop<P: Policy>(core: &mut Core<P, Columns>, cmd: Command) {
    let before = core.snapshot();
    let applied = core.apply(cmd).expect("apply が成功する");
    assert!(
        applied.effects.is_empty(),
        "Effect は空: {:?}",
        applied.effects
    );
    assert_eq!(core.snapshot(), before, "状態は変わらない");
}

pub fn win<P: Policy>(core: &Core<P, Columns>, k: u64) -> WindowId {
    core.view().window_id(key(k)).expect("登録済みの窓")
}

pub fn out<P: Policy>(core: &Core<P, Columns>, k: u64) -> OutputId {
    core.view().output_id(key(k)).expect("登録済みの出力")
}

pub fn rect_of<P: Policy>(core: &Core<P, Columns>, k: u64) -> Option<Rect> {
    core.view().window(win(core, k)).and_then(|w| w.rect)
}

pub fn state_of<P: Policy>(core: &Core<P, Columns>, k: u64) -> WindowState {
    core.view().window(win(core, k)).expect("窓").state
}

pub fn visible<P: Policy>(core: &Core<P, Columns>, k: u64) -> bool {
    core.view().window(win(core, k)).expect("窓").visible
}

/// Effect の種類の並び。順序や有無の検査に使う。
pub fn kinds(effects: &[Effect]) -> Vec<&'static str> {
    effects
        .iter()
        .map(|e| match e {
            Effect::Registered { .. } => "Registered",
            Effect::ShowWorkspace { .. } => "ShowWorkspace",
            Effect::Configure { .. } => "Configure",
            Effect::SetFocus(_) => "SetFocus",
            Effect::RequestClose(_) => "RequestClose",
            Effect::Damage(_) => "Damage",
            _ => "?",
        })
        .collect()
}

/// core-spec 5 節の Effect 列の順序。
/// Registered → ShowWorkspace（OutputId 昇順）→ Configure（WindowId 昇順）→ SetFocus（最大 1）
/// → RequestClose → Damage（OutputId 昇順、重複なし）。
pub fn assert_effect_order(effects: &[Effect]) {
    let rank = |e: &Effect| match e {
        Effect::Registered { .. } => 0,
        Effect::ShowWorkspace { .. } => 1,
        Effect::Configure { .. } => 2,
        Effect::SetFocus(_) => 3,
        Effect::RequestClose(_) => 4,
        Effect::Damage(_) => 5,
        _ => 6,
    };
    for pair in effects.windows(2) {
        assert!(
            rank(&pair[0]) <= rank(&pair[1]),
            "Effect の順序が仕様と違う: {effects:?}"
        );
    }
    let shown: Vec<OutputId> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::ShowWorkspace { output, .. } => Some(*output),
            _ => None,
        })
        .collect();
    assert!(
        shown.windows(2).all(|p| p[0] < p[1]),
        "ShowWorkspace は OutputId の昇順: {effects:?}"
    );
    let configured: Vec<WindowId> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::Configure { window, .. } => Some(*window),
            _ => None,
        })
        .collect();
    assert!(
        configured.windows(2).all(|p| p[0] < p[1]),
        "Configure は WindowId の昇順で、重複しない: {effects:?}"
    );
    let focus_changes = effects
        .iter()
        .filter(|e| matches!(e, Effect::SetFocus(_)))
        .count();
    assert!(focus_changes <= 1, "SetFocus は最大 1 つ: {effects:?}");
    let damaged: Vec<OutputId> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::Damage(output) => Some(*output),
            _ => None,
        })
        .collect();
    assert!(
        damaged.windows(2).all(|p| p[0] < p[1]),
        "Damage は OutputId の昇順で、重複しない: {effects:?}"
    );
}
