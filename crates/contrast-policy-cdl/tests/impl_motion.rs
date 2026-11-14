//! `Motion`（U3 段階 (a)）の変換のテスト: 通る入力、診断のコードと位置（原因の prop）、旧 prop との併存の警告。
//!
//! 期待する位置は、書いたファイル本文から属性の文字列を探して求める。

use std::fs;
use std::path::Path;

use contrast_policy::{
    ConfigDiagnostic, ConfigLoader, EasingCurve, LoadOutcome, MotionCurve, MotionKind, MotionSpec,
};
use contrast_policy_cdl::CdlLoader;

/// 読み込んだ結果: 設定の動きと警告、または診断。
type Loaded = Result<
    (
        Option<contrast_policy::MotionSettings>,
        Vec<ConfigDiagnostic>,
    ),
    Vec<ConfigDiagnostic>,
>;

/// `body`（`<Contrast>` の中身）を書いて読む。本文のテキストも返す。
fn run(dir: &str, body: &str) -> Result<(String, Loaded), String> {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("impl_motion")
        .join(dir);
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let path = base.join("config.cdl");
    let text = format!(
        "\"use cdl 1\";\nimport {{ Contrast, Layout, Motion, Scroll, Resize, Workspace, Focus, Open, Close }} from \"host:contrast\";\nexport default (\n  <Contrast>\n{body}\n  </Contrast>\n);\n"
    );
    fs::write(&path, &text).map_err(|e| e.to_string())?;
    let r = match CdlLoader::new().load(&path) {
        LoadOutcome::Loaded(c) => Ok((c.settings.motion, c.warnings.clone())),
        LoadOutcome::Failed(d) => Err(d),
        LoadOutcome::Absent => return Err("ファイルなし".to_owned()),
    };
    Ok((text, r))
}

/// `text` で `needle` が最初に現れる位置の (行, 列)。
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

/// 動き（`<Motion>` の中身）を読んで成功するもの。
fn motion_of(dir: &str, inner: &str) -> Result<contrast_policy::MotionSettings, String> {
    let body = format!("    <Motion>\n{inner}\n    </Motion>");
    let (_, r) = run(dir, &body)?;
    let (m, _) = r.map_err(|d| format!("{d:?}"))?;
    m.ok_or_else(|| "motion が None".to_owned())
}

/// 唯一の診断が `code` で、位置が `needle`。
fn assert_single(dir: &str, body: &str, code: &str, needle: &str) -> Result<(), String> {
    let (text, r) = run(dir, body)?;
    let diags = r.err().ok_or("診断を期待した")?;
    let got: Vec<_> = diags
        .iter()
        .map(|d| (d.code.as_str(), d.line.zip(d.column)))
        .collect();
    assert_eq!(
        got,
        [(code, Some(position_of(&text, needle)?))],
        "{dir}: {diags:?}"
    );
    Ok(())
}

fn easing(c: EasingCurve, ms: u32) -> Option<MotionSpec> {
    Some(MotionSpec {
        curve: MotionCurve::Easing(c),
        duration_ms: ms,
    })
}

#[test]
fn no_motion_element_means_none() -> Result<(), String> {
    let (_, r) = run("none", "")?;
    let (m, w) = r.map_err(|d| format!("{d:?}"))?;
    assert_eq!(m, None);
    assert!(w.is_empty());
    Ok(())
}

#[test]
fn an_empty_motion_is_some_and_every_kind_is_instant() -> Result<(), String> {
    let m = motion_of("empty", "")?;
    for k in [
        MotionKind::Scroll,
        MotionKind::Resize,
        MotionKind::Workspace,
        MotionKind::Focus,
        MotionKind::Open,
        MotionKind::Close,
    ] {
        assert_eq!(m.spec(k), None, "{k:?}");
    }
    Ok(())
}

#[test]
fn omitted_props_take_the_kind_defaults() -> Result<(), String> {
    let m = motion_of(
        "defaults",
        "<Scroll /><Resize /><Workspace /><Focus /><Open /><Close />",
    )?;
    for (k, ms) in [
        (MotionKind::Scroll, 150),
        (MotionKind::Resize, 150),
        (MotionKind::Workspace, 150),
        (MotionKind::Focus, 120),
        (MotionKind::Open, 150),
        (MotionKind::Close, 150),
    ] {
        assert_eq!(m.spec(k), easing(EasingCurve::EaseOutCubic, ms), "{k:?}");
    }
    Ok(())
}

#[test]
fn curves_and_durations_are_read_per_kind() -> Result<(), String> {
    let m = motion_of(
        "read",
        "<Scroll curve=\"easeOutQuad\" duration={ms(200)} /><Focus curve=\"easeInOutCubic\" duration={ms(90)} /><Open curve=\"easeOutExpo\" />",
    )?;
    assert_eq!(
        m.spec(MotionKind::Scroll),
        easing(EasingCurve::EaseOutQuad, 200)
    );
    assert_eq!(
        m.spec(MotionKind::Focus),
        easing(EasingCurve::EaseInOutCubic, 90)
    );
    assert_eq!(
        m.spec(MotionKind::Open),
        easing(EasingCurve::EaseOutExpo, 150)
    );
    assert_eq!(m.spec(MotionKind::Resize), None);
    Ok(())
}

#[test]
fn niri_spring_has_no_duration_and_is_allowed_on_rect_kinds() -> Result<(), String> {
    let m = motion_of(
        "spring",
        "<Scroll curve=\"niriSpring\" /><Resize curve=\"niriSpring\" /><Workspace curve=\"niriSpring\" /><Open curve=\"niriSpring\" /><Close curve=\"niriSpring\" />",
    )?;
    let want = Some(MotionSpec {
        curve: MotionCurve::NiriSpring,
        duration_ms: 0,
    });
    for k in [
        MotionKind::Scroll,
        MotionKind::Resize,
        MotionKind::Workspace,
        MotionKind::Open,
        MotionKind::Close,
    ] {
        assert_eq!(m.spec(k), want, "{k:?}");
    }
    Ok(())
}

#[test]
fn duration_boundaries_and_equivalent_spellings() -> Result<(), String> {
    for (dir, expr, ms) in [
        ("d0", "ms(0)", 0),
        ("d1", "ms(1)", 1),
        ("d399", "ms(399)", 399),
        ("d400", "ms(400)", 400),
        ("sec", "sec(0.15)", 150),
    ] {
        let m = motion_of(dir, &format!("<Scroll duration={{{expr}}} />"))?;
        assert_eq!(
            m.spec(MotionKind::Scroll),
            easing(EasingCurve::EaseOutCubic, ms),
            "{expr}"
        );
    }
    // sec(0.15) と ms(150) は同じ設定になる。
    assert_eq!(
        motion_of("sec2", "<Scroll duration={sec(0.15)} />")?,
        motion_of("ms2", "<Scroll duration={ms(150)} />")?
    );
    Ok(())
}

#[test]
fn duration_over_the_limit_or_negative_is_ct070_at_the_prop() -> Result<(), String> {
    for (dir, expr) in [
        ("d401", "ms(401)"),
        ("d2000", "ms(2000)"),
        ("neg", "ms(-5)"),
    ] {
        let body = format!("    <Motion>\n      <Scroll duration={{{expr}}} />\n    </Motion>");
        assert_single(dir, &body, "CT070", &format!("duration={{{expr}}}"))?;
    }
    Ok(())
}

#[test]
fn every_offending_element_is_reported_once_at_its_own_prop() -> Result<(), String> {
    let body = "    <Motion>\n      <Scroll duration={ms(500)} />\n      <Resize duration={ms(100)} />\n      <Focus duration={ms(900)} />\n    </Motion>";
    let (text, r) = run("two-bad", body)?;
    let diags = r.err().ok_or("診断を期待した")?;
    let got: Vec<_> = diags
        .iter()
        .map(|d| (d.code.as_str(), d.line.zip(d.column)))
        .collect();
    assert_eq!(
        got,
        [
            ("CT070", Some(position_of(&text, "duration={ms(500)}")?)),
            ("CT070", Some(position_of(&text, "duration={ms(900)}")?)),
        ]
    );
    Ok(())
}

#[test]
fn an_unknown_curve_is_ct071_at_the_curve_prop() -> Result<(), String> {
    assert_single(
        "curve",
        "    <Motion>\n      <Resize curve=\"linear\" />\n    </Motion>",
        "CT071",
        "curve=\"linear\"",
    )
}

#[test]
fn spring_with_duration_is_ct072_at_the_element() -> Result<(), String> {
    assert_single(
        "spring-dur",
        "    <Motion>\n      <Scroll curve=\"niriSpring\" duration={ms(100)} />\n    </Motion>",
        "CT072",
        "<Scroll",
    )
}

#[test]
fn spring_on_focus_is_ct072_at_the_curve_prop() -> Result<(), String> {
    assert_single(
        "spring-focus",
        "    <Motion>\n      <Focus curve=\"niriSpring\" />\n    </Motion>",
        "CT072",
        "curve=\"niriSpring\"",
    )
}

#[test]
fn a_duplicate_kind_is_ct073_at_each_extra_element() -> Result<(), String> {
    let body = "    <Motion>\n      <Scroll />\n      <Scroll duration={ms(10)} />\n      <Scroll />\n    </Motion>";
    let (text, r) = run("dup", body)?;
    let diags = r.err().ok_or("診断を期待した")?;
    let first = position_of(&text, "<Scroll duration")?;
    let got: Vec<_> = diags
        .iter()
        .map(|d| (d.code.as_str(), d.line.zip(d.column)))
        .collect();
    assert_eq!(got.len(), 2, "{diags:?}");
    assert!(got.iter().all(|(c, _)| *c == "CT073"));
    assert_eq!(got.first().and_then(|(_, p)| *p), Some(first));
    Ok(())
}

#[test]
fn two_motion_elements_are_rejected_by_cdl() -> Result<(), String> {
    let (_, r) = run("two-motion", "    <Motion />\n    <Motion />")?;
    let diags = r.err().ok_or("診断を期待した")?;
    assert_eq!(
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["E4006"]
    );
    Ok(())
}

#[test]
fn a_string_duration_is_a_cdl_type_error() -> Result<(), String> {
    let (_, r) = run(
        "dur-string",
        "    <Motion>\n      <Scroll duration=\"150ms\" />\n    </Motion>",
    )?;
    let diags = r.err().ok_or("診断を期待した")?;
    assert!(diags.iter().all(|d| d.code.starts_with('E')), "{diags:?}");
    Ok(())
}

#[test]
fn the_old_layout_animation_props_are_unknown_props_at_the_prop() -> Result<(), String> {
    for (dir, body, needle) in [
        (
            "old-animation",
            "    <Layout animation=\"spring\" />",
            "animation=\"spring\"",
        ),
        (
            "old-ms",
            "    <Layout animationMs={150} />",
            "animationMs={150}",
        ),
        (
            "old-with-motion",
            "    <Layout animationMs={150} />\n    <Motion />",
            "animationMs={150}",
        ),
    ] {
        let (text, r) = run(dir, body)?;
        let diags = r.err().ok_or("旧 prop は拒否される")?;
        let got: Vec<_> = diags
            .iter()
            .map(|d| (d.code.as_str(), d.line.zip(d.column)))
            .collect();
        assert_eq!(
            got,
            [("E4003", Some(position_of(&text, needle)?))],
            "{dir}: {diags:?}"
        );
    }
    Ok(())
}

#[test]
fn without_a_motion_element_every_kind_is_instant() -> Result<(), String> {
    let (_, r) = run("no-motion", "    <Layout gap={px(8)} />")?;
    let (m, w) = r.map_err(|d| format!("{d:?}"))?;
    assert_eq!(m, None);
    assert!(w.is_empty());
    Ok(())
}

#[test]
fn a_config_with_an_old_prop_fails_to_reload_and_the_old_config_is_kept() -> Result<(), String> {
    use contrast_policy::{Config, ConfigStore, Policy, ReloadOutcome};
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("impl_motion")
        .join("reload");
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let path = base.join("config.cdl");
    let doc = |body: &str| {
        format!(
            "\"use cdl 1\";\nimport {{ Contrast, Layout, Motion, Scroll }} from \"host:contrast\";\nexport default (\n  <Contrast>\n{body}\n  </Contrast>\n);\n"
        )
    };
    let loader = CdlLoader::new();
    fs::write(
        &path,
        doc("    <Motion><Scroll curve=\"niriSpring\" /></Motion>"),
    )
    .map_err(|e| e.to_string())?;
    let LoadOutcome::Loaded(first) = loader.load(&path) else {
        return Err("最初の設定が読めない".to_owned());
    };
    let store = ConfigStore::new(*first);
    let before = store.generation();
    let had = Policy::motion(&store);
    assert!(had.is_some());
    // 旧 prop を書いた設定: 失敗し、世代も動きも変わらない。
    fs::write(&path, doc("    <Layout animation=\"spring\" />")).map_err(|e| e.to_string())?;
    let ReloadOutcome::Failed(diags) = store.reload(&loader, &path) else {
        return Err("失敗するはず".to_owned());
    };
    assert_eq!(
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["E4003"]
    );
    assert_eq!(store.generation(), before);
    assert_eq!(Policy::motion(&store), had);
    let _ = Config::builtin;
    Ok(())
}
