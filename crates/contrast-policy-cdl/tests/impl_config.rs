//! `config/`（niri の設定を CDL で再編した、作者の設定）のテスト: 診断ゼロで読めること、キーの重複がないこと、
//! バインドの数、主要なバインド、変換しなかった binds の数（`unsupported.cdl`）。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use contrast_policy::{Action, ConfigLoader, LoadOutcome, ModKey, Mods, Settings};
use contrast_policy_cdl::CdlLoader;

/// 変換したバインドの数（`docs/config-migration.md`）。
const CONVERTED: usize = 69;
/// 変換しなかったバインドの数（`unsupported.cdl` の `//>` の行）。
const UNSUPPORTED: usize = 42;
/// 元の niri の config.kdl の binds の数。
const NIRI_BINDS: usize = 111;

/// リポジトリの `config/` のパス。
fn config_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config")
}

/// `config/config.cdl` を `mod_key` を既定にして読む。診断があれば、その一覧をエラーにする。
fn load(mod_key: ModKey) -> Result<Settings, String> {
    let loader = CdlLoader::new().with_default_mod_key(mod_key);
    match loader.load(&config_dir().join("config.cdl")) {
        LoadOutcome::Loaded(c) => Ok(c.settings),
        LoadOutcome::Failed(d) => Err(d
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")),
        LoadOutcome::Absent => Err("config/config.cdl が無い".to_owned()),
    }
}

/// 修飾キーを `mod` に足した `Mods`。
fn with(mod_key: ModKey, f: impl FnOnce(&mut Mods)) -> Mods {
    let mut mods = mod_key.mods();
    f(&mut mods);
    mods
}

#[test]
fn the_config_loads_without_diagnostics() -> Result<(), String> {
    let s = load(ModKey::Super)?;
    assert_eq!(s.workspaces, 9);
    assert_eq!(s.layout.mode, contrast_core::LayoutMode::Scrolling);
    assert_eq!(s.layout.gap, 16);
    assert_eq!(s.layout.outer_gap, 16);
    assert_eq!(s.layout.animation_ms, 150);
    assert_eq!(s.layout.preset_column_widths.len(), 3);
    assert_eq!(s.appearance.border_width, 4);
    assert_eq!(s.appearance.radius, 0);
    assert_eq!(s.cursor.theme.as_deref(), Some("Cecilia"));
    assert_eq!(s.cursor.size, Some(24));
    assert!(s.input.tap_to_click && s.input.natural_scroll);
    assert!(!s.input.focus_follows_mouse);
    assert_eq!(s.cursor.hide_after_inactive_ms, None);
    Ok(())
}

#[test]
fn startup_and_environment_are_those_of_niri() -> Result<(), String> {
    let s = load(ModKey::Super)?;
    assert_eq!(
        s.startup,
        [
            "fcitx5",
            "ironbar -c /home/user/.config/ironbar/niri-v8.toml -t /home/user/.config/ironbar/niri-v9.css",
            "swaybg -i /home/user/Pictures/wallpaper/cece.jpg",
            "mako",
            "udiskie",
        ]
    );
    let env: Vec<(&str, &str)> = s
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
        ]
    );
    Ok(())
}

#[test]
fn the_number_of_binds_and_the_keys_are_unique() -> Result<(), String> {
    for mod_key in [ModKey::Super, ModKey::Alt] {
        let s = load(mod_key)?;
        assert_eq!(s.keybinds.len(), CONVERTED, "{mod_key:?}");
        let mut seen = BTreeSet::new();
        for b in &s.keybinds {
            let m = b.key.mods;
            let id = (
                m.alt,
                m.shift,
                m.ctrl,
                m.logo,
                b.key.keysym.to_ascii_lowercase(),
            );
            assert!(seen.insert(id), "duplicate key {:?} ({mod_key:?})", b.key);
        }
    }
    Ok(())
}

#[test]
fn converted_plus_unsupported_is_the_number_of_niri_binds() -> Result<(), String> {
    let text =
        std::fs::read_to_string(config_dir().join("unsupported.cdl")).map_err(|e| e.to_string())?;
    let unsupported = text.lines().filter(|l| l.starts_with("//> ")).count();
    assert_eq!(unsupported, UNSUPPORTED);
    assert_eq!(CONVERTED + unsupported, NIRI_BINDS);
    Ok(())
}

#[test]
fn the_main_binds_resolve_to_the_expected_actions() -> Result<(), String> {
    let m = ModKey::Super;
    let s = load(m)?;
    let spawn = |c: &str| Some(Action::Spawn(c.to_owned()));
    let mod_only = m.mods();
    let ctrl = with(m, |x| x.ctrl = true);
    let shift = with(m, |x| x.shift = true);

    assert_eq!(s.resolve_key(mod_only, "q"), Some(Action::CloseFocused));
    assert_eq!(
        s.resolve_key(mod_only, "1"),
        Some(Action::SwitchWorkspace(0))
    );
    assert_eq!(
        s.resolve_key(mod_only, "9"),
        Some(Action::SwitchWorkspace(8))
    );
    assert_eq!(s.resolve_key(ctrl, "1"), Some(Action::MoveToWorkspace(0)));
    assert_eq!(s.resolve_key(ctrl, "9"), Some(Action::MoveToWorkspace(8)));
    assert_eq!(s.resolve_key(mod_only, "t"), spawn("alacritty"));
    assert_eq!(s.resolve_key(mod_only, "d"), spawn("fuzzel"));
    assert_eq!(s.resolve_key(mod_only, "b"), spawn("firefox-nightly"));
    // 矢印と hjkl は同じ操作（左右は列、上下は列の中の窓）。
    for (arrow, vim, focus, move_) in [
        ("Left", "h", Action::FocusPrev, Action::MoveLeft),
        ("Right", "l", Action::FocusNext, Action::MoveRight),
        ("Up", "k", Action::FocusUp, Action::MoveUp),
        ("Down", "j", Action::FocusDown, Action::MoveDown),
    ] {
        assert_eq!(s.resolve_key(mod_only, arrow), Some(focus.clone()));
        assert_eq!(s.resolve_key(mod_only, vim), Some(focus));
        assert_eq!(s.resolve_key(ctrl, arrow), Some(move_.clone()));
        assert_eq!(s.resolve_key(ctrl, vim), Some(move_));
    }
    // Page_Up/Down と U/I は同じ（下 = 次、上 = 前）。
    for (page, vim, focus, move_) in [
        (
            "Page_Down",
            "u",
            Action::WorkspaceNext,
            Action::MoveToWorkspaceNext,
        ),
        (
            "Page_Up",
            "i",
            Action::WorkspacePrev,
            Action::MoveToWorkspacePrev,
        ),
    ] {
        assert_eq!(s.resolve_key(mod_only, page), Some(focus.clone()));
        assert_eq!(s.resolve_key(mod_only, vim), Some(focus));
        assert_eq!(s.resolve_key(ctrl, page), Some(move_.clone()));
        assert_eq!(s.resolve_key(ctrl, vim), Some(move_));
    }
    assert_eq!(
        s.resolve_key(mod_only, "Home"),
        Some(Action::FocusColumnFirst)
    );
    assert_eq!(
        s.resolve_key(mod_only, "End"),
        Some(Action::FocusColumnLast)
    );
    assert_eq!(s.resolve_key(ctrl, "Home"), Some(Action::MoveColumnFirst));
    assert_eq!(s.resolve_key(ctrl, "End"), Some(Action::MoveColumnLast));
    assert_eq!(
        s.resolve_key(mod_only, "Comma"),
        Some(Action::ConsumeWindowIntoColumn)
    );
    assert_eq!(
        s.resolve_key(mod_only, "Period"),
        Some(Action::ExpelWindowFromColumn)
    );
    assert_eq!(
        s.resolve_key(mod_only, "r"),
        Some(Action::SwitchPresetColumnWidth)
    );
    assert_eq!(
        s.resolve_key(shift, "r"),
        Some(Action::SwitchPresetWindowHeight)
    );
    assert_eq!(s.resolve_key(ctrl, "r"), Some(Action::ResetWindowHeight));
    assert_eq!(s.resolve_key(mod_only, "f"), Some(Action::MaximizeColumn));
    assert_eq!(s.resolve_key(shift, "f"), Some(Action::ToggleFullscreen));
    assert_eq!(s.resolve_key(mod_only, "c"), Some(Action::CenterColumn));
    assert_eq!(
        s.resolve_key(mod_only, "Minus"),
        Some(Action::AdjustColumnWidth(-10))
    );
    assert_eq!(
        s.resolve_key(mod_only, "Equal"),
        Some(Action::AdjustColumnWidth(10))
    );
    assert_eq!(
        s.resolve_key(shift, "Minus"),
        Some(Action::AdjustWindowHeight(-10))
    );
    assert_eq!(
        s.resolve_key(shift, "Equal"),
        Some(Action::AdjustWindowHeight(10))
    );
    assert_eq!(s.resolve_key(shift, "e"), Some(Action::Quit));
    // 音量などと日本語入力の切り替え。
    assert_eq!(
        s.resolve_key(Mods::default(), "XF86AudioMute"),
        spawn("wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle")
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
        s.resolve_key(
            Mods {
                logo: true,
                alt: true,
                ..Mods::default()
            },
            "j"
        ),
        spawn("env DISPLAY=:0 /home/user/jetbrains-toolbox-3.8.1.88030/bin/jetbrains-toolbox")
    );
    // 変換しなかったキーは何もしない（ホイール以外の例: swaylock、モニタ、スクリーンショット）。
    assert_eq!(
        s.resolve_key(
            Mods {
                logo: true,
                alt: true,
                ..Mods::default()
            },
            "l"
        ),
        None
    );
    assert_eq!(s.resolve_key(shift, "Left"), None);
    assert_eq!(s.resolve_key(Mods::default(), "Print"), None);
    assert_eq!(s.resolve_key(mod_only, "BracketLeft"), None);
    Ok(())
}
