//! Contrast のスキーマ（`host:contrast`）を `cdl-host` のビルダーで登録する。内容は `docs/cdl-contract.md` の 1・2 節。
//!
//! 列挙の prop（`modKey`・`Layout.mode`・`Startup.ipc`・`Workspaces.mode`）は
//! 文字列リテラルのユニオンで宣言し、範囲外は CDL が評価前に E4004 で拒否する。
//! `Keybind.action` は未知の action の案内文（CT020）を保つため `string` のままにして、`convert` で検査する。

use cdl_host::{ComponentDef, Field, Literal, Schema, Ty};

/// 型 `Env` を公開するホストモジュール名（`import type { Env } from "host:contrast-env"`）。
/// `host:contrast` には要素 `Env`（`Environment` の子）があり、CDL は同じモジュールで要素と型の名前が重なるのを許さない
/// ので、別のモジュールにする。
pub(crate) const ENV_MODULE: &str = "host:contrast-env";

/// ホストモジュール名。
const MODULE: &str = "host:contrast";

/// 文字列リテラルのユニオン（`"a" | "b"`）。
fn one_of(values: &[&str]) -> Ty {
    Ty::union(
        values
            .iter()
            .map(|v| Ty::Literal(Literal::String((*v).to_owned())))
            .collect(),
    )
}

/// `Motion` の子（種別）の定義。`curve`（曲線名。`string` のまま CT071 で検査する）と `duration`（`ms(150)`）。
fn motion_kind(name: &str) -> ComponentDef {
    ComponentDef::new(name)
        .optional_prop("curve", Ty::String)
        .optional_prop("duration", Ty::Duration)
}

/// `Env`（`docs/ui-spec.md` §4.3）の型。読み取り専用の入力で、関数形ルート `function App(env: Env)` が受ける。
/// `title` は入れない（変化が頻繁なため）。省略できるのは `workspace.name` と `focus`（読むときは `?.`）。
fn env_type() -> Ty {
    Ty::object([
        ("profile", Field::required(Ty::String)),
        ("mode", Field::required(Ty::String)),
        (
            "output",
            Field::required(Ty::object([("name", Field::required(Ty::String))])),
        ),
        (
            "workspace",
            Field::required(Ty::object([
                ("index", Field::required(Ty::Number)),
                ("name", Field::optional(Ty::String)),
                ("windowCount", Field::required(Ty::Number)),
            ])),
        ),
        (
            "focus",
            Field::optional(Ty::object([
                ("appId", Field::required(Ty::String)),
                ("isDialog", Field::required(Ty::Boolean)),
                (
                    "state",
                    Field::required(one_of(&["tiled", "floating", "fullscreen"])),
                ),
            ])),
        ),
    ])
}

/// スキーマを組み立てる。失敗は `Schema::build` が返す誤りの一覧（実装の不具合を意味する）。
pub(crate) fn contrast_schema() -> Result<Schema, Vec<String>> {
    let window_info = Ty::object([
        ("appId", Field::required(Ty::String)),
        ("title", Field::optional(Ty::String)),
        ("isDialog", Field::required(Ty::Boolean)),
        ("workspace", Field::required(Ty::Number)),
    ]);
    let placement = Ty::object([
        ("workspace", Field::optional(Ty::Number)),
        ("floating", Field::required(Ty::Boolean)),
        ("focus", Field::optional(Ty::Boolean)),
        ("fullscreen", Field::optional(Ty::Boolean)),
        ("columnWidth", Field::optional(Ty::String)),
    ]);
    Schema::builder()
        .root(MODULE, "Contrast")
        .env(ENV_MODULE, env_type())
        .component(
            MODULE,
            ComponentDef::new("Contrast")
                .optional_prop("modKey", one_of(&["alt", "super"]))
                .children(&[
                    "Appearance",
                    "Tiers",
                    "Workspaces",
                    "Layout",
                    "Cursor",
                    "Screenshot",
                    "HotkeyOverlay",
                    "Input",
                    "Startup",
                    "DataControlClient",
                    "Environment",
                    "Keybind",
                    "Motion",
                    "Mode",
                ])
                .unique_child("Environment")
                .unique_child("Appearance")
                .unique_child("Tiers")
                .unique_child("Workspaces")
                .unique_child("Layout")
                .unique_child("Cursor")
                .unique_child("Screenshot")
                .unique_child("HotkeyOverlay")
                .unique_child("Input")
                .unique_child("Motion"),
        )
        // `Motion` の種別の一意性は CDL の `unique_child` ではなく、変換の CT073 で守る（診断コードを保つため）。
        .component(
            MODULE,
            ComponentDef::new("Motion").children(&[
                "Scroll",
                "Resize",
                "Workspace",
                "Focus",
                "Open",
                "Close",
            ]),
        )
        .component(MODULE, motion_kind("Scroll"))
        .component(MODULE, motion_kind("Resize"))
        // `<Workspace>` は、`<Motion>` の中では種別（`curve`・`duration`）、`<Workspaces>` の中では名前表の要素（`name` と子 `Launch`）。
        // CDL の要素は名前で 1 つに決まるので、両方の props を持ち、どちらで使えるかは変換が検査する。
        .component(
            MODULE,
            motion_kind("Workspace")
                .optional_prop("name", Ty::String)
                .children(&["Launch"]),
        )
        .component(
            MODULE,
            ComponentDef::new("Launch").prop("command", Ty::String),
        )
        .component(
            MODULE,
            ComponentDef::new("Mode")
                .prop("name", Ty::String)
                .optional_prop("fallthrough", Ty::Boolean),
        )
        .component(MODULE, motion_kind("Focus"))
        .component(MODULE, motion_kind("Open"))
        .component(MODULE, motion_kind("Close"))
        .component(
            MODULE,
            ComponentDef::new("Appearance")
                .optional_prop("borderWidth", Ty::Length)
                .optional_prop("background", Ty::Color)
                .optional_prop("radius", Ty::Length)
                .optional_prop("shadowBlur", Ty::Length)
                .optional_prop("shadowOffsetX", Ty::Length)
                .optional_prop("shadowOffsetY", Ty::Length)
                .optional_prop("shadowColor", Ty::Color)
                .optional_prop("inactiveBorderWidth", Ty::Length),
        )
        .component(
            MODULE,
            ComponentDef::new("Tiers")
                .children(&["FocusTier", "Tier"])
                .unique_child("FocusTier"),
        )
        .component(
            MODULE,
            ComponentDef::new("FocusTier")
                .prop("l", Ty::Number)
                .optional_prop("ringColor", Ty::Color),
        )
        .component(
            MODULE,
            ComponentDef::new("Tier")
                .prop("l", Ty::Number)
                .optional_prop("ringColor", Ty::Color)
                .optional_prop("name", Ty::String),
        )
        .component(
            MODULE,
            ComponentDef::new("Layout")
                .optional_prop("gap", Ty::Length)
                .optional_prop("outerGap", Ty::Length)
                .optional_prop("mode", one_of(&["equal", "scrolling"]))
                .optional_prop("defaultColumnWidth", Ty::Number)
                .optional_prop("presetColumnWidths", Ty::String)
                .optional_prop("windowHeightPresets", Ty::String),
        )
        .component(
            MODULE,
            ComponentDef::new("Cursor")
                .optional_prop("theme", Ty::String)
                .optional_prop("size", Ty::Number)
                .optional_prop("hideAfterInactiveMs", Ty::Number),
        )
        .component(
            MODULE,
            ComponentDef::new("Screenshot").optional_prop("path", Ty::String),
        )
        .component(
            MODULE,
            ComponentDef::new("HotkeyOverlay")
                .optional_prop("skipAtStartup", Ty::Boolean)
                .optional_prop("hideNotBound", Ty::Boolean),
        )
        .component(
            MODULE,
            ComponentDef::new("Startup")
                .prop("command", Ty::String)
                .optional_prop("ipc", one_of(&["niri"])),
        )
        .component(
            MODULE,
            ComponentDef::new("DataControlClient").prop("name", Ty::String),
        )
        .component(MODULE, ComponentDef::new("Environment").children(&["Env"]))
        .component(
            MODULE,
            ComponentDef::new("Env")
                .prop("name", Ty::String)
                .prop("value", Ty::String),
        )
        .component(
            MODULE,
            ComponentDef::new("Workspaces")
                .prop("count", Ty::Number)
                .optional_prop("mode", one_of(&["fixed", "dynamic"]))
                .children(&["Workspace"]),
        )
        .component(
            MODULE,
            ComponentDef::new("Input")
                .optional_prop("tapToClick", Ty::Boolean)
                .optional_prop("naturalScroll", Ty::Boolean)
                .optional_prop("accelSpeed", Ty::Number)
                .optional_prop("disableWhileTyping", Ty::Boolean)
                .optional_prop("focusFollowsMouse", Ty::Boolean),
        )
        .component(
            MODULE,
            ComponentDef::new("Keybind")
                .prop("keys", Ty::String)
                .prop("action", Ty::String)
                .optional_prop("command", Ty::String)
                .optional_prop("workspace", Ty::Number)
                .optional_prop("width", Ty::Number)
                .optional_prop("delta", Ty::Number)
                .optional_prop("height", Ty::Number)
                .optional_prop("heightPx", Ty::Number)
                .optional_prop("mode", Ty::String)
                .optional_prop("name", Ty::String)
                .optional_prop("cooldown", Ty::Number)
                .optional_prop("focus", Ty::Boolean),
        )
        .type_def(MODULE, "WindowInfo", window_info.clone())
        .type_def(MODULE, "Placement", placement.clone())
        .type_def(
            MODULE,
            "Policy",
            Ty::object([(
                "windowOpen",
                Field::optional(Ty::Function(vec![window_info], Box::new(placement))),
            )]),
        )
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_builds() {
        let r = contrast_schema();
        assert!(r.is_ok(), "{:?}", r.err());
    }
}
