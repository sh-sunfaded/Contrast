//! U4 段階 (a): 関数形ルート（`Env`）・Profile・名前つきワークスペース・シーン・キー層の変換のテスト。
//! 診断のコードと位置（原因の prop）、`Env` を渡したときの設定、静的ルートの設定が変わらないこと。

use std::fs;
use std::path::Path;

use contrast_policy::{
    Action, Config, ConfigDiagnostic, ConfigLoader, ConfigStore, EnvUpdate, FocusEnv, FocusState,
    LoadOutcome, Mods, Policy, PolicyEnv, Settings,
};
use contrast_policy_cdl::CdlLoader;

const IMPORTS: &str = "import { Contrast, Appearance, Tiers, FocusTier, Tier, Layout, Motion, Scroll, Workspaces, Workspace, Launch, Mode, Keybind } from \"host:contrast\";";

/// 静的ルートの設定を書く。`body` は `<Contrast>` の中身。
fn static_doc(body: &str) -> String {
    format!(
        "\"use cdl 1\";\n{IMPORTS}\nexport default (\n  <Contrast>\n{body}\n  </Contrast>\n);\n"
    )
}

/// 関数形ルートの設定を書く。`body` は `<Contrast>` の中身（`env` が使える）。
fn function_doc(body: &str) -> String {
    format!(
        "\"use cdl 1\";\n{IMPORTS}\nimport type {{ Env }} from \"host:contrast-env\";\nexport default function App(env: Env): Element<Contrast> {{\n  return (\n    <Contrast>\n{body}\n    </Contrast>\n  );\n}}\n"
    )
}

/// `text` を書いて、プロファイル `profile` で読む。
fn load_text(dir: &str, text: &str, profile: &str) -> Result<(String, LoadOutcome), String> {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("impl_env")
        .join(dir);
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let path = base.join("config.cdl");
    fs::write(&path, text).map_err(|e| e.to_string())?;
    let outcome = CdlLoader::new().with_profile(profile).load(&path);
    Ok((text.to_owned(), outcome))
}

/// 読めた設定を返す。診断なら一覧を文字列にしてエラー。
fn loaded(dir: &str, text: &str, profile: &str) -> Result<Config, String> {
    match load_text(dir, text, profile)?.1 {
        LoadOutcome::Loaded(c) => Ok(*c),
        LoadOutcome::Failed(d) => Err(format!("{d:?}")),
        LoadOutcome::Absent => Err("ファイルなし".to_owned()),
    }
}

/// 失敗した診断（コード・行・列）。
fn failed(dir: &str, text: &str) -> Result<(String, Vec<ConfigDiagnostic>), String> {
    match load_text(dir, text, "default")? {
        (t, LoadOutcome::Failed(d)) => Ok((t, d)),
        (_, LoadOutcome::Loaded(_)) => Err("診断を期待した".to_owned()),
        (_, LoadOutcome::Absent) => Err("ファイルなし".to_owned()),
    }
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

/// 唯一の診断が `code` で位置が `needle`。
fn assert_single(dir: &str, text: &str, code: &str, needle: &str) -> Result<(), String> {
    let (text, d) = failed(dir, text)?;
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

fn render(config: &Config, env: &PolicyEnv) -> Result<Settings, String> {
    let r = config.env_renderer.as_ref().ok_or("env_renderer が無い")?;
    r.render(env).map_err(|d| format!("{d:?}"))
}

fn focus(app_id: &str, state: FocusState) -> Option<FocusEnv> {
    Some(FocusEnv {
        app_id: app_id.to_owned(),
        is_dialog: false,
        state,
    })
}

// ---- 関数形ルートと Profile ----

const PROFILE_BODY: &str = "      {env.profile === \"dim\" && (\n        <Tiers>\n          <FocusTier l={1} />\n          <Tier l={0.6} />\n        </Tiers>\n      )}\n      {env.profile === \"fast\" && (\n        <Motion>\n          <Scroll curve=\"easeOutQuad\" duration={ms(90)} />\n        </Motion>\n      )}";

#[test]
fn a_function_root_loads_with_an_env_renderer_and_the_profile_it_was_read_with()
-> Result<(), String> {
    let c = loaded("fn-root", &function_doc(PROFILE_BODY), "dim")?;
    assert!(c.env_renderer.is_some());
    assert_eq!(c.profile, "dim");
    // `Config.settings` は基準の Env（フォーカスなし。profile は読んだときの値）で作った設定。
    assert!(c.settings.appearance.tiers.unfocused_dim() < 1.0);
    let plain = loaded("fn-root-default", &function_doc(PROFILE_BODY), "default")?;
    assert_eq!(plain.profile, "default");
    assert_eq!(
        plain.settings.appearance.tiers,
        Settings::builtin().appearance.tiers
    );
    Ok(())
}

#[test]
fn static_roots_have_no_env_renderer() -> Result<(), String> {
    let c = loaded("static", &static_doc("    <Layout gap={px(8)} />"), "dim")?;
    assert!(c.env_renderer.is_none());
    assert_eq!(c.settings.layout.gap, 8);
    Ok(())
}

#[test]
fn the_profile_switches_tiers_and_motion() -> Result<(), String> {
    let c = loaded("profiles", &function_doc(PROFILE_BODY), "default")?;
    let dim = render(&c, &PolicyEnv::baseline("dim"))?;
    assert!(dim.appearance.tiers.unfocused_dim() < 0.9);
    assert_eq!(dim.motion, None);
    let fast = render(&c, &PolicyEnv::baseline("fast"))?;
    assert!(fast.motion.is_some());
    assert_eq!(fast.appearance.tiers, Settings::builtin().appearance.tiers);
    // 同じ Env からは同じ設定（純粋・決定的）。
    assert_eq!(
        render(&c, &PolicyEnv::baseline("dim"))?,
        render(&c, &PolicyEnv::baseline("dim"))?
    );
    Ok(())
}

#[test]
fn the_focus_and_workspace_of_the_env_are_readable() -> Result<(), String> {
    let body = "      {env.focus?.appId === \"foot\" && (\n        <Tiers>\n          <FocusTier l={1} />\n        </Tiers>\n      )}\n      {env.workspace.name === \"web\" && <Layout gap={px(0)} />}";
    let c = loaded("focus", &function_doc(body), "default")?;
    let mut env = PolicyEnv::baseline("default");
    let none = render(&c, &env)?;
    env.focus = focus("foot", FocusState::Tiled);
    let foot = render(&c, &env)?;
    assert_ne!(none.appearance.tiers, foot.appearance.tiers);
    assert_eq!(foot.appearance.tiers.unfocused_dim(), 1.0);
    env.focus = focus("other", FocusState::Fullscreen);
    assert_eq!(render(&c, &env)?, none);
    Ok(())
}

#[test]
fn a_failure_in_the_baseline_env_is_ct081_and_keeps_the_original_code_in_the_message()
-> Result<(), String> {
    let body = "      <Layout gap={px(9999)} />";
    let (text, d) = failed("baseline-bad", &function_doc(body))?;
    let got: Vec<_> = d.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(got, ["CT081"], "{d:?}");
    let first = d.first().ok_or("診断が無い")?;
    assert!(first.message.contains("CT003"), "{}", first.message);
    assert_eq!(
        first.line.zip(first.column),
        Some(position_of(&text, "gap={px(9999)}")?)
    );
    Ok(())
}

#[test]
fn a_failure_only_under_a_representative_env_is_found_at_load() -> Result<(), String> {
    let body = "      {env.focus?.state === \"fullscreen\" && <Layout gap={px(9999)} />}";
    let (_, d) = failed("rep-bad", &function_doc(body))?;
    assert_eq!(
        d.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["CT081"],
        "{d:?}"
    );
    Ok(())
}

#[test]
fn a_type_mismatch_of_the_env_is_ct081_at_load_never_a_panic() -> Result<(), String> {
    // `env.nothing` は `Env` に無いフィールド（CDL の型検査の誤り。E コード）。
    let body = "      {env.nothing === \"x\" && <Layout gap={px(1)} />}";
    let (_, d) = failed("env-field", &function_doc(body))?;
    assert!(d.iter().all(|d| d.code.starts_with('E')), "{d:?}");
    Ok(())
}

#[test]
fn layout_and_workspaces_do_not_follow_the_env() -> Result<(), String> {
    let body = "      {env.profile === \"wide\" && <Layout gap={px(7)} />}\n      {env.profile === \"dim\" && <Tiers><FocusTier l={1} /><Tier l={0.5} /></Tiers>}";
    let c = loaded("fixed-parts", &function_doc(body), "default")?;
    let store = ConfigStore::new(c);
    let EnvUpdate::Applied(notes) = store.update_env(PolicyEnv::baseline("wide")) else {
        return Err("Applied を期待した".to_owned());
    };
    assert_eq!(
        notes.iter().map(|n| n.code.as_str()).collect::<Vec<_>>(),
        ["CT050"]
    );
    assert_eq!(Policy::layout_settings(&store).gap, 0);
    // 変わってよい部分（Tiers）は反映される。
    assert!(matches!(
        store.update_env(PolicyEnv::baseline("dim")),
        EnvUpdate::Applied(_)
    ));
    assert!(Policy::tiers(&store).unfocused_dim() < 0.9);
    Ok(())
}

#[test]
fn a_static_root_loaded_into_a_store_ignores_env_updates() -> Result<(), String> {
    let c = loaded(
        "static-store",
        &static_doc("    <Layout gap={px(3)} />"),
        "default",
    )?;
    let store = ConfigStore::new(c);
    assert_eq!(
        store.update_env(PolicyEnv::baseline("dim")),
        EnvUpdate::Unchanged
    );
    Ok(())
}

// ---- 名前つきワークスペースとシーン ----

const NAMES: &str = "    <Workspaces count={4}>\n      <Workspace name=\"web\">\n        <Launch command=\"firefox\" />\n        <Launch command=\"foot -e htop\" />\n      </Workspace>\n      <Workspace name=\"code\" />\n    </Workspaces>";

#[test]
fn named_workspaces_are_read_in_order_with_their_launch_commands() -> Result<(), String> {
    let c = loaded("names", &static_doc(NAMES), "default")?;
    let s = c.settings;
    assert_eq!(s.workspaces, 4);
    assert_eq!(s.workspace_named("web"), Some(0));
    assert_eq!(s.workspace_named("code"), Some(1));
    assert_eq!(s.workspace_named("none"), None);
    assert_eq!(
        s.named_workspaces.first().map(|w| w.launch.clone()),
        Some(vec!["firefox".to_owned(), "foot -e htop".to_owned()])
    );
    Ok(())
}

#[test]
fn a_name_on_dynamic_workspaces_is_ct083() -> Result<(), String> {
    let body = "    <Workspaces count={4} mode=\"dynamic\">\n      <Workspace name=\"web\" />\n    </Workspaces>";
    assert_single("names-dynamic", &static_doc(body), "CT083", "name=\"web\"")
}

#[test]
fn duplicate_empty_and_too_many_names_are_ct083_at_the_name() -> Result<(), String> {
    let dup = "    <Workspaces count={4}>\n      <Workspace name=\"web\" />\n      <Workspace name=\"web\" />\n    </Workspaces>";
    let (text, d) = failed("names-dup", &static_doc(dup))?;
    assert_eq!(
        d.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["CT083"]
    );
    let second = text
        .match_indices("name=\"web\"")
        .nth(1)
        .map(|(i, _)| i)
        .ok_or("2 つ目")?;
    let before = text.get(..second).ok_or("境界")?;
    assert_eq!(
        d.first().and_then(|d| d.line),
        Some(
            u32::try_from(before.matches('\n').count().saturating_add(1))
                .map_err(|e| e.to_string())?
        )
    );
    assert_single(
        "names-empty",
        &static_doc("    <Workspaces count={4}>\n      <Workspace name=\"\" />\n    </Workspaces>"),
        "CT083",
        "name=\"\"",
    )?;
    assert_single(
        "names-many",
        &static_doc(
            "    <Workspaces count={1}>\n      <Workspace name=\"a\" />\n      <Workspace name=\"b\" />\n    </Workspaces>",
        ),
        "CT083",
        "name=\"b\"",
    )
}

fn names_with_keybind(action: &str) -> String {
    static_doc(&format!(
        "{NAMES}\n    <Keybind keys=\"alt+1\" action=\"{action}\" name=\"web\" />"
    ))
}

#[test]
fn workspace_name_and_scene_actions_carry_the_name() -> Result<(), String> {
    let alt = Mods {
        alt: true,
        ..Mods::default()
    };
    let c = loaded("act-name", &names_with_keybind("workspace-name"), "default")?;
    assert_eq!(
        c.settings.resolve_key(alt, "1"),
        Some(Action::SwitchWorkspaceByName("web".to_owned()))
    );
    let c = loaded("act-scene", &names_with_keybind("scene"), "default")?;
    assert_eq!(
        c.settings.resolve_key(alt, "1"),
        Some(Action::OpenScene("web".to_owned()))
    );
    Ok(())
}

#[test]
fn unknown_names_are_ct084_and_unknown_scenes_are_ct085_at_the_name() -> Result<(), String> {
    let unknown = |action: &str| {
        static_doc(&format!(
            "{NAMES}\n    <Keybind keys=\"alt+1\" action=\"{action}\" name=\"nope\" />"
        ))
    };
    assert_single(
        "unk-name",
        &unknown("workspace-name"),
        "CT084",
        "name=\"nope\"",
    )?;
    assert_single("unk-scene", &unknown("scene"), "CT085", "name=\"nope\"")?;
    // `name` が無い。
    let missing = static_doc(&format!(
        "{NAMES}\n    <Keybind keys=\"alt+1\" action=\"scene\" />"
    ));
    let (_, d) = failed("scene-noname", &missing)?;
    assert_eq!(
        d.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["CT003"]
    );
    Ok(())
}

// ---- キー層 ----

const MODES: &str = "    <Mode name=\"resize\" fallthrough={true} />\n    <Keybind keys=\"alt+r\" action=\"enter-mode\" name=\"resize\" />\n    <Keybind mode=\"resize\" keys=\"h\" action=\"focus-left\" />\n    <Keybind mode=\"resize\" keys=\"q\" action=\"exit-mode\" />\n    <Keybind keys=\"h\" action=\"focus-right\" />";

#[test]
fn keybinds_belong_to_a_mode_and_the_same_key_may_differ_between_modes() -> Result<(), String> {
    let c = loaded("modes", &static_doc(MODES), "default")?;
    let s = c.settings;
    let none = Mods::default();
    assert_eq!(s.resolve_key(none, "h"), Some(Action::FocusNext));
    assert_eq!(
        s.resolve_key_in("resize", none, "h"),
        Some(Action::FocusPrev)
    );
    assert_eq!(
        s.resolve_key_in("resize", none, "q"),
        Some(Action::ExitMode)
    );
    assert_eq!(
        s.resolve_key(
            Mods {
                alt: true,
                ..Mods::default()
            },
            "r"
        ),
        Some(Action::EnterMode("resize".to_owned()))
    );
    // 層の中のキーは normal からは引けない。
    assert_eq!(s.resolve_key(none, "q"), None);
    assert!(s.mode_fallthrough("resize"));
    assert!(!s.mode_fallthrough("normal"));
    Ok(())
}

#[test]
fn a_duplicate_key_is_ct031_only_inside_the_same_mode() -> Result<(), String> {
    let dup = MODES.to_owned() + "\n    <Keybind mode=\"resize\" keys=\"h\" action=\"close\" />";
    let (text, d) = failed("modes-dup", &static_doc(&dup))?;
    assert_eq!(
        d.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["CT031"]
    );
    assert_eq!(d.first().and_then(|d| d.line).map(|l| l > 0), Some(true));
    assert!(text.contains("action=\"close\""));
    Ok(())
}

#[test]
fn an_undeclared_mode_or_an_unknown_target_is_ct084() -> Result<(), String> {
    assert_single(
        "mode-undeclared",
        &static_doc("    <Keybind mode=\"ghost\" keys=\"h\" action=\"exit-mode\" />"),
        "CT084",
        "mode=\"ghost\"",
    )?;
    assert_single(
        "mode-target",
        &static_doc(
            "    <Mode name=\"m\" />\n    <Keybind mode=\"m\" keys=\"q\" action=\"exit-mode\" />\n    <Keybind keys=\"alt+r\" action=\"enter-mode\" name=\"ghost\" />",
        ),
        "CT084",
        "name=\"ghost\"",
    )
}

#[test]
fn a_mode_without_a_way_out_is_ct082_at_the_mode() -> Result<(), String> {
    assert_single(
        "mode-no-exit",
        &static_doc(
            "    <Mode name=\"trap\" />\n    <Keybind mode=\"trap\" keys=\"h\" action=\"focus-left\" />",
        ),
        "CT082",
        "<Mode",
    )?;
    // `enter-mode normal` も出口。
    loaded(
        "mode-exit-normal",
        &static_doc(
            "    <Mode name=\"m\" />\n    <Keybind mode=\"m\" keys=\"q\" action=\"enter-mode\" name=\"normal\" />",
        ),
        "default",
    )?;
    // 層にキーが 1 つも無い層も出口が無い。
    assert_single(
        "mode-empty",
        &static_doc("    <Mode name=\"empty\" />"),
        "CT082",
        "<Mode",
    )
}

#[test]
fn escape_without_modifiers_cannot_be_rebound_inside_a_mode() -> Result<(), String> {
    assert_single(
        "mode-escape",
        &static_doc(
            "    <Mode name=\"m\" />\n    <Keybind mode=\"m\" keys=\"q\" action=\"exit-mode\" />\n    <Keybind mode=\"m\" keys=\"Escape\" action=\"focus-left\" />",
        ),
        "CT082",
        "keys=\"Escape\"",
    )?;
    // normal 層の Escape は自由。修飾キーつきの Escape も自由。
    loaded(
        "mode-escape-ok",
        &static_doc(
            "    <Mode name=\"m\" />\n    <Keybind mode=\"m\" keys=\"q\" action=\"exit-mode\" />\n    <Keybind mode=\"m\" keys=\"alt+Escape\" action=\"focus-left\" />\n    <Keybind keys=\"Escape\" action=\"focus-left\" />",
        ),
        "default",
    )?;
    Ok(())
}
