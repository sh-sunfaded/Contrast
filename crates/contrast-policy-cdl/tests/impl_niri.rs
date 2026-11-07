//! `examples/niri.cdl` のテスト: 読めること、キーの重複がないこと、各バインドが期待する `Action` になること、
//! `modKey` の優先順位（明示 > 読み込み器の既定）。
//!
//! 重複は読み込みが CT031 で失敗することで分かる。環境変数とバックエンドの既定の優先順位は、
//! 読み込み器に渡す前（コンポジタの `resolve_mod_key`）で決まるので、そちらのテストが持つ。

use std::fs;
use std::path::{Path, PathBuf};

use contrast_policy::{Action, ConfigLoader, LoadOutcome, ModKey, Mods, Settings};
use contrast_policy_cdl::CdlLoader;

/// リポジトリの `examples/` のファイル。
fn example(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name)
}

/// `loader` で `path` を読む。
fn load_with(loader: &CdlLoader, path: &Path) -> Result<Settings, String> {
    match loader.load(path) {
        LoadOutcome::Loaded(c) => Ok(c.settings),
        LoadOutcome::Failed(d) => Err(format!("{d:?}")),
        LoadOutcome::Absent => Err(format!("{} が無い", path.display())),
    }
}

/// 一時ファイルを書いて、そのパスを返す。
fn write_temp(dir: &str, body: &str) -> Result<PathBuf, String> {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("impl_niri")
        .join(dir);
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let path = base.join("config.cdl");
    fs::write(&path, body).map_err(|e| e.to_string())?;
    Ok(path)
}

fn super_mods() -> Mods {
    Mods {
        logo: true,
        ..Mods::default()
    }
}

#[test]
fn niri_example_loads_without_duplicate_keys() -> Result<(), String> {
    let loader = CdlLoader::new().with_default_mod_key(ModKey::Super);
    let settings = load_with(&loader, &example("niri.cdl"))?;
    assert_eq!(settings.mod_key, ModKey::Super);
    assert_eq!(settings.workspaces, 9);
    // 読み込みが通れば CT031（キーの重複）は無いが、念のため（修飾キー, キー名）の一意も確かめる。
    let mut seen = std::collections::BTreeSet::new();
    for b in &settings.keybinds {
        let m = b.key.mods;
        let id = (
            m.alt,
            m.shift,
            m.ctrl,
            m.logo,
            b.key.keysym.to_ascii_lowercase(),
        );
        assert!(seen.insert(id), "duplicate key {:?}", b.key);
    }
    Ok(())
}

#[test]
fn niri_example_binds_resolve_to_the_expected_actions() -> Result<(), String> {
    let loader = CdlLoader::new().with_default_mod_key(ModKey::Super);
    let s = load_with(&loader, &example("niri.cdl"))?;
    let m = super_mods();
    let shift = Mods { shift: true, ..m };
    let ctrl = Mods { ctrl: true, ..m };
    let spawn = |c: &str| Some(Action::Spawn(c.to_owned()));

    assert_eq!(s.resolve_key(m, "t"), spawn("alacritty"));
    assert_eq!(s.resolve_key(m, "d"), spawn("fuzzel"));
    assert_eq!(s.resolve_key(m, "b"), spawn("firefox-nightly"));
    assert_eq!(
        s.resolve_key(shift, "d"),
        spawn("strat fedora /home/user/DiscordPTB/launch.sh")
    );
    assert_eq!(
        s.resolve_key(Mods { alt: true, ..m }, "e"),
        spawn("strat fedora /home/user/eclipse/eclipse")
    );
    assert_eq!(
        s.resolve_key(Mods { alt: true, ..m }, "l"),
        spawn("swaylock")
    );
    assert_eq!(
        s.resolve_key(
            Mods {
                ctrl: true,
                shift: true,
                ..Mods::default()
            },
            "space"
        ),
        spawn("fcitx5-remote -t")
    );
    assert_eq!(
        s.resolve_key(Mods::default(), "XF86AudioMute"),
        spawn("wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle")
    );

    assert_eq!(s.resolve_key(m, "q"), Some(Action::CloseFocused));
    assert_eq!(s.resolve_key(shift, "e"), Some(Action::Quit));
    assert_eq!(s.resolve_key(m, "Left"), Some(Action::FocusPrev));
    assert_eq!(s.resolve_key(m, "h"), Some(Action::FocusPrev));
    assert_eq!(s.resolve_key(m, "Right"), Some(Action::FocusNext));
    assert_eq!(s.resolve_key(m, "l"), Some(Action::FocusNext));
    assert_eq!(s.resolve_key(m, "j"), Some(Action::FocusDown));
    assert_eq!(s.resolve_key(m, "k"), Some(Action::FocusUp));
    assert_eq!(s.resolve_key(ctrl, "Left"), Some(Action::MoveLeft));
    assert_eq!(s.resolve_key(ctrl, "l"), Some(Action::MoveRight));

    assert_eq!(s.resolve_key(m, "Page_Down"), Some(Action::WorkspaceNext));
    assert_eq!(s.resolve_key(m, "Page_Up"), Some(Action::WorkspacePrev));
    assert_eq!(s.resolve_key(m, "u"), Some(Action::WorkspaceNext));
    assert_eq!(s.resolve_key(m, "i"), Some(Action::WorkspacePrev));
    assert_eq!(
        s.resolve_key(ctrl, "Page_Down"),
        Some(Action::MoveToWorkspaceNext)
    );
    assert_eq!(s.resolve_key(ctrl, "i"), Some(Action::MoveToWorkspacePrev));
    assert_eq!(s.resolve_key(m, "1"), Some(Action::SwitchWorkspace(0)));
    assert_eq!(s.resolve_key(m, "9"), Some(Action::SwitchWorkspace(8)));
    assert_eq!(s.resolve_key(ctrl, "1"), Some(Action::MoveToWorkspace(0)));
    assert_eq!(s.resolve_key(ctrl, "9"), Some(Action::MoveToWorkspace(8)));

    assert_eq!(s.resolve_key(shift, "f"), Some(Action::ToggleFullscreen));
    assert_eq!(s.resolve_key(m, "v"), Some(Action::ToggleFloating));
    // スクロール列のキー。
    assert_eq!(s.resolve_key(m, "Home"), Some(Action::FocusColumnFirst));
    assert_eq!(s.resolve_key(m, "End"), Some(Action::FocusColumnLast));
    assert_eq!(s.resolve_key(ctrl, "Home"), Some(Action::MoveColumnFirst));
    assert_eq!(s.resolve_key(ctrl, "End"), Some(Action::MoveColumnLast));
    assert_eq!(
        s.resolve_key(m, "Comma"),
        Some(Action::ConsumeWindowIntoColumn)
    );
    assert_eq!(
        s.resolve_key(m, "Period"),
        Some(Action::ExpelWindowFromColumn)
    );
    assert_eq!(s.resolve_key(m, "r"), Some(Action::SwitchPresetColumnWidth));
    assert_eq!(s.resolve_key(m, "f"), Some(Action::MaximizeColumn));
    assert_eq!(
        s.resolve_key(m, "Minus"),
        Some(Action::AdjustColumnWidth(-10))
    );
    assert_eq!(
        s.resolve_key(m, "Equal"),
        Some(Action::AdjustColumnWidth(10))
    );
    assert_eq!(s.resolve_key(m, "Up"), Some(Action::FocusUp));
    assert_eq!(s.resolve_key(m, "Down"), Some(Action::FocusDown));
    assert_eq!(s.resolve_key(ctrl, "k"), Some(Action::MoveUp));
    assert_eq!(s.resolve_key(ctrl, "Down"), Some(Action::MoveDown));
    assert_eq!(s.resolve_key(m, "c"), Some(Action::CenterColumn));
    // まだ対応しないキーは何もしない（consume-or-expel-window-left/right）。
    assert_eq!(s.resolve_key(m, "BracketLeft"), None);
    assert_eq!(s.resolve_key(m, "BracketRight"), None);
    Ok(())
}

#[test]
fn mod_follows_the_loader_default_when_the_config_has_no_mod_key() -> Result<(), String> {
    let alt = load_with(
        &CdlLoader::new().with_default_mod_key(ModKey::Alt),
        &example("niri.cdl"),
    )?;
    assert_eq!(alt.mod_key, ModKey::Alt);
    assert_eq!(
        alt.resolve_key(
            Mods {
                alt: true,
                ..Mods::default()
            },
            "t"
        ),
        Some(Action::Spawn("alacritty".to_owned()))
    );
    assert_eq!(alt.resolve_key(super_mods(), "t"), None);
    Ok(())
}

#[test]
fn explicit_mod_key_beats_the_loader_default() -> Result<(), String> {
    let head =
        "\"use cdl 1\";\nimport { Contrast, Keybind } from \"host:contrast\";\nexport default (\n";
    let tail = "    <Keybind keys=\"mod+q\" action=\"close\" />\n  </Contrast>\n);\n";
    let explicit_alt = write_temp(
        "explicit_alt",
        &format!("{head}  <Contrast modKey=\"alt\">\n{tail}"),
    )?;
    let none = write_temp("none", &format!("{head}  <Contrast>\n{tail}"))?;

    let super_default = CdlLoader::new().with_default_mod_key(ModKey::Super);
    let explicit = load_with(&super_default, &explicit_alt)?;
    assert_eq!(explicit.mod_key, ModKey::Alt);
    assert_eq!(
        explicit.resolve_key(
            Mods {
                alt: true,
                ..Mods::default()
            },
            "q"
        ),
        Some(Action::CloseFocused)
    );

    let omitted = load_with(&super_default, &none)?;
    assert_eq!(omitted.mod_key, ModKey::Super);
    assert_eq!(
        omitted.resolve_key(super_mods(), "q"),
        Some(Action::CloseFocused)
    );

    // 既定は Alt（`CdlLoader::new()`）。
    assert_eq!(load_with(&CdlLoader::new(), &none)?.mod_key, ModKey::Alt);
    Ok(())
}

#[test]
fn niri_example_carries_the_niri_look() -> Result<(), String> {
    let loader = CdlLoader::new().with_default_mod_key(ModKey::Super);
    let s = load_with(&loader, &example("niri.cdl"))?;
    assert_eq!((s.layout.gap, s.layout.outer_gap), (16, 16));
    assert_eq!(s.layout.mode, contrast_core::LayoutMode::Scrolling);
    assert_eq!(
        s.layout.default_column_width,
        contrast_core::Ratio::new(500, 1000)
    );
    assert_eq!(s.layout.preset_column_widths.len(), 3);
    assert_eq!(s.appearance.border_width, 4);
    assert_eq!(s.appearance.radius, 0);
    // niri の focus-ring の inactive-color はフォーカス外の窓には描かれないので、borderColor は無い。
    assert!(s.appearance.border_color.is_none());
    assert!(!s.input.disable_while_typing);
    assert_eq!(s.cursor.theme.as_deref(), Some("Cecilia"));
    assert_eq!(s.cursor.size, Some(24));
    assert!(s.input.tap_to_click && s.input.natural_scroll);
    Ok(())
}

#[test]
fn niri_example_has_the_startup_commands_and_environment() -> Result<(), String> {
    let settings = load_with(&CdlLoader::new(), &example("niri.cdl"))?;
    // niri の config.kdl の spawn-at-startup（5 つ、この順）。
    // `ipc="niri"`（NIRI_SOCKET を渡す）のは ironbar だけ。
    let ipc: Vec<Option<&str>> = settings
        .startup
        .iter()
        .filter(|c| c.niri_ipc)
        .map(|c| c.command.split_whitespace().next())
        .collect();
    assert_eq!(ipc, [Some("ironbar")]);
    let startup: Vec<&str> = settings
        .startup
        .iter()
        .map(|c| c.command.as_str())
        .collect();
    assert_eq!(
        startup,
        [
            "fcitx5",
            "ironbar -c /home/user/.config/ironbar/niri-v8.toml -t /home/user/.config/ironbar/niri-v9.css",
            "swaybg -i /home/user/Pictures/wallpaper/cece.jpg",
            "mako",
            "udiskie",
        ]
    );
    // environment（4 つと、niri がセッションとして設定する XDG_* の 2 つ）。
    let env: Vec<(&str, &str)> = settings
        .environment
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    assert_eq!(
        env,
        [
            ("GTK_IM_MODULE", "fcitx5"),
            ("MOZ_ENABLE_WAYLAND", "1"),
            ("QT_IM_MODULE", "fcitx5"),
            ("SDL_IM_MODULE", "fcitx5"),
            ("XDG_CURRENT_DESKTOP", "niri"),
            ("XDG_SESSION_TYPE", "wayland"),
        ]
    );
    // niri の config は focus-follows-mouse も cursor の hide も使わない。
    assert!(!settings.input.focus_follows_mouse);
    assert_eq!(settings.cursor.hide_after_inactive_ms, None);
    Ok(())
}

#[test]
fn niri_example_binds_focus_column_first_and_last() -> Result<(), String> {
    let loader = CdlLoader::new().with_default_mod_key(ModKey::Super);
    let settings = load_with(&loader, &example("niri.cdl"))?;
    let mods = super_mods();
    assert_eq!(
        settings.resolve_key(mods, "Home"),
        Some(Action::FocusColumnFirst)
    );
    assert_eq!(
        settings.resolve_key(mods, "End"),
        Some(Action::FocusColumnLast)
    );
    Ok(())
}
