//! 値の誤りの診断が、誤りのある prop の属性（`name={値}`）の位置を指すこと（CDL 0.2.1 の `prop_span`）。
//!
//! 期待する位置は、書いたファイル本文から属性の文字列を探して求める（行・列は 1 始まり、列は文字単位）。
//! 診断コードとメッセージは位置の移行で変えない。

use std::fs;
use std::path::Path;

use contrast_policy::{ConfigDiagnostic, ConfigLoader, LoadOutcome};
use contrast_policy_cdl::CdlLoader;

/// `body`（`<Contrast>` の中身）を書いて読み、本文と診断を返す。
fn run(dir: &str, body: &str) -> Result<(String, Vec<ConfigDiagnostic>), String> {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("impl_prop_span")
        .join(dir);
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let path = base.join("config.cdl");
    let text = format!(
        "\"use cdl 1\";\nimport {{ Contrast, Appearance, Tiers, FocusTier, Tier, Layout, Cursor, Workspaces, Keybind }} from \"host:contrast\";\nexport default (\n  <Contrast>\n{body}\n  </Contrast>\n);\n"
    );
    fs::write(&path, &text).map_err(|e| e.to_string())?;
    match CdlLoader::new().load(&path) {
        LoadOutcome::Failed(d) => Ok((text, d)),
        other => Err(format!("診断を期待したが {}", describe(&other))),
    }
}

/// 読み込み結果の短い説明。
fn describe(o: &LoadOutcome) -> &'static str {
    match o {
        LoadOutcome::Loaded(_) => "成功",
        LoadOutcome::Failed(_) => "失敗",
        LoadOutcome::Absent => "ファイルなし",
    }
}

/// `text` の中で `needle` が最初に現れる位置の (行, 列)。
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

/// 唯一の診断が `code` で、位置が `needle` の属性であること。
fn assert_single_at(dir: &str, body: &str, code: &str, needle: &str) -> Result<(), String> {
    let (text, diags) = run(dir, body)?;
    let want = position_of(&text, needle)?;
    let got: Vec<_> = diags
        .iter()
        .map(|d| (d.code.as_str(), d.line.zip(d.column)))
        .collect();
    assert_eq!(got, [(code, Some(want))], "{dir}: {diags:?}");
    Ok(())
}

#[test]
fn tier_l_out_of_range_points_at_the_l_attribute() -> Result<(), String> {
    assert_single_at(
        "tier-l",
        "    <Tiers>\n      <FocusTier l={1} />\n      <Tier l={0} />\n    </Tiers>",
        "CT060",
        "l={0}",
    )
}

#[test]
fn tier_name_duplicate_points_at_the_name_attribute() -> Result<(), String> {
    assert_single_at(
        "tier-name",
        "    <Tiers>\n      <FocusTier l={1} />\n      <Tier l={0.5} name=\"\" />\n    </Tiers>",
        "CT065",
        "name=\"\"",
    )
}

#[test]
fn layout_range_error_points_at_the_prop() -> Result<(), String> {
    assert_single_at(
        "layout-gap",
        "    <Layout gap={px(9999)} />",
        "CT003",
        "gap={px(9999)}",
    )
}

#[test]
fn cursor_size_points_at_the_prop_not_the_element() -> Result<(), String> {
    assert_single_at(
        "cursor-size",
        "    <Cursor theme=\"x\" size={1} />",
        "CT003",
        "size={1}",
    )
}

#[test]
fn bad_workspace_count_points_at_count() -> Result<(), String> {
    assert_single_at(
        "ws-count",
        "    <Workspaces count={0} />",
        "CT010",
        "count={0}",
    )
}

#[test]
fn unknown_action_points_at_the_action_attribute() -> Result<(), String> {
    assert_single_at(
        "action",
        "    <Keybind keys=\"Mod+X\" action=\"nope\" />",
        "CT020",
        "action=\"nope\"",
    )
}

#[test]
fn keybind_workspace_out_of_range_points_at_workspace() -> Result<(), String> {
    assert_single_at(
        "kb-ws",
        "    <Workspaces count={2} />\n    <Keybind keys=\"Mod+X\" action=\"workspace\" workspace={9} />",
        "CT022",
        "workspace={9}",
    )
}

#[test]
fn a_missing_prop_keeps_the_element_position() -> Result<(), String> {
    // `workspace` を書いていないので、prop の位置が無く要素の位置のまま。
    assert_single_at(
        "kb-missing",
        "    <Keybind keys=\"Mod+X\" action=\"workspace\" />",
        "CT022",
        "<Keybind",
    )
}
