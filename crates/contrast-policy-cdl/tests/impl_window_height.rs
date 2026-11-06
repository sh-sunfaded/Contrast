//! 窓の高さと center-column の配線（N3b）の変換のテスト: action、診断、プリセット、`examples/niri.cdl` の割り当て。

use std::fs;
use std::path::{Path, PathBuf};

use contrast_core::{Ratio, WindowHeight};
use contrast_policy::{Action, ConfigLoader, LoadOutcome, ModKey, Mods, Settings};
use contrast_policy_cdl::CdlLoader;

/// 一時ファイルを書いて、そのパスを返す。`body` は `<Contrast>` の中身。
/// テストは並列に走るので、`dir` が同じでも競合しないよう、テストの名前でも分ける。
fn write_temp(dir: &str, body: &str) -> Result<PathBuf, String> {
    let test = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .to_owned();
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("impl_window_height")
        .join(test)
        .join(dir);
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let path = base.join("config.cdl");
    let text = format!(
        "\"use cdl 1\";\nimport {{ Contrast, Layout, Keybind }} from \"host:contrast\";\nexport default (\n  <Contrast>\n{body}\n  </Contrast>\n);\n"
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

/// `Alt` を mod とした `key` の割り当て。
fn action_of(dir: &str, keybind: &str) -> Result<Option<Action>, String> {
    let s = load(dir, keybind)?.map_err(|d| format!("{d:?}"))?;
    Ok(s.resolve_key(
        Mods {
            alt: true,
            ..Mods::default()
        },
        "x",
    ))
}

/// 失敗する設定の診断コード。成功したらエラー。
fn codes_of(dir: &str, body: &str) -> Result<Vec<String>, String> {
    match load(dir, body)? {
        Ok(_) => Err(format!("誤りにならない: {body}")),
        Err(codes) => Ok(codes),
    }
}

fn ratio(n: u32, d: u32) -> Result<Ratio, String> {
    Ratio::new(n, d).ok_or_else(|| "ratio".to_owned())
}

#[test]
fn the_simple_actions_are_converted() -> Result<(), String> {
    for (dir, name, want) in [
        ("center", "center-column", Action::CenterColumn),
        (
            "preset",
            "switch-preset-window-height",
            Action::SwitchPresetWindowHeight,
        ),
        ("reset", "reset-window-height", Action::ResetWindowHeight),
    ] {
        let body = format!(r#"<Keybind keys="alt+x" action="{name}" />"#);
        assert_eq!(action_of(dir, &body)?, Some(want), "{name}");
    }
    Ok(())
}

#[test]
fn set_window_height_takes_a_proportion_or_pixels() -> Result<(), String> {
    assert_eq!(
        action_of(
            "prop",
            r#"<Keybind keys="alt+x" action="set-window-height" height={0.333} />"#
        )?,
        Some(Action::SetWindowHeight(WindowHeight::Proportion(ratio(
            333, 1000
        )?)))
    );
    assert_eq!(
        action_of(
            "full",
            r#"<Keybind keys="alt+x" action="set-window-height" height={1} />"#
        )?,
        Some(Action::SetWindowHeight(WindowHeight::Proportion(ratio(
            1000, 1000
        )?)))
    );
    for (dir, px) in [("px1", 1), ("px400", 400), ("pxmax", 16_384)] {
        let body =
            format!(r#"<Keybind keys="alt+x" action="set-window-height" heightPx={{{px}}} />"#);
        assert_eq!(
            action_of(dir, &body)?,
            Some(Action::SetWindowHeight(WindowHeight::Fixed(px))),
            "{px}"
        );
    }
    Ok(())
}

#[test]
fn bad_set_window_height_is_ct003() -> Result<(), String> {
    for (dir, props) in [
        ("neither", ""),
        ("both", "height={0.5} heightPx={100}"),
        ("zero", "height={0}"),
        ("over", "height={1.5}"),
        ("negative", "height={-0.2}"),
        ("px_zero", "heightPx={0}"),
        ("px_negative", "heightPx={-3}"),
        ("px_fraction", "heightPx={10.5}"),
        ("px_over", "heightPx={16385}"),
    ] {
        let body = format!(r#"<Keybind keys="alt+x" action="set-window-height" {props} />"#);
        assert_eq!(codes_of(dir, &body)?, ["CT003"], "{props}");
    }
    Ok(())
}

#[test]
fn adjust_window_height_takes_a_percent_delta() -> Result<(), String> {
    for (dir, delta, want) in [("up", "10", 10), ("down", "-10", -10), ("edge", "100", 100)] {
        let body =
            format!(r#"<Keybind keys="alt+x" action="adjust-window-height" delta={{{delta}}} />"#);
        assert_eq!(
            action_of(dir, &body)?,
            Some(Action::AdjustWindowHeight(want)),
            "{delta}"
        );
    }
    for (dir, props) in [
        ("none", ""),
        ("zero", "delta={0}"),
        ("over", "delta={101}"),
        ("fraction", "delta={1.5}"),
    ] {
        let body = format!(r#"<Keybind keys="alt+x" action="adjust-window-height" {props} />"#);
        assert_eq!(codes_of(dir, &body)?, ["CT003"], "{props}");
    }
    Ok(())
}

#[test]
fn window_height_presets_are_read() -> Result<(), String> {
    let s = load(
        "presets",
        r#"<Layout mode="scrolling" windowHeightPresets="0.25, 0.5 0.75" />"#,
    )?
    .map_err(|d| format!("{d:?}"))?;
    let got: Vec<_> = s
        .layout
        .window_height_presets
        .iter()
        .map(|r| (r.num(), r.den()))
        .collect();
    assert_eq!(got, vec![(250, 1000), (500, 1000), (750, 1000)]);
    // 列幅のプリセットとは別。
    assert!(s.layout.preset_column_widths.is_empty());
    let none = load("presets-none", "")?.map_err(|d| format!("{d:?}"))?;
    assert!(none.layout.window_height_presets.is_empty());
    Ok(())
}

#[test]
fn bad_window_height_presets_are_ct003() -> Result<(), String> {
    for (dir, value) in [
        ("empty", ""),
        ("word", "0.5 abc"),
        ("big", "0.5 2"),
        ("zero", "0"),
        ("many", "0.1 0.2 0.3 0.4 0.5 0.6 0.7 0.8 0.9"),
    ] {
        let body = format!(r#"<Layout windowHeightPresets="{value}" />"#);
        assert_eq!(codes_of(dir, &body)?, ["CT003"], "{value:?}");
    }
    Ok(())
}

#[test]
fn niri_example_binds_the_window_height_keys() -> Result<(), String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/niri.cdl");
    let loader = CdlLoader::new().with_default_mod_key(ModKey::Super);
    let s = match loader.load(&path) {
        LoadOutcome::Loaded(c) => c.settings,
        LoadOutcome::Failed(d) => return Err(format!("{d:?}")),
        LoadOutcome::Absent => return Err("niri.cdl が無い".to_owned()),
    };
    let m = ModKey::Super.mods();
    let shift = Mods { shift: true, ..m };
    let ctrl = Mods { ctrl: true, ..m };
    assert_eq!(s.resolve_key(m, "c"), Some(Action::CenterColumn));
    assert_eq!(
        s.resolve_key(shift, "r"),
        Some(Action::SwitchPresetWindowHeight)
    );
    assert_eq!(s.resolve_key(ctrl, "r"), Some(Action::ResetWindowHeight));
    assert_eq!(
        s.resolve_key(shift, "Minus"),
        Some(Action::AdjustWindowHeight(-10))
    );
    assert_eq!(
        s.resolve_key(shift, "Equal"),
        Some(Action::AdjustWindowHeight(10))
    );
    // 既存の列の操作は変わらない。
    assert_eq!(s.resolve_key(m, "r"), Some(Action::SwitchPresetColumnWidth));
    assert_eq!(
        s.resolve_key(m, "Minus"),
        Some(Action::AdjustColumnWidth(-10))
    );
    Ok(())
}
