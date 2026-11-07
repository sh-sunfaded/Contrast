//! `Keybind` の `cooldown`（niri の `cooldown-ms`）と、ホイールのキー名（`WheelScrollDown` など）の変換のテスト。

use std::fs;
use std::path::{Path, PathBuf};

use contrast_policy::{Action, ConfigLoader, LoadOutcome, ModKey, Mods, Settings};
use contrast_policy_cdl::CdlLoader;

/// 一時ファイルを書いて、そのパスを返す。`body` は `<Contrast>` の中身。テスト名でディレクトリを分ける。
fn write_temp(dir: &str, body: &str) -> Result<PathBuf, String> {
    let test = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .to_owned();
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("impl_cooldown")
        .join(test)
        .join(dir);
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let path = base.join("config.cdl");
    let text = format!(
        "\"use cdl 1\";\nimport {{ Contrast, Keybind }} from \"host:contrast\";\nexport default (\n  <Contrast>\n{body}\n  </Contrast>\n);\n"
    );
    fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(path)
}

/// 読み込んで、設定か診断コードの並びを返す。
fn load(dir: &str, body: &str) -> Result<Result<Settings, Vec<String>>, String> {
    let path = write_temp(dir, body)?;
    let loader = CdlLoader::new().with_default_mod_key(ModKey::Super);
    Ok(match loader.load(&path) {
        LoadOutcome::Loaded(c) => Ok(c.settings),
        LoadOutcome::Failed(d) => Err(d.iter().map(|d| d.code.clone()).collect()),
        LoadOutcome::Absent => Err(vec!["absent".to_owned()]),
    })
}

fn super_mods() -> Mods {
    ModKey::Super.mods()
}

#[test]
fn a_cooldown_is_kept_on_the_binding_that_has_it() -> Result<(), String> {
    let body = r#"
    <Keybind keys="mod+WheelScrollDown" action="workspace-next" cooldown={150} />
    <Keybind keys="mod+WheelScrollRight" action="focus-right" />"#;
    let s = load("kept", body)?.map_err(|d| format!("{d:?}"))?;
    let m = super_mods();
    assert_eq!(
        s.resolve_key(m, "WheelScrollDown"),
        Some(Action::WorkspaceNext)
    );
    assert_eq!(s.resolve_cooldown_ms(m, "WheelScrollDown"), Some(150));
    // 大文字小文字を区別しない（キー名は小文字にして持つ）。
    assert_eq!(s.resolve_cooldown_ms(m, "wheelscrolldown"), Some(150));
    // 間引き指定のないバインドと、バインドのないキーは `None`。
    assert_eq!(
        s.resolve_key(m, "WheelScrollRight"),
        Some(Action::FocusNext)
    );
    assert_eq!(s.resolve_cooldown_ms(m, "WheelScrollRight"), None);
    assert_eq!(s.resolve_cooldown_ms(m, "WheelScrollUp"), None);
    Ok(())
}

#[test]
fn a_cooldown_outside_the_range_is_ct003() -> Result<(), String> {
    for (dir, value) in [
        ("zero", "0"),
        ("negative", "-1"),
        ("fraction", "1.5"),
        ("huge", "3600001"),
    ] {
        let body = format!(
            r#"<Keybind keys="mod+WheelScrollDown" action="workspace-next" cooldown={{{value}}} />"#
        );
        assert_eq!(
            load(dir, &body)?.err(),
            Some(vec!["CT003".to_owned()]),
            "{value}"
        );
    }
    // 範囲の端は通る。
    let ok = r#"<Keybind keys="mod+WheelScrollDown" action="workspace-next" cooldown={3600000} />"#;
    assert!(load("max", ok)?.is_ok());
    Ok(())
}

#[test]
fn the_wheel_binds_of_config_resolve_with_their_cooldowns() -> Result<(), String> {
    let loader = CdlLoader::new().with_default_mod_key(ModKey::Super);
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/config.cdl");
    let LoadOutcome::Loaded(c) = loader.load(&path) else {
        return Err("config/config.cdl を読めない".to_owned());
    };
    let s = c.settings;
    let m = super_mods();
    let ctrl = Mods { ctrl: true, ..m };
    let shift = Mods { shift: true, ..m };
    let ctrl_shift = Mods {
        ctrl: true,
        shift: true,
        ..m
    };
    // niri の config.kdl の 12 個（縦のワークスペースの 4 つだけ cooldown-ms=150）。
    let table: [(Mods, &str, Action, Option<u32>); 12] = [
        (m, "WheelScrollDown", Action::WorkspaceNext, Some(150)),
        (m, "WheelScrollUp", Action::WorkspacePrev, Some(150)),
        (
            ctrl,
            "WheelScrollDown",
            Action::MoveToWorkspaceNext,
            Some(150),
        ),
        (
            ctrl,
            "WheelScrollUp",
            Action::MoveToWorkspacePrev,
            Some(150),
        ),
        (m, "WheelScrollRight", Action::FocusNext, None),
        (m, "WheelScrollLeft", Action::FocusPrev, None),
        (ctrl, "WheelScrollRight", Action::MoveRight, None),
        (ctrl, "WheelScrollLeft", Action::MoveLeft, None),
        (shift, "WheelScrollDown", Action::FocusNext, None),
        (shift, "WheelScrollUp", Action::FocusPrev, None),
        (ctrl_shift, "WheelScrollDown", Action::MoveRight, None),
        (ctrl_shift, "WheelScrollUp", Action::MoveLeft, None),
    ];
    for (mods, key, action, cooldown) in table {
        assert_eq!(s.resolve_key(mods, key), Some(action), "{key} {mods:?}");
        assert_eq!(s.resolve_cooldown_ms(mods, key), cooldown, "{key} {mods:?}");
    }
    Ok(())
}
