//! U4 段階 (a): `examples/u4-*.cdl` が読めること、プロファイルごとの設定、`render` + 変換の所要時間（60Hz のフレーム予算 16.7ms に対して）。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use contrast_policy::{Config, ConfigLoader, FocusEnv, FocusState, LoadOutcome, PolicyEnv};
use contrast_policy_cdl::CdlLoader;

fn example(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name)
}

fn load(name: &str, profile: &str) -> Result<Config, String> {
    match CdlLoader::new().with_profile(profile).load(&example(name)) {
        LoadOutcome::Loaded(c) => Ok(*c),
        LoadOutcome::Failed(d) => Err(format!("{d:?}")),
        LoadOutcome::Absent => Err(format!("{name} が無い")),
    }
}

#[test]
fn the_profile_example_switches_tiers_and_motion_by_profile() -> Result<(), String> {
    let c = load("u4-profile.cdl", "default")?;
    let r = c.env_renderer.as_ref().ok_or("env_renderer")?;
    let get = |p: &str| {
        r.render(&PolicyEnv::baseline(p))
            .map_err(|d| format!("{d:?}"))
    };
    let default = get("default")?;
    assert_eq!(default.motion, None);
    assert_eq!(
        default.appearance.tiers,
        contrast_policy::Settings::builtin().appearance.tiers
    );
    let dim = get("dim")?;
    assert!(dim.appearance.tiers.unfocused_dim() < 0.9);
    assert!(dim.motion.is_some_and(|m| m.focus.is_some()));
    let calm = get("calm")?;
    assert!(
        calm.motion
            .is_some_and(|m| m.scroll.is_some() && m.focus.is_none())
    );
    let fast = get("fast")?;
    assert!(fast.motion.is_some_and(|m| m.scroll.is_some()));
    // 全画面のときだけ増えるバインド。
    let mut env = PolicyEnv::baseline("default");
    let before = r.render(&env).map_err(|d| format!("{d:?}"))?.keybinds.len();
    env.focus = Some(FocusEnv {
        app_id: "x".to_owned(),
        is_dialog: false,
        state: FocusState::Fullscreen,
    });
    let after = r.render(&env).map_err(|d| format!("{d:?}"))?.keybinds.len();
    assert_eq!(after, before + 1);
    Ok(())
}

#[test]
fn the_workspaces_example_has_names_scenes_and_a_mode() -> Result<(), String> {
    let c = load("u4-workspaces.cdl", "default")?;
    assert!(c.env_renderer.is_none());
    let s = c.settings;
    assert_eq!(s.workspace_named("code"), Some(1));
    assert_eq!(s.named_workspaces.len(), 3);
    assert!(s.modes.iter().any(|m| m.name == "resize"));
    Ok(())
}

/// `render` + 変換の所要時間（`config/` より小さい例だが、条件・Keybind を含む）。測定値は `--nocapture` で出る。
#[test]
fn render_and_convert_fit_in_the_frame_budget() -> Result<(), String> {
    let c = load("u4-profile.cdl", "dim")?;
    let r = c.env_renderer.as_ref().ok_or("env_renderer")?;
    let mut env = PolicyEnv::baseline("dim");
    env.focus = Some(FocusEnv {
        app_id: "foot".to_owned(),
        is_dialog: false,
        state: FocusState::Tiled,
    });
    let n: u32 = 300;
    let mut worst = Duration::ZERO;
    let start = Instant::now();
    for i in 0..n {
        env.workspace.window_count = i % 7;
        let t = Instant::now();
        r.render(&env).map_err(|d| format!("{d:?}"))?;
        worst = worst.max(t.elapsed());
    }
    let total = start.elapsed();
    let mean = total / n;
    eprintln!("U4 render+convert: mean {mean:?}, worst {worst:?} over {n} runs (debug build)");
    // 60Hz のフレーム予算 16.7ms。debug ビルドでも平均はその 1/10 以下を期待する。
    assert!(mean < Duration::from_micros(1670), "mean {mean:?}");
    Ok(())
}
