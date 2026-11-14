//! キー層（mode）の経路決め（純粋）。押されたキーを、層に応じて「実行する操作・消費・素通し」のどれにするか決める。
//!
//! 守る不変条件（INV-K1）: 層の中では、修飾キーなしの `Escape` は**常に** `ExitMode`（設定で上書きできない）。
//! したがって、設定の誤りがあっても、層から必ず抜けられる。VT 切替・オーバービュー・スクリーンショット UI は、
//! この経路より先にキーを奪う（`input/mod.rs`）。

use contrast_policy::{Action, Mods, NORMAL_MODE, Policy};

/// キー 1 つの行き先。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KeyRoute {
    /// この操作を実行する（キーはクライアントへ渡さない）。
    Run(Action),
    /// 何もせず消費する（層の中の当たらないキー。クライアントへ渡さない）。
    Consume,
    /// クライアントへ渡す。
    Forward,
}

/// 修飾キーそのもの（押しただけでは何も起こさない）。層の中でも素通しする。
fn is_modifier_key(name: &str) -> bool {
    matches!(
        name,
        "Shift_L"
            | "Shift_R"
            | "Control_L"
            | "Control_R"
            | "Alt_L"
            | "Alt_R"
            | "Meta_L"
            | "Meta_R"
            | "Super_L"
            | "Super_R"
            | "Hyper_L"
            | "Hyper_R"
            | "ISO_Level3_Shift"
            | "ISO_Level5_Shift"
            | "Caps_Lock"
            | "Num_Lock"
    )
}

/// 層 `mode` で、修飾キー `mods`・キー名 `name` のキーの行き先を決める。
///
/// - `normal`: `Policy::resolve_key`（当たれば実行、当たらなければ素通し）。
/// - それ以外: 修飾キーなしの `Escape` は常に `ExitMode`。ほかは層のバインドを引き、当たらなければ、層が `fallthrough`
///   なら `normal` と同じに、そうでなければ消費する（修飾キーそのものは素通し）。
pub(crate) fn route_key(policy: &dyn Policy, mode: &str, mods: Mods, name: &str) -> KeyRoute {
    let normal = |mods: Mods| match policy.resolve_key(mods, name) {
        Some(action) => KeyRoute::Run(action),
        None => KeyRoute::Forward,
    };
    if mode == NORMAL_MODE {
        return normal(mods);
    }
    if mods == Mods::default() && name.eq_ignore_ascii_case("escape") {
        return KeyRoute::Run(Action::ExitMode);
    }
    if let Some(action) = policy.resolve_key_in_mode(mode, mods, name) {
        return KeyRoute::Run(action);
    }
    if policy.mode_fallthrough(mode) {
        return normal(mods);
    }
    if is_modifier_key(name) {
        KeyRoute::Forward
    } else {
        KeyRoute::Consume
    }
}

#[cfg(test)]
mod tests {
    use contrast_policy::{
        Config, ConfigStore, KeyBinding, KeySpec, ModeSettings, Settings, StaticPolicy,
    };

    use super::*;

    fn bind(mode: &str, key: &str, action: Action) -> KeyBinding {
        KeyBinding {
            mode: mode.to_owned(),
            key: KeySpec {
                mods: Mods::default(),
                keysym: key.to_owned(),
            },
            action,
            cooldown_ms: None,
        }
    }

    /// `resize` 層（`fallthrough` を選べる）と `normal` の `h`・`x` を持つポリシー。
    fn policy(fallthrough: bool) -> ConfigStore {
        let mut s = Settings::builtin();
        s.keybinds = vec![
            bind("normal", "h", Action::FocusNext),
            bind("normal", "x", Action::CloseFocused),
            bind("resize", "h", Action::FocusPrev),
            bind("resize", "q", Action::ExitMode),
        ];
        s.modes = vec![ModeSettings {
            name: "resize".to_owned(),
            fallthrough,
        }];
        ConfigStore::new(Config::new(s, None))
    }

    #[test]
    fn normal_runs_bound_keys_and_forwards_the_rest() {
        let p = policy(false);
        let none = Mods::default();
        assert_eq!(
            route_key(&p, "normal", none, "h"),
            KeyRoute::Run(Action::FocusNext)
        );
        assert_eq!(route_key(&p, "normal", none, "z"), KeyRoute::Forward);
        // normal の Escape は設定に従う（束縛が無ければ素通し）。
        assert_eq!(route_key(&p, "normal", none, "Escape"), KeyRoute::Forward);
    }

    #[test]
    fn a_mode_runs_its_own_bindings_and_not_the_normal_ones() {
        let p = policy(false);
        let none = Mods::default();
        assert_eq!(
            route_key(&p, "resize", none, "h"),
            KeyRoute::Run(Action::FocusPrev)
        );
        assert_eq!(
            route_key(&p, "resize", none, "q"),
            KeyRoute::Run(Action::ExitMode)
        );
        // 当たらないキーは消費する（`normal` の `x` も実行しない）。
        assert_eq!(route_key(&p, "resize", none, "x"), KeyRoute::Consume);
    }

    #[test]
    fn fallthrough_sends_unmatched_keys_to_normal() {
        let p = policy(true);
        let none = Mods::default();
        assert_eq!(
            route_key(&p, "resize", none, "x"),
            KeyRoute::Run(Action::CloseFocused)
        );
        assert_eq!(route_key(&p, "resize", none, "z"), KeyRoute::Forward);
        // 層のバインドが先。
        assert_eq!(
            route_key(&p, "resize", none, "h"),
            KeyRoute::Run(Action::FocusPrev)
        );
    }

    #[test]
    fn escape_always_leaves_a_mode_even_if_the_config_has_no_exit() {
        // 抜けるキーの無い層（変換の検査を通らない設定でも）。
        let mut s = Settings::builtin();
        s.keybinds = vec![bind("trap", "h", Action::FocusNext)];
        s.modes = vec![ModeSettings {
            name: "trap".to_owned(),
            fallthrough: false,
        }];
        let p = ConfigStore::new(Config::new(s, None));
        let none = Mods::default();
        assert_eq!(
            route_key(&p, "trap", none, "Escape"),
            KeyRoute::Run(Action::ExitMode)
        );
        assert_eq!(
            route_key(&p, "trap", none, "escape"),
            KeyRoute::Run(Action::ExitMode)
        );
        // 修飾キーつきの Escape は常設の脱出ではない。
        let shift = Mods {
            shift: true,
            ..Mods::default()
        };
        assert_eq!(route_key(&p, "trap", shift, "Escape"), KeyRoute::Consume);
    }

    #[test]
    fn modifier_keys_are_forwarded_inside_a_mode() {
        let p = policy(false);
        for name in ["Shift_L", "Control_R", "Alt_L", "Super_L", "Caps_Lock"] {
            assert_eq!(
                route_key(&p, "resize", Mods::default(), name),
                KeyRoute::Forward,
                "{name}"
            );
        }
    }

    #[test]
    fn a_policy_without_modes_only_knows_normal() {
        // 層を持たない実装（`resolve_key_in_mode` の既定）でも、`normal` の経路は変わらない。
        let p = StaticPolicy::new("foot");
        let alt = Mods {
            alt: true,
            ..Mods::default()
        };
        assert_eq!(
            route_key(&p, "normal", alt, "q"),
            KeyRoute::Run(Action::CloseFocused)
        );
        assert_eq!(route_key(&p, "ghost", alt, "q"), KeyRoute::Consume);
    }
}
