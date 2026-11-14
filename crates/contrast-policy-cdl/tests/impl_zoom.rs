//! U5 段階 (a): `<Zoom>`・`<Step>`・`zoom-*` の `Keybind` の変換のテスト。診断のコードと位置（原因の prop）、
//! `<Zoom>` が無い設定が等倍のみで変わらないこと、`examples/u5-zoom.cdl`。

use std::fs;
use std::path::{Path, PathBuf};

use contrast_policy::{
    Action, ConfigDiagnostic, ConfigLoader, LoadOutcome, Mods, Settings, ZoomLevel,
};
use contrast_policy_cdl::CdlLoader;

const IMPORTS: &str = "import { Contrast, Zoom, Step, Keybind } from \"host:contrast\";";

fn doc(body: &str) -> String {
    format!(
        "\"use cdl 1\";\n{IMPORTS}\nexport default (\n  <Contrast>\n{body}\n  </Contrast>\n);\n"
    )
}

fn write(dir: &str, text: &str) -> Result<PathBuf, String> {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("impl_zoom")
        .join(dir);
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let path = base.join("config.cdl");
    fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(path)
}

fn loaded(dir: &str, body: &str) -> Result<Settings, String> {
    match CdlLoader::new().load(&write(dir, &doc(body))?) {
        LoadOutcome::Loaded(c) => Ok(c.settings),
        LoadOutcome::Failed(d) => Err(format!("{d:?}")),
        LoadOutcome::Absent => Err("ファイルなし".to_owned()),
    }
}

fn failed(dir: &str, body: &str) -> Result<(String, Vec<ConfigDiagnostic>), String> {
    let text = doc(body);
    match CdlLoader::new().load(&write(dir, &text)?) {
        LoadOutcome::Failed(d) => Ok((text, d)),
        LoadOutcome::Loaded(_) => Err("診断を期待した".to_owned()),
        LoadOutcome::Absent => Err("ファイルなし".to_owned()),
    }
}

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

/// 唯一の診断が `code` で位置が `needle`。
fn assert_single(dir: &str, body: &str, code: &str, needle: &str) -> Result<(), String> {
    let (text, d) = failed(dir, body)?;
    let got: Vec<_> = d
        .iter()
        .map(|d| (d.code.as_str(), d.line.zip(d.column)))
        .collect();
    assert_eq!(
        got,
        [(code, Some(position_of(&text, needle)?))],
        "{dir}: {d:?}"
    );
    Ok(())
}

fn divisors(s: &Settings) -> Vec<u8> {
    s.zoom.levels.iter().map(|l| l.divisor()).collect()
}

#[test]
fn without_a_zoom_element_there_is_only_the_identity() -> Result<(), String> {
    let s = loaded("none", "    <Keybind keys=\"alt+q\" action=\"close\" />")?;
    assert_eq!(divisors(&s), [1]);
    assert_eq!(s.zoom, Settings::builtin().zoom);
    Ok(())
}

#[test]
fn steps_follow_the_identity_in_increasing_order() -> Result<(), String> {
    let one = loaded("one", "    <Zoom><Step divisor={2} /></Zoom>")?;
    assert_eq!(divisors(&one), [1, 2]);
    let two = loaded(
        "two",
        "    <Zoom><Step divisor={2} /><Step divisor={4} /></Zoom>",
    )?;
    assert_eq!(divisors(&two), [1, 2, 4]);
    assert_eq!(two.zoom.level(1), ZoomLevel::new(2).ok_or("2")?);
    // 範囲の端（2 と 8）。
    let edge = loaded(
        "edge",
        "    <Zoom><Step divisor={2} /><Step divisor={8} /></Zoom>",
    )?;
    assert_eq!(divisors(&edge), [1, 2, 8]);
    // 段階の無い `<Zoom>` は等倍だけ。
    assert_eq!(divisors(&loaded("empty", "    <Zoom />")?), [1]);
    Ok(())
}

#[test]
fn a_bad_divisor_is_ct086_at_the_divisor_prop() -> Result<(), String> {
    for (dir, value) in [
        ("d1", "1"),
        ("d9", "9"),
        ("d0", "0"),
        ("dneg", "-2"),
        ("dfrac", "2.5"),
    ] {
        let body = format!("    <Zoom><Step divisor={{{value}}} /></Zoom>");
        assert_single(dir, &body, "CT086", &format!("divisor={{{value}}}"))?;
    }
    Ok(())
}

#[test]
fn steps_must_strictly_increase_and_there_are_at_most_two() -> Result<(), String> {
    assert_single(
        "order",
        "    <Zoom><Step divisor={4} /><Step divisor={2} /></Zoom>",
        "CT086",
        "divisor={2}",
    )?;
    assert_single(
        "dup",
        "    <Zoom><Step divisor={2} /><Step divisor={2} /></Zoom>",
        "CT086",
        "divisor={2} /></Zoom>",
    )?;
    // 3 つ目の `Step` は、個数の誤りとして要素の位置。
    let body = "    <Zoom>\n      <Step divisor={2} />\n      <Step divisor={3} />\n      <Step divisor={4} />\n    </Zoom>";
    assert_single("three", body, "CT086", "<Step divisor={4}")?;
    Ok(())
}

#[test]
fn two_zoom_elements_are_rejected_by_cdl() -> Result<(), String> {
    let (_, d) = failed("two-zoom", "    <Zoom />\n    <Zoom />")?;
    assert_eq!(
        d.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["E4006"]
    );
    Ok(())
}

#[test]
fn the_zoom_keys_resolve_when_there_are_steps() -> Result<(), String> {
    let body = "    <Zoom><Step divisor={2} /></Zoom>\n    <Keybind keys=\"alt+minus\" action=\"zoom-in\" />\n    <Keybind keys=\"alt+equal\" action=\"zoom-out\" />\n    <Keybind keys=\"alt+0\" action=\"zoom-reset\" />";
    let s = loaded("keys", body)?;
    let alt = Mods {
        alt: true,
        ..Mods::default()
    };
    assert_eq!(s.resolve_key(alt, "minus"), Some(Action::ZoomIn));
    assert_eq!(s.resolve_key(alt, "equal"), Some(Action::ZoomOut));
    assert_eq!(s.resolve_key(alt, "0"), Some(Action::ZoomReset));
    Ok(())
}

#[test]
fn a_zoom_key_without_steps_is_ct088_at_the_action() -> Result<(), String> {
    for action in ["zoom-in", "zoom-out", "zoom-reset"] {
        let body = format!("    <Keybind keys=\"alt+minus\" action=\"{action}\" />");
        assert_single(
            &format!("no-steps-{action}"),
            &body,
            "CT088",
            &format!("action=\"{action}\""),
        )?;
    }
    // `<Zoom />`（段階なし）でも同じ。
    assert_single(
        "empty-zoom",
        "    <Zoom />\n    <Keybind keys=\"alt+minus\" action=\"zoom-in\" />",
        "CT088",
        "action=\"zoom-in\"",
    )
}

fn example(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name)
}

#[test]
fn the_baseline_example_has_no_zoom() -> Result<(), String> {
    match CdlLoader::new().load(&example("niri.cdl")) {
        LoadOutcome::Loaded(c) => {
            assert_eq!(c.settings.zoom, Settings::builtin().zoom);
            Ok(())
        }
        LoadOutcome::Failed(d) => Err(format!("{d:?}")),
        LoadOutcome::Absent => Err("niri.cdl が無い".to_owned()),
    }
}

#[test]
fn the_zoom_example_loads_with_two_levels_and_the_keys() -> Result<(), String> {
    match CdlLoader::new().load(&example("u5-zoom.cdl")) {
        LoadOutcome::Loaded(c) => {
            assert_eq!(divisors(&c.settings), [1, 2]);
            let has = |a: Action| c.settings.keybinds.iter().any(|b| b.action == a);
            assert!(has(Action::ZoomIn) && has(Action::ZoomOut) && has(Action::ZoomReset));
            Ok(())
        }
        LoadOutcome::Failed(d) => Err(format!("{d:?}")),
        LoadOutcome::Absent => Err("u5-zoom.cdl が無い".to_owned()),
    }
}
