//! `Tiers` / `FocusTier` / `Tier`（CC-3）の変換のテスト: 通る入力と、診断のコード・位置（行・列）。
//!
//! 位置は要素単位（props に位置は無い）。本文は `<Contrast>` の中身で、ファイルの 5 行目から始まる
//! （1 行目 `"use cdl 1";`、2 行目 import、3 行目 `export default (`、4 行目 `<Contrast>`）。

use std::fs;
use std::path::{Path, PathBuf};

use contrast_policy::{ConfigLoader, LoadOutcome, Settings};
use contrast_policy_cdl::CdlLoader;

/// 診断 1 件: (コード, 行, 列)。
type Diag = (String, Option<u32>, Option<u32>);

/// 一時ファイルを書いて、そのパスを返す。`body` は `<Contrast>` の中身。
fn write_temp(dir: &str, body: &str) -> Result<PathBuf, String> {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("impl_tiers")
        .join(dir);
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let path = base.join("config.cdl");
    let text = format!(
        "\"use cdl 1\";\nimport {{ Contrast, Appearance, Tiers, FocusTier, Tier }} from \"host:contrast\";\nexport default (\n  <Contrast>\n{body}\n  </Contrast>\n);\n"
    );
    fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(path)
}

/// 読み込んで、設定か診断の並びを返す。
fn load(dir: &str, body: &str) -> Result<Result<Settings, Vec<Diag>>, String> {
    let path = write_temp(dir, body)?;
    Ok(match CdlLoader::new().load(&path) {
        LoadOutcome::Loaded(c) => Ok(c.settings),
        LoadOutcome::Failed(d) => Err(d
            .iter()
            .map(|d| (d.code.clone(), d.line, d.column))
            .collect()),
        LoadOutcome::Absent => Err(vec![("absent".to_owned(), None, None)]),
    })
}

/// 診断のコードだけの並び。
fn codes(r: &Result<Settings, Vec<Diag>>) -> Vec<&str> {
    match r {
        Ok(_) => Vec::new(),
        Err(d) => d.iter().map(|(c, _, _)| c.as_str()).collect(),
    }
}

#[test]
fn omitted_tiers_use_the_builtin() -> Result<(), String> {
    let s = load("omitted", "")?.map_err(|d| format!("{d:?}"))?;
    assert_eq!(s.appearance.tiers, Settings::builtin().appearance.tiers);
    Ok(())
}

#[test]
fn a_focus_tier_and_one_tier_are_read() -> Result<(), String> {
    let body = r##"    <Tiers>
      <FocusTier l={1} ringColor={color("#7fc8ff")} />
      <Tier l={0.8} name="dim" />
    </Tiers>"##;
    let s = load("ok", body)?.map_err(|d| format!("{d:?}"))?;
    let t = &s.appearance.tiers;
    assert!((t.focus.l - 1.0).abs() < 1e-6);
    assert!(t.focus.ring_color.is_some());
    assert_eq!(t.rest.len(), 1);
    assert!(t.rest.first().is_some_and(|x| (x.l - 0.8).abs() < 1e-6));
    assert_eq!(t.unfocused_ring(), None);
    assert!(t.unfocused_dim() < 1.0);
    Ok(())
}

#[test]
fn two_tiers_are_ct064_at_the_tiers_element() -> Result<(), String> {
    // DU-12 は満たす 2 個（差 0.5 > 幅 0.05）: CT064 だけ。
    let body = "    <Tiers>\n      <FocusTier l={1} />\n      <Tier l={0.5} />\n      <Tier l={0.45} />\n    </Tiers>";
    let r = load("two", body)?;
    assert_eq!(codes(&r), ["CT064"]);
    // `Tiers` は 5 行目の 5 列目。
    assert_eq!(
        r.err().and_then(|d| d.first().cloned()),
        Some(("CT064".to_owned(), Some(5), Some(5)))
    );
    // DU-12 も破る 2 個（差 0.2 <= 幅 0.2）: CT064 と CT063（CT063 は FocusTier の位置）。
    let body = "    <Tiers>\n      <FocusTier l={1} />\n      <Tier l={0.8} />\n      <Tier l={0.6} />\n    </Tiers>";
    let r = load("two_spread", body)?;
    assert_eq!(codes(&r), ["CT064", "CT063"]);
    assert_eq!(
        r.err().and_then(|d| d.get(1).cloned()),
        Some(("CT063".to_owned(), Some(6), Some(7)))
    );
    Ok(())
}

#[test]
fn a_brighter_tier_than_focus_is_ct062_at_that_tier() -> Result<(), String> {
    let body = "    <Tiers>\n      <FocusTier l={0.8} />\n      <Tier l={0.9} />\n    </Tiers>";
    let r = load("brighter", body)?;
    assert_eq!(codes(&r), ["CT062"]);
    assert_eq!(
        r.err().and_then(|d| d.first().cloned()),
        Some(("CT062".to_owned(), Some(7), Some(7)))
    );
    Ok(())
}

#[test]
fn equal_l_is_ct062() -> Result<(), String> {
    let body = "    <Tiers>\n      <FocusTier l={1} />\n      <Tier l={1} />\n    </Tiers>";
    assert_eq!(codes(&load("equal", body)?), ["CT062"]);
    Ok(())
}

#[test]
fn out_of_range_l_is_ct060_at_the_element() -> Result<(), String> {
    for (dir, focus, rest, line) in [
        ("zero", "1", "0", 7),
        ("big", "1.2", "0.8", 6),
        ("neg", "1", "-0.5", 7),
    ] {
        let body = format!(
            "    <Tiers>\n      <FocusTier l={{{focus}}} />\n      <Tier l={{{rest}}} />\n    </Tiers>"
        );
        let r = load(dir, &body)?;
        assert_eq!(codes(&r), ["CT060"], "{dir}");
        assert_eq!(
            r.err().and_then(|d| d.first().cloned()),
            Some(("CT060".to_owned(), Some(line), Some(7))),
            "{dir}"
        );
    }
    Ok(())
}

#[test]
fn a_string_l_is_a_cdl_type_diagnostic() -> Result<(), String> {
    let body = "    <Tiers>\n      <FocusTier l=\"1\" />\n      <Tier l={0.8} />\n    </Tiers>";
    let r = load("string", body)?;
    let c = codes(&r);
    assert!(!c.is_empty(), "型の誤りは診断になる");
    assert!(
        c.iter().all(|c| !c.starts_with("CT06")),
        "CT06x ではなく CDL の診断: {c:?}"
    );
    Ok(())
}

#[test]
fn a_missing_focus_tier_is_ct061() -> Result<(), String> {
    let body = "    <Tiers>\n      <Tier l={0.8} />\n    </Tiers>";
    let r = load("nofocus", body)?;
    assert_eq!(codes(&r), ["CT061"]);
    assert_eq!(
        r.err().and_then(|d| d.first().cloned()),
        Some(("CT061".to_owned(), Some(5), Some(5)))
    );
    Ok(())
}

#[test]
fn duplicate_unique_children_are_rejected_by_cdl() -> Result<(), String> {
    let two_focus = "    <Tiers>\n      <FocusTier l={1} />\n      <FocusTier l={1} />\n      <Tier l={0.8} />\n    </Tiers>";
    let r = load("two_focus", two_focus)?;
    assert_eq!(codes(&r), ["E4006"], "FocusTier が 2 個");
    let two_tiers = "    <Tiers>\n      <FocusTier l={1} />\n      <Tier l={0.8} />\n    </Tiers>\n    <Tiers>\n      <FocusTier l={1} />\n      <Tier l={0.8} />\n    </Tiers>";
    let r = load("two_tiers", two_tiers)?;
    assert_eq!(codes(&r), ["E4006"], "Tiers が 2 個");
    Ok(())
}

#[test]
fn an_empty_or_duplicate_name_is_ct065() -> Result<(), String> {
    let body =
        "    <Tiers>\n      <FocusTier l={1} />\n      <Tier l={0.8} name=\"\" />\n    </Tiers>";
    assert_eq!(codes(&load("empty_name", body)?), ["CT065"]);
    Ok(())
}
