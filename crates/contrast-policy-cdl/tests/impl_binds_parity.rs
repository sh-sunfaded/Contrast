//! `config/`（作者の niri の設定を CDL で再編したもの）のキー割り当てが、基準の `examples/niri.cdl` と
//! 同じキーに同じ操作を割り当てているかの比較。`mod` が Super（実機）と Alt（nested）の両方で、
//! 解決した結果をキーごとに突き合わせる。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use contrast_policy::{Action, ConfigLoader, KeyBinding, LoadOutcome, ModKey, Settings};
use contrast_policy_cdl::CdlLoader;

/// リポジトリのルートからの相対パスを、このクレートからのパスにする。
fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

/// `path` を、`mod_key` を `mod` の既定にして読む。診断があればエラー。
fn load(path: &Path, mod_key: ModKey) -> Result<Settings, String> {
    let loader = CdlLoader::new().with_default_mod_key(mod_key);
    match loader.load(path) {
        LoadOutcome::Loaded(c) => Ok(c.settings),
        LoadOutcome::Failed(d) => Err(d
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")),
        LoadOutcome::Absent => Err(format!("{} が無い", path.display())),
    }
}

/// キーの表示（`super+ctrl+shift+alt+キー名` の形）。
fn name(binding: &KeyBinding) -> String {
    let m = &binding.key.mods;
    let mut s = String::new();
    for (on, label) in [
        (m.logo, "super+"),
        (m.ctrl, "ctrl+"),
        (m.shift, "shift+"),
        (m.alt, "alt+"),
    ] {
        if on {
            s.push_str(label);
        }
    }
    s.push_str(&binding.key.keysym.to_lowercase());
    s
}

/// キー → 操作（`Debug` の文字列）。
fn table(settings: &Settings) -> BTreeMap<String, String> {
    settings
        .keybinds
        .iter()
        .map(|b| (name(b), format!("{:?}", b.action)))
        .collect()
}

/// `config/` と `examples/niri.cdl` の差（キー、どちらにあるか、操作）の一覧。
fn differences(mod_key: ModKey) -> Result<Vec<String>, String> {
    let config = table(&load(&repo("config/config.cdl"), mod_key)?);
    let example = table(&load(&repo("examples/niri.cdl"), mod_key)?);
    let mut out = Vec::new();
    for (k, v) in &example {
        match config.get(k) {
            None => out.push(format!("only in the example: {k} => {v}")),
            Some(c) if c != v => out.push(format!("different: {k}: example={v} config={c}")),
            Some(_) => {}
        }
    }
    for (k, v) in &config {
        if !example.contains_key(k) {
            out.push(format!("only in config/: {k} => {v}"));
        }
    }
    Ok(out)
}

#[test]
fn config_and_the_niri_example_bind_the_same_keys_to_the_same_actions() -> Result<(), String> {
    for mod_key in [ModKey::Super, ModKey::Alt] {
        assert_eq!(differences(mod_key)?, Vec::<String>::new(), "{mod_key:?}");
    }
    Ok(())
}

#[test]
fn the_consume_or_expel_keys_resolve_in_config() -> Result<(), String> {
    let m = ModKey::Super;
    let s = load(&repo("config/config.cdl"), m)?;
    assert_eq!(
        s.resolve_key(m.mods(), "BracketLeft"),
        Some(Action::ConsumeOrExpelWindowLeft)
    );
    assert_eq!(
        s.resolve_key(m.mods(), "BracketRight"),
        Some(Action::ConsumeOrExpelWindowRight)
    );
    Ok(())
}

#[test]
fn the_keys_that_were_missing_resolve_in_config() -> Result<(), String> {
    let m = ModKey::Super;
    let s = load(&repo("config/config.cdl"), m)?;
    let spawn = |c: &str| Some(Action::Spawn(c.to_owned()));
    let mut shift = m.mods();
    shift.shift = true;
    let mut alt = m.mods();
    alt.alt = true;
    assert_eq!(s.resolve_key(m.mods(), "v"), Some(Action::ToggleFloating));
    assert_eq!(
        s.resolve_key(shift, "d"),
        spawn("strat fedora /home/user/DiscordPTB/launch.sh")
    );
    assert_eq!(
        s.resolve_key(alt, "e"),
        spawn("strat fedora /home/user/eclipse/eclipse")
    );
    assert_eq!(s.resolve_key(alt, "l"), spawn("swaylock"));
    Ok(())
}
