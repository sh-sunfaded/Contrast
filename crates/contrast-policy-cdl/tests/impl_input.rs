//! `<Input>`（タッチパッドなどの設定）の変換のテスト: 値、既定、範囲外の診断。

use std::fs;
use std::path::{Path, PathBuf};

use contrast_policy::{ConfigLoader, InputSettings, LoadOutcome, Settings};
use contrast_policy_cdl::CdlLoader;

/// 一時ファイルを書いて、そのパスを返す。`body` は `<Contrast>` の中身。
fn write_temp(dir: &str, body: &str) -> Result<PathBuf, String> {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("impl_input")
        .join(dir);
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let path = base.join("config.cdl");
    let text = format!(
        "\"use cdl 1\";\nimport {{ Contrast, Input }} from \"host:contrast\";\nexport default (\n  <Contrast>\n{body}\n  </Contrast>\n);\n"
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
fn all_values_are_read() -> Result<(), String> {
    let body = "<Input tapToClick={false} naturalScroll={true} accelSpeed={0.5} disableWhileTyping={false} />";
    let settings = load("all", body)?.map_err(|d| format!("{d:?}"))?;
    assert_eq!(
        settings.input,
        InputSettings {
            tap_to_click: false,
            natural_scroll: true,
            accel_speed: 0.5,
            disable_while_typing: false,
            focus_follows_mouse: false,
        }
    );
    Ok(())
}

#[test]
fn omitted_values_and_element_keep_the_defaults() -> Result<(), String> {
    let partial =
        load("partial", "<Input naturalScroll={true} />")?.map_err(|d| format!("{d:?}"))?;
    assert_eq!(
        partial.input,
        InputSettings {
            natural_scroll: true,
            ..InputSettings::default()
        }
    );
    let none = load("none", "")?.map_err(|d| format!("{d:?}"))?;
    assert_eq!(none.input, Settings::builtin().input);
    assert_eq!(InputSettings::default().accel_speed, 0.0);
    assert!(InputSettings::default().tap_to_click);
    assert!(InputSettings::default().disable_while_typing);
    assert!(!InputSettings::default().natural_scroll);
    Ok(())
}

#[test]
fn accel_speed_edges_are_accepted() -> Result<(), String> {
    for (dir, v, want) in [("lo", "-1", -1.0), ("hi", "1", 1.0), ("zero", "0", 0.0)] {
        let s =
            load(dir, &format!("<Input accelSpeed={{{v}}} />"))?.map_err(|d| format!("{d:?}"))?;
        assert_eq!(s.input.accel_speed, want);
    }
    Ok(())
}

#[test]
fn out_of_range_accel_speed_is_ct003() -> Result<(), String> {
    for (dir, v) in [("big", "1.5"), ("small", "-2"), ("huge", "100")] {
        let r = load(dir, &format!("<Input accelSpeed={{{v}}} />"))?;
        assert_eq!(r.err(), Some(vec!["CT003".to_owned()]), "accelSpeed={v}");
    }
    Ok(())
}

#[test]
fn wrong_types_and_duplicates_fail() -> Result<(), String> {
    assert!(load("type1", r#"<Input tapToClick="yes" />"#)?.is_err());
    assert!(load("type2", r#"<Input accelSpeed="fast" />"#)?.is_err());
    assert!(load("dup", "<Input /><Input />")?.is_err());
    Ok(())
}
