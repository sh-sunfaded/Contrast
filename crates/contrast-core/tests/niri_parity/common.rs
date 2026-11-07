//! niri_parity のテスト共通の補助。公開 API だけを使う。
#![allow(
    dead_code,
    reason = "補助関数はファイルごとに使うものが違う"
)]

use contrast_core::{
    Action, Columns, Command, Core, CoreConfig, CoreError, DefaultPolicy, ExternalKey, LayoutMode,
    OutputName, Rect, WindowMeta,
};

/// 既定のポリシーと Columns レイアウトの Core。
pub type TestCore = Core<DefaultPolicy, Columns>;

/// 窓の最初のキー。`k` 番目の窓のキーは `FIRST_KEY + k`。
pub const FIRST_KEY: u64 = 11;

/// 出力の幅（高さは 600）。既定の列幅は 1/2（450）。
pub const OUTPUT_WIDTH: i32 = 900;

pub fn rect(x: i32, y: i32, w: i32, h: i32) -> Rect {
    Rect::new(x, y, w, h).expect("有効な矩形")
}

/// 出力 1 台（900x600）と、窓 `windows` 枚（キーは 11 から）の Core。`mode` で配置の方式を選ぶ。フォーカスは最後の窓。
pub fn core(mode: LayoutMode, windows: u64) -> TestCore {
    let mut c = Core::new(
        CoreConfig::new(4).with_layout(mode),
        DefaultPolicy::new(),
        Columns::new(),
    );
    apply_ok(
        &mut c,
        Command::OutputAdded {
            key: ExternalKey::new(1),
            name: OutputName::new("out"),
            geometry: rect(0, 0, OUTPUT_WIDTH, 600),
        },
    );
    for i in 0..windows {
        apply_ok(
            &mut c,
            Command::WindowMapped {
                key: ExternalKey::new(FIRST_KEY.saturating_add(i)),
                meta: WindowMeta::default(),
            },
        );
    }
    c
}

/// Scrolling の Core。
pub fn scrolling(windows: u64) -> TestCore {
    core(LayoutMode::Scrolling, windows)
}

/// 適用して、不変条件を確かめる。
pub fn apply_ok(c: &mut TestCore, cmd: Command) {
    c.apply(cmd).expect("apply ok");
    c.check_invariants().expect("不変条件");
}

/// Action を適用する。
pub fn act(c: &mut TestCore, a: Action) {
    apply_ok(c, Command::Action(a));
}

/// 適用して、エラーを返す。状態が変わっていないことも確かめる。
pub fn apply_err(c: &mut TestCore, cmd: Command) -> CoreError {
    let before = c.snapshot();
    let err = c.apply(cmd).expect_err("エラーになるはず");
    assert_eq!(c.snapshot(), before, "エラーでは状態を変えない");
    err
}

/// 窓 `k` の矩形。
pub fn rect_of(c: &TestCore, k: u64) -> Rect {
    let id = c.view().window_id(ExternalKey::new(k)).expect("窓がある");
    c.view().window(id).and_then(|w| w.rect).expect("矩形がある")
}

/// フォーカス窓のキー。
pub fn focus_key(c: &TestCore) -> Option<u64> {
    let id = c.view().focus()?;
    let want = c.view().window_key(id)?;
    (FIRST_KEY..FIRST_KEY.saturating_add(64)).find(|k| ExternalKey::new(*k) == want)
}

/// 窓 `keys` の、列ごとの並び。`x` の小さい列から、列の中は `y` の小さい窓から。各列の幅も返す。
pub fn columns(c: &TestCore, keys: &[u64]) -> Vec<(Vec<u64>, i32)> {
    let mut placed: Vec<(i32, i32, i32, u64)> = keys
        .iter()
        .filter_map(|k| {
            let id = c.view().window_id(ExternalKey::new(*k))?;
            let r = c.view().window(id)?.rect?;
            Some((r.x(), r.y(), r.width(), *k))
        })
        .collect();
    placed.sort();
    let mut out: Vec<(Vec<u64>, i32)> = Vec::new();
    let mut last_x = None;
    for (x, _, w, k) in placed {
        if last_x == Some(x) {
            if let Some((col, _)) = out.last_mut() {
                col.push(k);
            }
        } else {
            out.push((vec![k], w));
            last_x = Some(x);
        }
    }
    out
}
