//! 見た目の設定（`Layout`・`Appearance` の角丸と影・`Cursor`）の変換のテスト: 値、既定、範囲外の診断。

use std::fs;
use std::path::{Path, PathBuf};

use contrast_policy::{ConfigLoader, LoadOutcome, Settings};
use contrast_policy_cdl::CdlLoader;

/// 一時ファイルを書いて、そのパスを返す。`body` は `<Contrast>` の中身。
fn write_temp(dir: &str, body: &str) -> Result<PathBuf, String> {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("impl_look")
        .join(dir);
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let path = base.join("config.cdl");
    let text = format!(
        "\"use cdl 1\";\nimport {{ Contrast, Appearance, Layout, Cursor, Screenshot, HotkeyOverlay, Keybind }} from \"host:contrast\";\nexport default (\n  <Contrast>\n{body}\n  </Contrast>\n);\n"
    );
    fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(path)
}

/// 読み込んで、設定か診断コードの並びを返す。
fn load(dir: &str, body: &str) -> Result<Result<Settings, Vec<String>>, String> {
    let path = write_temp(dir, body)?;
    Ok(match CdlLoader::new().load(&path) {
        LoadOutcome::Loaded(c) => Ok(c.settings),
        LoadOutcome::Failed(d) => Err(d.iter().map(|d| d.code.clone()).collect()),
        LoadOutcome::Absent => Err(vec!["absent".to_owned()]),
    })
}

#[test]
fn defaults_keep_the_current_look() -> Result<(), String> {
    let s = load("none", "")?.map_err(|d| format!("{d:?}"))?;
    let b = Settings::builtin();
    assert_eq!(s.layout, b.layout);
    assert_eq!(s.layout.gap, 0);
    assert_eq!(s.layout.outer_gap, 0);
    assert_eq!(s.appearance.radius, 0);
    assert_eq!(s.appearance.shadow.blur, 0);
    assert_eq!(s.appearance.inactive_border_width, None);
    assert_eq!(s.cursor.theme, None);
    assert_eq!(s.cursor.size, None);
    Ok(())
}

#[test]
fn layout_appearance_and_cursor_are_read() -> Result<(), String> {
    let body = r##"
    <Appearance borderWidth={px(4)} focusColor={color("#7fc8ff")} radius={px(12)} shadowBlur={px(30)}
      shadowOffsetX={px(0)} shadowOffsetY={px(-5)} shadowColor={color("#00000080")} inactiveBorderWidth={px(2)} />
    <Layout gap={px(16)} outerGap={px(8)} />
    <Cursor theme="Cecilia" size={24} />"##;
    let s = load("all", body)?.map_err(|d| format!("{d:?}"))?;
    assert_eq!(s.layout.gap, 16);
    assert_eq!(s.layout.outer_gap, 8);
    assert_eq!(s.appearance.radius, 12);
    assert_eq!(s.appearance.shadow.blur, 30);
    assert_eq!(s.appearance.shadow.offset_x, 0);
    assert_eq!(s.appearance.shadow.offset_y, -5);
    assert!((s.appearance.shadow.color[3] - 128.0 / 255.0).abs() < 1e-6);
    assert_eq!(s.appearance.inactive_border_width, Some(2));
    assert_eq!(s.cursor.theme.as_deref(), Some("Cecilia"));
    assert_eq!(s.cursor.size, Some(24));
    Ok(())
}

#[test]
fn out_of_range_values_are_ct003() -> Result<(), String> {
    let bad = [
        ("gap_neg", "<Layout gap={px(-1)} />"),
        ("gap_big", "<Layout gap={px(513)} />"),
        ("gap_frac", "<Layout outerGap={px(1.5)} />"),
        (
            "radius_neg",
            r##"<Appearance borderWidth={px(1)} focusColor={color("#fff")} radius={px(-2)} />"##,
        ),
        (
            "shadow_big",
            r##"<Appearance borderWidth={px(1)} focusColor={color("#fff")} shadowBlur={px(9999)} />"##,
        ),
        (
            "offset_big",
            r##"<Appearance borderWidth={px(1)} focusColor={color("#fff")} shadowOffsetX={px(-600)} />"##,
        ),
        ("size_small", r#"<Cursor size={4} />"#),
        ("size_big", r#"<Cursor size={1000} />"#),
        ("size_frac", r#"<Cursor size={24.5} />"#),
    ];
    for (dir, body) in bad {
        let r = load(dir, body)?;
        assert_eq!(r.err(), Some(vec!["CT003".to_owned()]), "{dir}");
    }
    Ok(())
}

#[test]
fn edges_of_the_ranges_are_accepted() -> Result<(), String> {
    let s = load(
        "edges",
        "<Layout gap={px(512)} outerGap={px(0)} /><Cursor size={256} />",
    )?
    .map_err(|d| format!("{d:?}"))?;
    assert_eq!(s.layout.gap, 512);
    assert_eq!(s.cursor.size, Some(256));
    let s = load("edges2", "<Cursor size={8} />")?.map_err(|d| format!("{d:?}"))?;
    assert_eq!(s.cursor.size, Some(8));
    Ok(())
}

#[test]
fn duplicate_layout_or_cursor_fails() -> Result<(), String> {
    assert!(load("dup1", "<Layout /><Layout />")?.is_err());
    assert!(load("dup2", "<Cursor /><Cursor />")?.is_err());
    Ok(())
}

#[test]
fn scrolling_layout_options_are_read() -> Result<(), String> {
    let body = r#"<Layout mode="scrolling" defaultColumnWidth={0.4} presetColumnWidths="0.25, 0.5 0.75" />"#;
    let s = load("scroll", body)?.map_err(|d| format!("{d:?}"))?;
    assert_eq!(s.layout.mode, contrast_core::LayoutMode::Scrolling);
    assert_eq!(
        s.layout.default_column_width,
        contrast_core::Ratio::new(400, 1000)
    );
    let presets: Vec<_> = s
        .layout
        .preset_column_widths
        .iter()
        .map(|r| (r.num(), r.den()))
        .collect();
    assert_eq!(presets, vec![(250, 1000), (500, 1000), (750, 1000)]);
    // 既定は Equal・core の既定。
    let d = load("scroll_default", "<Layout />")?.map_err(|d| format!("{d:?}"))?;
    assert_eq!(d.layout.mode, contrast_core::LayoutMode::Equal);
    assert_eq!(d.layout.default_column_width, None);
    assert!(d.layout.preset_column_widths.is_empty());
    let e = load("scroll_equal", r#"<Layout mode="equal" />"#)?.map_err(|d| format!("{d:?}"))?;
    assert_eq!(e.layout.mode, contrast_core::LayoutMode::Equal);
    Ok(())
}

#[test]
fn bad_scrolling_layout_options_are_ct003() -> Result<(), String> {
    let bad = [
        ("mode", r#"<Layout mode="grid" />"#),
        ("width0", "<Layout defaultColumnWidth={0} />"),
        ("width2", "<Layout defaultColumnWidth={1.5} />"),
        ("preset_empty", r#"<Layout presetColumnWidths="" />"#),
        ("preset_bad", r#"<Layout presetColumnWidths="0.5 abc" />"#),
        ("preset_big", r#"<Layout presetColumnWidths="0.5 2" />"#),
        (
            "preset_many",
            r#"<Layout presetColumnWidths="0.1 0.2 0.3 0.4 0.5 0.6 0.7 0.8 0.9" />"#,
        ),
    ];
    for (dir, body) in bad {
        let r = load(dir, body)?;
        assert_eq!(r.err(), Some(vec!["CT003".to_owned()]), "{dir}");
    }
    Ok(())
}

#[test]
fn column_width_actions_need_valid_arguments() -> Result<(), String> {
    use contrast_policy::Action;
    let ok = r#"
    <Keybind keys="alt+1" action="set-column-width" width={0.333} />
    <Keybind keys="alt+2" action="adjust-column-width" delta={-10} />
    <Keybind keys="alt+3" action="adjust-column-width" delta={25} />"#;
    let s = load("actions_ok", ok)?.map_err(|d| format!("{d:?}"))?;
    let alt = contrast_policy::ModKey::Alt.mods();
    assert_eq!(
        s.resolve_key(alt, "1"),
        contrast_core::Ratio::new(333, 1000).map(Action::SetColumnWidth)
    );
    assert_eq!(
        s.resolve_key(alt, "2"),
        Some(Action::AdjustColumnWidth(-10))
    );
    assert_eq!(s.resolve_key(alt, "3"), Some(Action::AdjustColumnWidth(25)));
    let bad = [
        (
            "set_missing",
            r#"<Keybind keys="alt+1" action="set-column-width" />"#,
        ),
        (
            "set_range",
            r#"<Keybind keys="alt+1" action="set-column-width" width={2} />"#,
        ),
        (
            "adj_missing",
            r#"<Keybind keys="alt+1" action="adjust-column-width" />"#,
        ),
        (
            "adj_zero",
            r#"<Keybind keys="alt+1" action="adjust-column-width" delta={0} />"#,
        ),
        (
            "adj_big",
            r#"<Keybind keys="alt+1" action="adjust-column-width" delta={101} />"#,
        ),
        (
            "adj_frac",
            r#"<Keybind keys="alt+1" action="adjust-column-width" delta={1.5} />"#,
        ),
    ];
    for (dir, body) in bad {
        let r = load(dir, body)?;
        assert_eq!(r.err(), Some(vec!["CT003".to_owned()]), "{dir}");
    }
    Ok(())
}

#[test]
fn plain_scrolling_actions_are_recognised() -> Result<(), String> {
    use contrast_policy::Action;
    let names = [
        ("focus-up", Action::FocusUp),
        ("focus-down", Action::FocusDown),
        ("move-up", Action::MoveUp),
        ("move-down", Action::MoveDown),
        ("focus-column-first", Action::FocusColumnFirst),
        ("focus-column-last", Action::FocusColumnLast),
        ("move-column-first", Action::MoveColumnFirst),
        ("move-column-last", Action::MoveColumnLast),
        (
            "consume-window-into-column",
            Action::ConsumeWindowIntoColumn,
        ),
        ("expel-window-from-column", Action::ExpelWindowFromColumn),
        (
            "switch-preset-column-width",
            Action::SwitchPresetColumnWidth,
        ),
        ("maximize-column", Action::MaximizeColumn),
    ];
    let alt = contrast_policy::ModKey::Alt.mods();
    for (i, (name, want)) in names.into_iter().enumerate() {
        let body = format!(r#"<Keybind keys="alt+F{}" action="{name}" />"#, i + 1);
        let s = load(&format!("plain_{i}"), &body)?.map_err(|d| format!("{d:?}"))?;
        assert_eq!(
            s.resolve_key(alt, &format!("F{}", i + 1)),
            Some(want),
            "{name}"
        );
    }
    Ok(())
}

#[test]
fn animation_ms_defaults_to_off_and_accepts_the_edges() -> Result<(), String> {
    let none = load("anim-none", "")?.map_err(|d| format!("{d:?}"))?;
    assert_eq!(none.layout.animation_ms, 0);
    for (dir, v, want) in [
        ("zero", "0", 0),
        ("niri", "150", 150),
        ("max", "2000", 2000),
    ] {
        let s =
            load(dir, &format!("<Layout animationMs={{{v}}} />"))?.map_err(|d| format!("{d:?}"))?;
        assert_eq!(s.layout.animation_ms, want, "{v}");
        // ほかの項目は既定のまま。
        assert_eq!(s.layout.gap, 0);
    }
    Ok(())
}

#[test]
fn bad_animation_ms_is_ct003() -> Result<(), String> {
    for (dir, v) in [("negative", "-1"), ("fraction", "1.5"), ("over", "2001")] {
        let r = load(dir, &format!("<Layout animationMs={{{v}}} />"))?;
        assert_eq!(r.err(), Some(vec!["CT003".to_owned()]), "{v}");
    }
    Ok(())
}

#[test]
fn animation_kind_defaults_to_easing_and_accepts_spring() -> Result<(), String> {
    use contrast_policy::AnimationKind;
    let none = load("kind-none", "")?.map_err(|d| format!("{d:?}"))?;
    assert_eq!(none.layout.animation, AnimationKind::Easing);
    for (dir, v, want) in [
        ("kind-easing", "easing", AnimationKind::Easing),
        ("kind-spring", "spring", AnimationKind::Spring),
    ] {
        let s =
            load(dir, &format!("<Layout animation=\"{v}\" />"))?.map_err(|d| format!("{d:?}"))?;
        assert_eq!(s.layout.animation, want, "{v}");
    }
    let bad = load("kind-bad", "<Layout animation=\"bounce\" />")?;
    assert_eq!(bad.err(), Some(vec!["CT003".to_owned()]));
    Ok(())
}

#[test]
fn screenshot_path_defaults_to_niri_and_can_be_changed_or_disabled() -> Result<(), String> {
    let none = load("shot-none", "")?.map_err(|d| format!("{d:?}"))?;
    assert_eq!(
        none.screenshot_path.as_deref(),
        Some(contrast_policy::DEFAULT_SCREENSHOT_PATH)
    );
    let custom =
        load("shot-custom", "<Screenshot path=\"~/s/%Y.png\" />")?.map_err(|d| format!("{d:?}"))?;
    assert_eq!(custom.screenshot_path.as_deref(), Some("~/s/%Y.png"));
    let off = load("shot-off", "<Screenshot path=\"\" />")?.map_err(|d| format!("{d:?}"))?;
    assert_eq!(off.screenshot_path, None);
    Ok(())
}

#[test]
fn niri_example_animates() -> Result<(), String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/niri.cdl");
    match CdlLoader::new().load(&path) {
        LoadOutcome::Loaded(c) => {
            assert_eq!(c.settings.layout.animation_ms, 150);
            Ok(())
        }
        LoadOutcome::Failed(d) => Err(format!("{d:?}")),
        LoadOutcome::Absent => Err("niri.cdl が無い".to_owned()),
    }
}

#[test]
fn hotkey_overlay_defaults_to_niri_and_can_skip_at_startup_and_hide_not_bound() -> Result<(), String>
{
    let none = load("hotkey-none", "")?.map_err(|d| format!("{d:?}"))?;
    assert!(!none.hotkey_overlay.skip_at_startup);
    assert!(!none.hotkey_overlay.hide_not_bound);
    let both = load(
        "hotkey-both",
        "<HotkeyOverlay skipAtStartup={true} hideNotBound={true} />",
    )?
    .map_err(|d| format!("{d:?}"))?;
    assert!(both.hotkey_overlay.skip_at_startup && both.hotkey_overlay.hide_not_bound);
    let one = load("hotkey-one", "<HotkeyOverlay hideNotBound={true} />")?
        .map_err(|d| format!("{d:?}"))?;
    assert!(!one.hotkey_overlay.skip_at_startup && one.hotkey_overlay.hide_not_bound);
    let bad = load("hotkey-bad", "<HotkeyOverlay skipAtStartup=\"yes\" />")?;
    assert!(bad.is_err());
    Ok(())
}
