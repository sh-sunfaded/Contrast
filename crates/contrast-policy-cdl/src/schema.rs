//! Contrast のスキーマ（`host:contrast`）を `cdl-host` のビルダーで登録する。内容は `docs/cdl-contract.md` の 1・2 節。
//!
//! `modKey` と `action` は本来リテラルユニオンだが、`cdl-host` が `Literal` を公開していないので
//! `string` にして、`convert` で検査する。

use cdl_host::{ComponentDef, Field, Schema, Ty};

/// ホストモジュール名。
const MODULE: &str = "host:contrast";

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
        .component(
            MODULE,
            ComponentDef::new("Contrast")
                .optional_prop("modKey", Ty::String)
                .children(&[
                    "Appearance",
                    "Workspaces",
                    "Layout",
                    "Cursor",
                    "Screenshot",
                    "Input",
                    "Startup",
                    "DataControlClient",
                    "Environment",
                    "Keybind",
                ])
                .unique_child("Environment")
                .unique_child("Appearance")
                .unique_child("Workspaces")
                .unique_child("Layout")
                .unique_child("Cursor")
                .unique_child("Screenshot")
                .unique_child("Input"),
        )
        .component(
            MODULE,
            ComponentDef::new("Appearance")
                .prop("borderWidth", Ty::Length)
                .prop("focusColor", Ty::Color)
                .optional_prop("borderColor", Ty::Color)
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
            ComponentDef::new("Layout")
                .optional_prop("gap", Ty::Length)
                .optional_prop("outerGap", Ty::Length)
                .optional_prop("mode", Ty::String)
                .optional_prop("defaultColumnWidth", Ty::Number)
                .optional_prop("presetColumnWidths", Ty::String)
                .optional_prop("windowHeightPresets", Ty::String)
                .optional_prop("animationMs", Ty::Number)
                .optional_prop("animation", Ty::String),
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
            ComponentDef::new("Startup")
                .prop("command", Ty::String)
                .optional_prop("ipc", Ty::String),
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
                .optional_prop("mode", Ty::String),
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
        assert!(contrast_schema().is_ok());
    }
}
