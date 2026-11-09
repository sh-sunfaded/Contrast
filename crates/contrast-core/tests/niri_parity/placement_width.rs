//! 窓ごとの最初の列幅（`docs/core-spec-p14.md` の 20 節、D96・D97）。実装より先に書いた。

use contrast_core::{
    Action, ColumnWidth, Columns, Command, Core, CoreConfig, ExternalKey, LayoutMode, OutputName,
    Placement, PlacementContext, Policy, WindowMeta, WindowState,
};

use crate::common::{rect, rect_of};

/// app_id が "wide" の窓に Fixed(700)、ほかは既定。
struct WidePolicy;

impl Policy for WidePolicy {
    fn place(&self, meta: &WindowMeta, _ctx: &PlacementContext<'_>) -> Placement {
        let p = Placement::new(None, WindowState::Tiled, true);
        if meta.app_id.as_deref() == Some("wide") {
            p.with_column_width(ColumnWidth::fixed(700).expect("幅"))
        } else {
            p
        }
    }
}

fn core(mode: LayoutMode) -> Core<WidePolicy, Columns> {
    let mut c = Core::new(CoreConfig::new(4).with_layout(mode), WidePolicy, Columns::new());
    c.apply(Command::OutputAdded {
        key: ExternalKey::new(1),
        name: OutputName::new("o"),
        geometry: rect(0, 0, 900, 600),
    })
    .expect("出力");
    c
}

fn open(c: &mut Core<WidePolicy, Columns>, k: u64, app: &str) {
    let mut meta = WindowMeta::default();
    meta.app_id = Some(app.to_string());
    c.apply(Command::WindowMapped { key: ExternalKey::new(k), meta }).expect("窓");
    c.check_invariants().expect("不変条件");
}

fn width(c: &Core<WidePolicy, Columns>, k: u64) -> i32 {
    let id = c.view().window_id(ExternalKey::new(k)).expect("窓");
    c.view().window(id).and_then(|w| w.rect).expect("矩形").width()
}

#[test]
fn the_first_column_uses_the_placement_width() {
    let mut c = core(LayoutMode::Scrolling);
    open(&mut c, 11, "other");
    open(&mut c, 12, "wide");
    assert_eq!(width(&c, 11), 450, "既定 1/2");
    assert_eq!(width(&c, 12), 700);
}

#[test]
fn only_the_first_column_and_not_in_equal() {
    let mut c = core(LayoutMode::Scrolling);
    open(&mut c, 11, "other");
    open(&mut c, 12, "wide");
    // 12 を 11 の列へ取り込み、また出すと、新しい列は既定の幅。
    c.apply(Command::Action(Action::ConsumeOrExpelWindowLeft)).expect("ok");
    c.apply(Command::Action(Action::ConsumeOrExpelWindowRight)).expect("ok");
    c.check_invariants().expect("不変条件");
    assert_eq!(width(&c, 12), 450);
    let mut e = core(LayoutMode::Equal);
    open(&mut e, 11, "wide");
    assert_eq!(width(&e, 11), 900, "Equal では使わない");
    let _ = rect_of;
}

#[test]
fn a_floating_start_uses_it_when_it_becomes_tiled() {
    struct FloatWide;
    impl Policy for FloatWide {
        fn place(&self, _m: &WindowMeta, _c: &PlacementContext<'_>) -> Placement {
            Placement::new(None, WindowState::Floating, true)
                .with_column_width(ColumnWidth::fixed(300).expect("幅"))
        }
    }
    let mut c = Core::new(CoreConfig::new(4).with_layout(LayoutMode::Scrolling), FloatWide, Columns::new());
    c.apply(Command::OutputAdded {
        key: ExternalKey::new(1),
        name: OutputName::new("o"),
        geometry: rect(0, 0, 900, 600),
    })
    .expect("出力");
    c.apply(Command::WindowMapped { key: ExternalKey::new(11), meta: WindowMeta::default() }).expect("窓");
    c.apply(Command::Action(Action::ToggleFloating)).expect("ok");
    let id = c.view().window_id(ExternalKey::new(11)).expect("窓");
    assert_eq!(c.view().window(id).and_then(|w| w.rect).map(|r| r.width()), Some(300));
}
