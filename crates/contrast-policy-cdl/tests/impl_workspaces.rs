//! `Workspaces` の `mode`（固定／動的なワークスペース）と、列ごとの移動・ワークスペースの入れ替えの action の変換のテスト。

use std::fs;
use std::path::{Path, PathBuf};

use contrast_core::WorkspaceMode;
use contrast_policy::{Action, ConfigLoader, LoadOutcome, ModKey, Settings};
use contrast_policy_cdl::CdlLoader;

/// 一時ファイルを書いて、そのパスを返す。`body` は `<Contrast>` の中身。テスト名でディレクトリを分ける。
fn write_temp(dir: &str, body: &str) -> Result<PathBuf, String> {
    let test = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .to_owned();
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("impl_workspaces")
        .join(test)
        .join(dir);
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let path = base.join("config.cdl");
    let text = format!(
        "\"use cdl 1\";\nimport {{ Contrast, Workspaces, Keybind }} from \"host:contrast\";\nexport default (\n  <Contrast>\n{body}\n  </Contrast>\n);\n"
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

#[test]
fn the_mode_defaults_to_fixed_and_dynamic_is_read() -> Result<(), String> {
    let fixed = load("default", r#"<Workspaces count={9} />"#)?.map_err(|d| format!("{d:?}"))?;
    assert_eq!(fixed.workspace_mode, WorkspaceMode::Fixed);
    let explicit =
        load("fixed", r#"<Workspaces count={9} mode="fixed" />"#)?.map_err(|d| format!("{d:?}"))?;
    assert_eq!(explicit.workspace_mode, WorkspaceMode::Fixed);
    let dynamic = load("dynamic", r#"<Workspaces count={9} mode="dynamic" />"#)?
        .map_err(|d| format!("{d:?}"))?;
    assert_eq!(dynamic.workspace_mode, WorkspaceMode::Dynamic);
    // `count` は動的でも、`workspace` の番号の上限として読む。
    assert_eq!(dynamic.workspaces, 9);
    Ok(())
}

#[test]
fn an_unknown_mode_is_ct003() -> Result<(), String> {
    for (dir, value) in [("empty", ""), ("upper", "Dynamic"), ("other", "static")] {
        let body = format!(r#"<Workspaces count={{9}} mode="{value}" />"#);
        assert_eq!(
            load(dir, &body)?.err(),
            Some(vec!["CT003".to_owned()]),
            "{value:?}"
        );
    }
    Ok(())
}

#[test]
fn the_new_actions_are_converted() -> Result<(), String> {
    let body = r#"
    <Workspaces count={9} mode="dynamic" />
    <Keybind keys="mod+1" action="move-column-to-workspace" workspace={3} />
    <Keybind keys="mod+2" action="move-column-to-workspace-next" />
    <Keybind keys="mod+3" action="move-column-to-workspace-prev" />
    <Keybind keys="mod+4" action="move-workspace-down" />
    <Keybind keys="mod+5" action="move-workspace-up" />"#;
    let s = load("actions", body)?.map_err(|d| format!("{d:?}"))?;
    let m = ModKey::Super.mods();
    let got: Vec<Option<Action>> = ["1", "2", "3", "4", "5"]
        .iter()
        .map(|k| s.resolve_key(m, k))
        .collect();
    assert_eq!(
        got,
        [
            Some(Action::MoveColumnToWorkspace(2)),
            Some(Action::MoveColumnToWorkspaceNext),
            Some(Action::MoveColumnToWorkspacePrev),
            Some(Action::MoveWorkspaceDown),
            Some(Action::MoveWorkspaceUp),
        ]
    );
    Ok(())
}

#[test]
fn move_column_to_workspace_needs_a_valid_workspace() -> Result<(), String> {
    // 番号が無い・範囲外は CT022（`workspace` と同じ検査）。
    for (dir, props) in [
        ("none", ""),
        ("zero", " workspace={0}"),
        ("over", " workspace={10}"),
    ] {
        let body = format!(
            r#"<Workspaces count={{9}} /><Keybind keys="mod+1" action="move-column-to-workspace"{props} />"#
        );
        assert_eq!(
            load(dir, &body)?.err(),
            Some(vec!["CT022".to_owned()]),
            "{props}"
        );
    }
    Ok(())
}

#[test]
fn move_column_to_workspace_with_focus_false_does_not_switch() -> Result<(), String> {
    let body = r#"
    <Workspaces count={9} mode="dynamic" />
    <Keybind keys="mod+1" action="move-column-to-workspace" workspace={3} focus={false} />
    <Keybind keys="mod+2" action="move-column-to-workspace" workspace={3} focus={true} />"#;
    let s = load("focus", body)?.map_err(|d| format!("{d:?}"))?;
    let m = ModKey::Super.mods();
    assert_eq!(
        s.resolve_key(m, "1"),
        Some(Action::MoveColumnToWorkspaceNoFocus(2))
    );
    assert_eq!(
        s.resolve_key(m, "2"),
        Some(Action::MoveColumnToWorkspace(2))
    );
    Ok(())
}
