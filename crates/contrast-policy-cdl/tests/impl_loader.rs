//! `CdlLoader` の結合テスト: 実ファイルを読み、設定値・診断・フックの結果を確かめる。
//!
//! 一時ファイルは `CARGO_TARGET_TMPDIR` の下にテストごとのディレクトリを作って置く。

use std::fs;
use std::path::{Path, PathBuf};

use contrast_core::{WindowMeta, WindowState};
use contrast_policy::{
    Config, ConfigDiagnostic, ConfigLoader, LoadOutcome, Settings, default_placement,
};
use contrast_policy_cdl::CdlLoader;

/// このテスト用のディレクトリを作り、そこへ `files`（名前, 本文）を書く。最初のファイルのパスを返す。
fn write_files(dir: &str, files: &[(&str, &str)]) -> Result<PathBuf, String> {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("impl_loader")
        .join(dir);
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let mut first = None;
    for (name, text) in files {
        let path = base.join(name);
        fs::write(&path, text).map_err(|e| e.to_string())?;
        first.get_or_insert(path);
    }
    first.ok_or_else(|| "no files".to_owned())
}

/// リポジトリの `examples/` のファイル。
fn example(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name)
}

/// 読み込む。ファイルが無いときは診断に見せかけず `Err(vec![])`。
fn load(path: &Path) -> Result<Config, Vec<ConfigDiagnostic>> {
    match CdlLoader::new().load(path) {
        LoadOutcome::Loaded(c) => Ok(*c),
        LoadOutcome::Failed(d) => Err(d),
        LoadOutcome::Absent => Err(Vec::new()),
    }
}

/// `app_id` と `is_dialog` だけ決めた窓のメタ情報（`WindowMeta` は non_exhaustive）。
fn window(app: &str, is_dialog: bool) -> WindowMeta {
    let mut meta = WindowMeta::default();
    meta.app_id = Some(app.to_owned());
    meta.is_dialog = is_dialog;
    meta
}

/// 診断のコードの並び。
fn codes(diags: &[ConfigDiagnostic]) -> Vec<&str> {
    diags.iter().map(|d| d.code.as_str()).collect()
}

/// 最小の設定ファイル（`body` は `<Contrast>` の中身）。
fn doc(head: &str, body: &str) -> String {
    format!(
        "\"use cdl 1\";\n\
         import {{ Contrast, Appearance, Workspaces, Keybind }} from \"host:contrast\";\n\
         import type {{ Policy, WindowInfo, Placement }} from \"host:contrast\";\n\
         {head}\n\
         export default (\n  <Contrast>\n{body}\n  </Contrast>\n);\n"
    )
}

#[test]
fn default_example_equals_builtin() -> Result<(), String> {
    let config = load(&example("default.cdl")).map_err(|d| format!("{d:?}"))?;
    assert_eq!(config.settings, Settings::builtin());
    Ok(())
}

#[test]
fn config_example_loads_and_window_rules_apply() -> Result<(), String> {
    let config = load(&example("config.cdl")).map_err(|d| format!("{d:?}"))?;
    assert_eq!(config.settings.workspaces, 6);
    assert_eq!(config.settings.appearance.border_width, 4);
    assert!(config.settings.appearance.border_color.is_some());
    let meta = |app: &str, dialog: bool| window(app, dialog);

    let (p, d) = config.place(&meta("firefox", false), Some(0));
    assert_eq!(d, None);
    assert_eq!(
        (p.workspace, p.state, p.focus),
        (Some(1), WindowState::Tiled, true)
    );

    let (p, d) = config.place(&meta("pavucontrol", false), Some(0));
    assert_eq!(d, None);
    assert_eq!((p.workspace, p.state), (None, WindowState::Floating));

    // 既定の分岐: ワークスペースは今のまま（1 始まりで渡って 1 始まりで返る）、ダイアログは Floating。
    let (p, _) = config.place(&meta("foot", true), Some(2));
    assert_eq!((p.workspace, p.state), (Some(2), WindowState::Floating));
    Ok(())
}

#[test]
fn missing_file_is_absent() {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-such-dir/config.cdl");
    assert!(matches!(CdlLoader::new().load(&path), LoadOutcome::Absent));
}

#[test]
fn all_conversion_errors_are_reported_together() -> Result<(), String> {
    let body = [
        r#"<Keybind keys="mod+a" action="frobnicate" />"#, // CT020
        r#"<Keybind keys="mod+b" action="spawn" />"#,      // CT021
        r#"<Keybind keys="mod+c" action="workspace" workspace={9} />"#, // CT022（count は 4）
        r#"<Keybind keys="hyper+d" action="close" />"#,    // CT030
        r#"<Keybind keys="mod+e" action="close" />"#,
        r#"<Keybind keys="alt+E" action="quit" />"#, // CT031（mod は alt。大文字小文字は区別しない）
        r#"<Workspaces count={4} />"#,
    ]
    .map(|line| format!("    {line}"))
    .join("\n");
    let path = write_files("errors", &[("config.cdl", &doc("", &body))])?;
    let diags = load(&path).err().ok_or("must fail")?;
    assert_eq!(
        codes(&diags),
        ["CT020", "CT021", "CT022", "CT030", "CT031"],
        "{diags:?}"
    );
    // 位置は要素の行（本文は 7 行目から 1 行ずつ）。
    let lines: Vec<_> = diags.iter().map(|d| d.line).collect();
    assert_eq!(lines, [7, 8, 9, 10, 12].map(Some));
    assert!(
        diags
            .iter()
            .all(|d| d.file.ends_with("config.cdl") && d.column == Some(5))
    );
    Ok(())
}

#[test]
fn prop_value_errors_have_codes() -> Result<(), String> {
    let cases = [
        (r#"<Contrast modKey="hyper">"#, "CT003"),
        (
            r##"<Appearance borderWidth={px(-1)} focusColor={color("#fff")} />"##,
            "CT003",
        ),
        (
            r##"<Appearance borderWidth={px(1.5)} focusColor={color("#fff")} />"##,
            "CT003",
        ),
        (r#"<Workspaces count={0} />"#, "CT010"),
        (r#"<Workspaces count={33} />"#, "CT010"),
        (r#"<Workspaces count={2.5} />"#, "CT010"),
    ];
    for (i, (line, code)) in cases.iter().enumerate() {
        let text = if let Some(attrs) = line.strip_prefix("<Contrast ") {
            format!(
                "\"use cdl 1\";\nimport {{ Contrast }} from \"host:contrast\";\nexport default (<Contrast {attrs}</Contrast>);\n"
            )
        } else {
            doc("", &format!("    {line}"))
        };
        let path = write_files(&format!("prop{i}"), &[("config.cdl", &text)])?;
        let diags = load(&path)
            .err()
            .ok_or_else(|| format!("case {i} must fail"))?;
        assert_eq!(codes(&diags), [*code], "case {i}: {diags:?}");
    }
    Ok(())
}

#[test]
fn syntax_and_type_errors_come_from_cdl_with_positions() -> Result<(), String> {
    let path = write_files("syntax", &[("config.cdl", "\"use cdl 1\";\nconst x = ;\n")])?;
    let diags = load(&path).err().ok_or("must fail")?;
    assert!(diags.iter().all(|d| d.code.starts_with('E')), "{diags:?}");
    assert!(diags.iter().any(|d| d.line == Some(2)), "{diags:?}");
    Ok(())
}

#[test]
fn import_outside_config_dir_is_rejected() -> Result<(), String> {
    write_files(
        "outside",
        &[("secret.cdl", "\"use cdl 1\";\nexport const X = 1;\n")],
    )?;
    let path = write_files(
        "outside/inner",
        &[(
            "config.cdl",
            &doc(
                "import { X } from \"../secret.cdl\";",
                "    <Workspaces count={4} />",
            ),
        )],
    )?;
    let diags = load(&path).err().ok_or("must fail")?;
    assert!(
        diags.iter().any(|d| d.message.contains("外は読めない")),
        "{diags:?}"
    );
    Ok(())
}

#[test]
fn import_inside_config_dir_works() -> Result<(), String> {
    let path = write_files(
        "inside",
        &[
            (
                "config.cdl",
                &doc(
                    "import { N } from \"./n.cdl\";",
                    "    <Workspaces count={N} />",
                ),
            ),
            ("n.cdl", "\"use cdl 1\";\nexport const N = 5;\n"),
        ],
    )?;
    let config = load(&path).map_err(|d| format!("{d:?}"))?;
    assert_eq!(config.settings.workspaces, 5);
    Ok(())
}

#[test]
fn invalid_hook_workspace_falls_back_to_default_placement() -> Result<(), String> {
    let head = r#"
export const policy = {
  windowOpen: (w: WindowInfo): Placement => {
    switch (w.appId) {
      case "zero": return { workspace: 0, floating: true };
      case "negative": return { workspace: -1, floating: true };
      case "fraction": return { workspace: 1.5, floating: true };
      case "range": return { workspace: 9, floating: true };
      case "ok": return { workspace: 4, floating: true };
      default: return { floating: false };
    }
  },
} satisfies Policy;"#;
    let path = write_files(
        "hook",
        &[("config.cdl", &doc(head, "    <Workspaces count={4} />"))],
    )?;
    let config = load(&path).map_err(|d| format!("{d:?}"))?;
    let meta = |app: &str| window(app, false);
    let fallback = default_placement(&meta("x"));
    for (app, code) in [
        ("zero", "CT042"),
        ("negative", "CT042"),
        ("fraction", "CT042"),
        ("range", "CT043"),
    ] {
        let (p, d) = config.place(&meta(app), Some(0));
        assert_eq!(p, fallback, "{app}");
        assert_eq!(d.map(|d| d.code), Some(code.to_owned()), "{app}");
    }
    let (p, d) = config.place(&meta("ok"), Some(0));
    assert_eq!(d, None);
    assert_eq!((p.workspace, p.state), (Some(3), WindowState::Floating));
    let (p, d) = config.place(&meta("other"), Some(0));
    assert_eq!((p, d), (fallback, None));
    Ok(())
}
