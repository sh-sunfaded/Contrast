//! N2 の設定（`Startup`・`Environment`・`Input.focusFollowsMouse`・`Cursor.hideAfterInactiveMs`・
//! フックの `fullscreen`）の変換のテスト: 値、既定、誤りの診断。

use std::fs;
use std::path::{Path, PathBuf};

use contrast_policy::{Config, ConfigLoader, LoadOutcome};
use contrast_policy_cdl::CdlLoader;

/// 一時ファイルを書いて、そのパスを返す。`head` は import のあとに置く宣言、`body` は `<Contrast>` の中身。
fn write_temp(dir: &str, head: &str, body: &str) -> Result<PathBuf, String> {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("impl_startup")
        .join(dir);
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let path = base.join("config.cdl");
    let text = format!(
        "\"use cdl 1\";\n\
         import {{ Contrast, Cursor, Input, Startup, Environment, Env }} from \"host:contrast\";\n\
         import type {{ Policy, WindowInfo, Placement }} from \"host:contrast\";\n\
         {head}\n\
         export default (\n  <Contrast>\n{body}\n  </Contrast>\n);\n"
    );
    fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(path)
}

/// 読み込んで、設定か診断コードの並びを返す。
fn load_with(dir: &str, head: &str, body: &str) -> Result<Result<Config, Vec<String>>, String> {
    let path = write_temp(dir, head, body)?;
    Ok(match CdlLoader::new().load(&path) {
        LoadOutcome::Loaded(c) => Ok(*c),
        LoadOutcome::Failed(d) => Err(d.iter().map(|d| d.code.clone()).collect()),
        LoadOutcome::Absent => Err(vec!["absent".to_owned()]),
    })
}

/// `<Contrast>` の中身だけ与えて読み込む。
fn load(dir: &str, body: &str) -> Result<Result<Config, Vec<String>>, String> {
    load_with(dir, "", body)
}

/// 失敗する設定の診断コード。成功したらエラー。
fn codes_of(dir: &str, body: &str) -> Result<Vec<String>, String> {
    match load(dir, body)? {
        Ok(_) => Err(format!("誤りにならない: {body}")),
        Err(codes) => Ok(codes),
    }
}

#[test]
fn defaults_have_no_startup_environment_or_hiding() -> Result<(), String> {
    let c = load("none", "")?.map_err(|d| format!("{d:?}"))?;
    assert!(c.settings.startup.is_empty());
    assert!(c.settings.environment.is_empty());
    assert_eq!(c.settings.cursor.hide_after_inactive_ms, None);
    assert!(!c.settings.input.focus_follows_mouse);
    Ok(())
}

#[test]
fn startup_commands_are_read_in_order() -> Result<(), String> {
    let body = r#"
    <Startup command="fcitx5" />
    <Startup command="ironbar -c /a.toml -t /b.css" />
    <Startup command="  mako  " />"#;
    let c = load("startup", body)?.map_err(|d| format!("{d:?}"))?;
    let commands: Vec<&str> = c
        .settings
        .startup
        .iter()
        .map(|s| s.command.as_str())
        .collect();
    assert_eq!(commands, ["fcitx5", "ironbar -c /a.toml -t /b.css", "mako"]);
    // `ipc` を書かなければ、`NIRI_SOCKET` は渡さない。
    assert!(c.settings.startup.iter().all(|s| !s.niri_ipc));
    Ok(())
}

#[test]
fn startup_ipc_niri_marks_only_that_command() -> Result<(), String> {
    let body = r#"
    <Startup command="fcitx5" />
    <Startup command="ironbar -c /a.toml" ipc="niri" />
    <Startup command="mako" />"#;
    let c = load("startup-ipc", body)?.map_err(|d| format!("{d:?}"))?;
    let flags: Vec<(&str, bool)> = c
        .settings
        .startup
        .iter()
        .map(|s| (s.command.as_str(), s.niri_ipc))
        .collect();
    assert_eq!(
        flags,
        [
            ("fcitx5", false),
            ("ironbar -c /a.toml", true),
            ("mako", false)
        ]
    );
    Ok(())
}

#[test]
fn bad_startup_ipc_is_ct003() -> Result<(), String> {
    for (dir, value) in [("sway", "sway"), ("empty", ""), ("upper", "Niri")] {
        let body = format!(r#"<Startup command="x" ipc="{value}" />"#);
        assert_eq!(
            codes_of(&format!("startup-ipc-{dir}"), &body)?,
            ["CT003"],
            "{value:?}"
        );
    }
    // 文字列でない値は、CDL のスキーマ適合の誤り（E4002 ではなく型の誤り）。成功してはいけない。
    assert!(load("startup-ipc-number", r#"<Startup command="x" ipc={1} />"#)?.is_err());
    Ok(())
}

#[test]
fn bad_startup_is_an_error() -> Result<(), String> {
    assert_eq!(
        codes_of("startup-empty", r#"<Startup command="   " />"#)?,
        ["CT003"]
    );
    // `command` が無いのは CDL のスキーマ適合（E4002）。
    assert_eq!(codes_of("startup-missing", "<Startup />")?, ["E4002"]);
    Ok(())
}

#[test]
fn environment_is_read_as_a_sorted_map() -> Result<(), String> {
    let body = r#"
    <Environment>
      <Env name="QT_IM_MODULE" value="fcitx5" />
      <Env name="GTK_IM_MODULE" value="fcitx5" />
      <Env name="SDL_IM_MODULE" value="fcitx5" />
      <Env name="MOZ_ENABLE_WAYLAND" value="1" />
      <Env name="EMPTY" value="" />
    </Environment>"#;
    let c = load("env", body)?.map_err(|d| format!("{d:?}"))?;
    let got: Vec<(&str, &str)> = c
        .settings
        .environment
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            ("EMPTY", ""),
            ("GTK_IM_MODULE", "fcitx5"),
            ("MOZ_ENABLE_WAYLAND", "1"),
            ("QT_IM_MODULE", "fcitx5"),
            ("SDL_IM_MODULE", "fcitx5"),
        ]
    );
    Ok(())
}

#[test]
fn bad_environment_is_an_error() -> Result<(), String> {
    let env = |inner: &str| format!("<Environment>{inner}</Environment>");
    // 名前の重複。
    assert_eq!(
        codes_of(
            "env-dup",
            &env(r#"<Env name="A" value="1" /><Env name="A" value="2" />"#)
        )?,
        ["CT003"]
    );
    // 空の名前、`=` を含む名前。
    assert_eq!(
        codes_of("env-empty-name", &env(r#"<Env name="" value="1" />"#))?,
        ["CT003"]
    );
    assert_eq!(
        codes_of("env-equals", &env(r#"<Env name="A=B" value="1" />"#))?,
        ["CT003"]
    );
    // `value` が無い、`Environment` が 2 つ（一意な子）は CDL のスキーマ適合。
    assert_eq!(
        codes_of("env-no-value", &env(r#"<Env name="A" />"#))?,
        ["E4002"]
    );
    assert_eq!(
        codes_of(
            "env-twice",
            "<Environment></Environment><Environment></Environment>"
        )?,
        ["E4006"]
    );
    Ok(())
}

#[test]
fn focus_follows_mouse_is_read() -> Result<(), String> {
    let on = load("ffm-on", "<Input focusFollowsMouse={true} />")?.map_err(|d| format!("{d:?}"))?;
    assert!(on.settings.input.focus_follows_mouse);
    let off =
        load("ffm-off", "<Input focusFollowsMouse={false} />")?.map_err(|d| format!("{d:?}"))?;
    assert!(!off.settings.input.focus_follows_mouse);
    // ほかの項目は既定のまま。
    assert!(on.settings.input.tap_to_click);
    // 型が違うのは CDL の型検査（E3xxx）。
    let wrong = codes_of("ffm-wrong", r#"<Input focusFollowsMouse="yes" />"#)?;
    assert!(wrong.iter().all(|c| c.starts_with('E')), "{wrong:?}");
    Ok(())
}

#[test]
fn hide_after_inactive_ms_accepts_the_edges_and_rejects_the_rest() -> Result<(), String> {
    for (dir, v, want) in [
        ("lo", "1", 1),
        ("mid", "1500", 1500),
        ("hi", "3600000", 3_600_000),
    ] {
        let c = load(dir, &format!("<Cursor hideAfterInactiveMs={{{v}}} />"))?
            .map_err(|d| format!("{d:?}"))?;
        assert_eq!(c.settings.cursor.hide_after_inactive_ms, Some(want));
    }
    for (dir, v) in [
        ("zero", "0"),
        ("negative", "-5"),
        ("fraction", "1.5"),
        ("over", "3600001"),
    ] {
        assert_eq!(
            codes_of(dir, &format!("<Cursor hideAfterInactiveMs={{{v}}} />"))?,
            ["CT003"],
            "{v}"
        );
    }
    // theme・size と一緒に使える。
    let c = load(
        "with-size",
        r#"<Cursor theme="x" size={24} hideAfterInactiveMs={1000} />"#,
    )?
    .map_err(|d| format!("{d:?}"))?;
    assert_eq!(c.settings.cursor.size, Some(24));
    assert_eq!(c.settings.cursor.hide_after_inactive_ms, Some(1000));
    Ok(())
}

#[test]
fn open_fullscreen_comes_from_the_hook() -> Result<(), String> {
    let head = r#"
export const policy = {
  windowOpen: (w: WindowInfo): Placement => {
    switch (w.appId) {
      case "mpv": return { floating: false, fullscreen: true };
      case "plain": return { floating: false, fullscreen: false };
      default: return { floating: false };
    }
  },
} satisfies Policy;"#;
    let c = load_with("fullscreen", head, "")?.map_err(|d| format!("{d:?}"))?;
    let meta = |app: &str| {
        let mut m = contrast_core::WindowMeta::default();
        m.app_id = Some(app.to_owned());
        m
    };
    for (app, want) in [("mpv", true), ("plain", false), ("other", false)] {
        let (p, d) = c.place(&meta(app), Some(0));
        assert_eq!(d, None, "{app}");
        assert_eq!(p.fullscreen, want, "{app}");
    }
    Ok(())
}

#[test]
fn wrong_fullscreen_type_in_the_hook_is_a_cdl_type_error() -> Result<(), String> {
    let head = r#"
export const policy = {
  windowOpen: (w: WindowInfo): Placement => ({ floating: false, fullscreen: "yes" }),
} satisfies Policy;"#;
    match load_with("fullscreen-wrong", head, "")? {
        Ok(_) => Err("型検査を通ってしまった".to_owned()),
        Err(codes) => {
            assert!(codes.iter().all(|c| c.starts_with('E')), "{codes:?}");
            Ok(())
        }
    }
}

#[test]
fn the_hook_can_let_the_window_choose_its_column_width() -> Result<(), String> {
    let head = r#"
export const policy = {
  windowOpen: (w: WindowInfo): Placement =>
    w.appId === "wez" ? { floating: false, columnWidth: "window" } : { floating: false },
} satisfies Policy;"#;
    let c = load_with("column-width", head, "")?.map_err(|d| format!("{d:?}"))?;
    let meta = |app: &str| {
        let mut m = contrast_core::WindowMeta::default();
        m.app_id = Some(app.to_owned());
        m
    };
    let (p, d) = c.place(&meta("wez"), Some(0));
    assert_eq!(d, None);
    assert_eq!(
        p.column_width,
        Some(contrast_policy::PlacementWidth::Window)
    );
    let (q, _) = c.place(&meta("other"), Some(0));
    assert_eq!(q.column_width, None);
    Ok(())
}
