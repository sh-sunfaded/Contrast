//! 列挙の prop（`modKey`・`Layout.mode`・`Startup.ipc`・`Workspaces.mode`）が、
//! CDL のリテラルのユニオンで検査されること。有効な値は通り、範囲外は E4004（評価前の型の誤り）で、
//! メッセージにユニオンが出る。位置は誤りのある prop。

use std::fs;
use std::path::Path;

use contrast_policy::{ConfigDiagnostic, ConfigLoader, LoadOutcome, Settings};
use contrast_policy_cdl::CdlLoader;

/// `body`（`<Contrast>` の中身）と `<Contrast>` の属性を書いて読む。
fn run(
    dir: &str,
    attrs: &str,
    body: &str,
) -> Result<(String, Result<Settings, Vec<ConfigDiagnostic>>), String> {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("impl_literal")
        .join(dir);
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let path = base.join("config.cdl");
    let text = format!(
        "\"use cdl 1\";\nimport {{ Contrast, Layout, Startup, Workspaces }} from \"host:contrast\";\nexport default (\n  <Contrast{attrs}>\n{body}\n  </Contrast>\n);\n"
    );
    fs::write(&path, &text).map_err(|e| e.to_string())?;
    let r = match CdlLoader::new().load(&path) {
        LoadOutcome::Loaded(c) => Ok(c.settings),
        LoadOutcome::Failed(d) => Err(d),
        LoadOutcome::Absent => return Err("ファイルなし".to_owned()),
    };
    Ok((text, r))
}

/// `text` で `needle` が最初に現れる位置の (行, 列)（1 始まり、列は文字単位）。
fn position_of(text: &str, needle: &str) -> Result<(u32, u32), String> {
    let at = text
        .find(needle)
        .ok_or_else(|| format!("{needle:?} が本文に無い"))?;
    let before = text.get(..at).ok_or("境界")?;
    let line = before.matches('\n').count().saturating_add(1);
    let col = before
        .rsplit('\n')
        .next()
        .map_or(0, |l| l.chars().count())
        .saturating_add(1);
    Ok((
        u32::try_from(line).map_err(|e| e.to_string())?,
        u32::try_from(col).map_err(|e| e.to_string())?,
    ))
}

/// 範囲外の値が、唯一の診断 E4004 になる。メッセージに有効な値のユニオンが出て、位置は `at_needle`。
fn assert_e4004(
    dir: &str,
    attrs: &str,
    body: &str,
    at_needle: &str,
    members: &[&str],
) -> Result<(), String> {
    let (text, r) = run(dir, attrs, body)?;
    let diags = r.err().ok_or("診断を期待した")?;
    let got: Vec<_> = diags
        .iter()
        .map(|d| (d.code.as_str(), d.line.zip(d.column)))
        .collect();
    assert_eq!(
        got,
        [("E4004", Some(position_of(&text, at_needle)?))],
        "{dir}: {diags:?}"
    );
    let message = diags.first().map(|d| d.message.as_str()).unwrap_or("");
    for m in members {
        assert!(
            message.contains(m),
            "{dir}: {m:?} がメッセージに無い: {message}"
        );
    }
    Ok(())
}

#[test]
fn mod_key_accepts_alt_and_super_and_rejects_the_rest() -> Result<(), String> {
    use contrast_policy::ModKey;
    for (dir, v, want) in [("a", "alt", ModKey::Alt), ("s", "super", ModKey::Super)] {
        let (_, r) = run(&format!("mod-{dir}"), &format!(" modKey=\"{v}\""), "")?;
        assert_eq!(r.map_err(|d| format!("{d:?}"))?.mod_key, want);
    }
    assert_e4004(
        "mod-bad",
        " modKey=\"hyper\"",
        "",
        "\"hyper\"",
        &["alt", "super"],
    )
}

#[test]
fn layout_mode_is_an_enumeration() -> Result<(), String> {
    use contrast_core::LayoutMode;
    for (dir, v, want) in [
        ("e", "equal", LayoutMode::Equal),
        ("s", "scrolling", LayoutMode::Scrolling),
    ] {
        let (_, r) = run(
            &format!("lm-{dir}"),
            "",
            &format!("    <Layout mode=\"{v}\" />"),
        )?;
        assert_eq!(r.map_err(|d| format!("{d:?}"))?.layout.mode, want);
    }
    assert_e4004(
        "lm-bad",
        "",
        "    <Layout mode=\"grid\" />",
        "\"grid\"",
        &["equal", "scrolling"],
    )
}

#[test]
fn startup_ipc_is_an_enumeration() -> Result<(), String> {
    let (_, r) = run("ipc-ok", "", "    <Startup command=\"x\" ipc=\"niri\" />")?;
    assert_eq!(r.map_err(|d| format!("{d:?}"))?.startup.len(), 1);
    assert_e4004(
        "ipc-bad",
        "",
        "    <Startup command=\"x\" ipc=\"sway\" />",
        "\"sway\"",
        &["niri"],
    )
}

#[test]
fn workspaces_mode_is_an_enumeration() -> Result<(), String> {
    use contrast_core::WorkspaceMode;
    for (dir, v, want) in [
        ("f", "fixed", WorkspaceMode::Fixed),
        ("d", "dynamic", WorkspaceMode::Dynamic),
    ] {
        let (_, r) = run(
            &format!("wm-{dir}"),
            "",
            &format!("    <Workspaces count={{4}} mode=\"{v}\" />"),
        )?;
        assert_eq!(r.map_err(|d| format!("{d:?}"))?.workspace_mode, want);
    }
    assert_e4004(
        "wm-bad",
        "",
        "    <Workspaces count={4} mode=\"static\" />",
        "\"static\"",
        &["fixed", "dynamic"],
    )
}

/// 設定の中で列挙の値を変数や配列に通すとき、型注釈を `string` にすると E4004 になる（CDL の型は
/// リテラルのユニオンで、`string` はそれに代入できない）。`config/startup.cdl` が該当していた。
#[test]
fn a_string_typed_value_is_rejected_but_a_literal_typed_one_is_accepted() -> Result<(), String> {
    let doc = |ty: &str| {
        format!(
            "const xs: {{ command: string; ipc?: {ty} }}[] = [{{ command: \"a\", ipc: \"niri\" }}];\nexport const startup = xs.map((s) => <Startup command={{s.command}} ipc={{s.ipc}} />);"
        )
    };
    for (dir, ty, ok) in [
        ("typed-string", "string", false),
        ("typed-literal", "\"niri\"", true),
    ] {
        let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("impl_literal")
            .join(dir);
        fs::create_dir_all(&base).map_err(|e| e.to_string())?;
        let path = base.join("config.cdl");
        let text = format!(
            "\"use cdl 1\";\nimport {{ Contrast, Startup }} from \"host:contrast\";\n{}\nexport default (\n  <Contrast>\n    {{startup}}\n  </Contrast>\n);\n",
            doc(ty)
        );
        fs::write(&path, text).map_err(|e| e.to_string())?;
        let r = CdlLoader::new().load(&path);
        match (ok, r) {
            (true, LoadOutcome::Loaded(c)) => assert_eq!(c.settings.startup.len(), 1),
            (false, LoadOutcome::Failed(d)) => {
                assert_eq!(
                    d.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
                    ["E4004"]
                );
            }
            (_, other) => return Err(format!("{dir}: 想定外 {}", describe(&other))),
        }
    }
    Ok(())
}

/// 読み込み結果の短い説明。
fn describe(o: &LoadOutcome) -> &'static str {
    match o {
        LoadOutcome::Loaded(_) => "成功",
        LoadOutcome::Failed(_) => "失敗",
        LoadOutcome::Absent => "ファイルなし",
    }
}
