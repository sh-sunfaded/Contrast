//! 評価後の値木（`Contrast` 要素）を `Settings` にする。
//!
//! 誤りは見つけた順に全部集める。1 つでもあれば `Err` を返し、半端な `Settings` は作らない。
//! 値木の位置は要素単位（props には位置がない）なので、診断は誤りのある prop を含む要素の位置を指し、
//! メッセージに prop 名を書く。

use std::collections::BTreeMap;

use cdl_host::{Node, Px, Rgba};
use contrast_core::{LayoutMode, Ratio, WindowHeight};
use contrast_policy::settings::{
    MAX_ANIMATION_MS, MAX_BORDER_WIDTH, MAX_DECORATION_PX, MAX_HIDE_AFTER_INACTIVE_MS,
    MAX_WINDOW_HEIGHT_PX, MAX_WORKSPACES, rgba8,
};
use contrast_policy::{Action, ConfigDiagnostic, KeyBinding, KeySpec, ModKey, Settings, codes};

use crate::diag::{at, convert};
use crate::keys::parse_key_spec;

/// 変換の文脈: 診断の行・列を求めるためのソース本文と、集めた診断。
struct Ctx<'a> {
    /// id → ソース本文。
    sources: &'a BTreeMap<String, String>,
    /// これまでに集めた診断。
    diags: Vec<ConfigDiagnostic>,
}

impl Ctx<'_> {
    /// `node` の位置に診断を足す。
    fn error(&mut self, node: Node<'_>, code: &str, message: impl Into<String>) {
        let span = &node.element().span;
        self.diags.push(at(span, code, message, self.sources));
    }

    /// 必須の prop を読む。読めなければ診断を足して `None`。
    fn prop<T: cdl_host::FromValue>(&mut self, node: Node<'_>, key: &str) -> Option<T> {
        match node.prop(key) {
            Ok(v) => Some(v),
            Err(d) => {
                self.diags.push(convert(&d, self.sources));
                None
            }
        }
    }

    /// 省略できる prop を読む。型が違えば診断を足して `None`。
    fn optional<T: cdl_host::FromValue>(&mut self, node: Node<'_>, key: &str) -> Option<T> {
        match node.optional_prop(key) {
            Ok(v) => v,
            Err(d) => {
                self.diags.push(convert(&d, self.sources));
                None
            }
        }
    }
}

/// 重複検査用のキー: (alt, shift, ctrl, super, キー名)。
type KeyId = (bool, bool, bool, bool, String);

/// `KeySpec` を重複検査用のキーにする。
fn key_id(k: &KeySpec) -> KeyId {
    (
        k.mods.alt,
        k.mods.shift,
        k.mods.ctrl,
        k.mods.logo,
        k.keysym.clone(),
    )
}

/// `n` が `lo..=hi` の整数ならその値。
fn integer_in(n: f64, lo: u32, hi: u32) -> Option<u32> {
    if !n.is_finite() || n.fract() != 0.0 || n < f64::from(lo) || n > f64::from(hi) {
        return None;
    }
    Some(n as u32)
}

/// 1 始まりのワークスペース番号 `n`（`1..=count`）を 0 始まりの `u8` にする。
fn workspace_index(n: f64, count: u8) -> Option<u8> {
    integer_in(n, 1, u32::from(count))
        .and_then(|v| v.checked_sub(1))
        .and_then(|v| u8::try_from(v).ok())
}

/// `Contrast` 要素を `Settings` にする。`sources` は診断の位置の計算に使う（id → 本文）。
pub(crate) fn to_settings(
    root: Node<'_>,
    sources: &BTreeMap<String, String>,
    default_mod_key: ModKey,
) -> Result<Settings, Vec<ConfigDiagnostic>> {
    let mut cx = Ctx {
        sources,
        diags: Vec::new(),
    };
    let mut settings = Settings::builtin();
    settings.keybinds.clear();

    if let Err(d) = root.expect("Contrast") {
        return Err(vec![convert(&d, sources)]);
    }
    settings.mod_key = read_mod_key(&mut cx, root, default_mod_key);

    // 1 回目: Appearance と Workspaces（キーバインドの検査に数が要る）。
    let mut keybind_nodes = Vec::new();
    let mut data_control_nodes = Vec::new();
    for child in root.children() {
        match child.name() {
            "Appearance" => read_appearance(&mut cx, child, &mut settings),
            "Workspaces" => read_workspaces(&mut cx, child, &mut settings),
            "Layout" => read_layout(&mut cx, child, &mut settings),
            "Cursor" => read_cursor(&mut cx, child, &mut settings),
            "Input" => read_input(&mut cx, child, &mut settings),
            "Startup" => read_startup(&mut cx, child, &mut settings),
            "DataControlClient" => data_control_nodes.push(child),
            "Environment" => read_environment(&mut cx, child, &mut settings),
            "Keybind" => keybind_nodes.push(child),
            other => cx.error(
                child,
                codes::TREE_MISMATCH,
                format!("未知の要素 `<{other}>`"),
            ),
        }
    }

    read_data_control_clients(&mut cx, &data_control_nodes, &mut settings);

    // 2 回目: Keybind。
    let mut seen: BTreeMap<KeyId, (u32, u32)> = BTreeMap::new();
    for node in keybind_nodes {
        let Some(binding) = read_keybind(&mut cx, node, &settings) else {
            continue;
        };
        let here = position(&cx, node);
        let id = key_id(&binding.key);
        if let Some((line, col)) = seen.get(&id) {
            cx.error(
                node,
                codes::DUPLICATE_KEY,
                format!(
                    "キー `{}` は {line}:{col} でも使っている",
                    prop_text(node, "keys")
                ),
            );
            continue;
        }
        seen.insert(id, here);
        settings.keybinds.push(binding);
    }

    if cx.diags.is_empty() {
        Ok(settings)
    } else {
        Err(cx.diags)
    }
}

/// 要素の行・列（求められなければ 0:0）。
fn position(cx: &Ctx<'_>, node: Node<'_>) -> (u32, u32) {
    let span = &node.element().span;
    cx.sources
        .get(&span.file)
        .and_then(|text| crate::diag::line_column(text, span.start))
        .unwrap_or((0, 0))
}

/// 文字列 prop の値（診断のメッセージ用。無ければ空）。
fn prop_text(node: Node<'_>, key: &str) -> String {
    node.optional_prop::<String>(key)
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// `Contrast` の `modKey`。省略したときは `default`（環境変数・バックエンドで決まった既定）。
fn read_mod_key(cx: &mut Ctx<'_>, root: Node<'_>, default: ModKey) -> ModKey {
    match cx.optional::<String>(root, "modKey").as_deref() {
        None => default,
        Some("alt") => ModKey::Alt,
        Some("super") => ModKey::Super,
        Some(other) => {
            cx.error(
                root,
                codes::BAD_PROP,
                format!("`modKey` は \"alt\" か \"super\"（\"{other}\" は使えない）"),
            );
            default
        }
    }
}

/// `Appearance` を読む。
fn read_appearance(cx: &mut Ctx<'_>, node: Node<'_>, settings: &mut Settings) {
    if let Some(Px(width)) = cx.prop::<Px>(node, "borderWidth") {
        let max = u32::try_from(MAX_BORDER_WIDTH).unwrap_or(0);
        match integer_in(width, 0, max).and_then(|w| i32::try_from(w).ok()) {
            Some(w) => settings.appearance.border_width = w,
            None => cx.error(
                node,
                codes::BAD_PROP,
                format!("`borderWidth` は 0〜{MAX_BORDER_WIDTH}px の整数（{width}px は使えない）"),
            ),
        }
    }
    if let Some(Rgba(c)) = cx.prop::<Rgba>(node, "focusColor") {
        settings.appearance.focus_color = rgba8(c);
    }
    if let Some(Rgba(c)) = cx.optional::<Rgba>(node, "borderColor") {
        settings.appearance.border_color = Some(rgba8(c));
    }
    if let Some(Rgba(c)) = cx.optional::<Rgba>(node, "background") {
        settings.appearance.background = rgba8(c);
    }
    if let Some(v) = optional_px(cx, node, "radius") {
        settings.appearance.radius = v;
    }
    if let Some(v) = optional_px(cx, node, "shadowBlur") {
        settings.appearance.shadow.blur = v;
    }
    if let Some(v) = optional_offset(cx, node, "shadowOffsetX") {
        settings.appearance.shadow.offset_x = v;
    }
    if let Some(v) = optional_offset(cx, node, "shadowOffsetY") {
        settings.appearance.shadow.offset_y = v;
    }
    if let Some(Rgba(c)) = cx.optional::<Rgba>(node, "shadowColor") {
        settings.appearance.shadow.color = rgba8(c);
    }
    if let Some(v) = optional_px(cx, node, "inactiveBorderWidth") {
        settings.appearance.inactive_border_width = Some(v);
    }
}

/// 長さの prop（省略可）を、0〜`MAX_DECORATION_PX` の整数 px で読む。範囲外は診断を足して `None`。
/// 影のずれは負も使えるので、`optional_offset` を使う。
fn optional_px(cx: &mut Ctx<'_>, node: Node<'_>, key: &str) -> Option<i32> {
    let Px(v) = cx.optional::<Px>(node, key)?;
    let max = u32::try_from(MAX_DECORATION_PX).unwrap_or(0);
    match integer_in(v, 0, max).and_then(|n| i32::try_from(n).ok()) {
        Some(n) => Some(n),
        None => {
            cx.error(
                node,
                codes::BAD_PROP,
                format!("`{key}` は 0〜{MAX_DECORATION_PX}px の整数（{v}px は使えない）"),
            );
            None
        }
    }
}

/// 長さの prop（省略可）を、-`MAX_DECORATION_PX`〜`MAX_DECORATION_PX` の整数 px で読む（影のずれ用）。範囲外は診断を足して `None`。
fn optional_offset(cx: &mut Ctx<'_>, node: Node<'_>, key: &str) -> Option<i32> {
    let Px(v) = cx.optional::<Px>(node, key)?;
    let max = f64::from(MAX_DECORATION_PX);
    if v.is_finite() && v.fract() == 0.0 && (-max..=max).contains(&v) {
        return Some(v as i32);
    }
    cx.error(
        node,
        codes::BAD_PROP,
        format!(
            "`{key}` は -{MAX_DECORATION_PX}〜{MAX_DECORATION_PX}px の整数（{v}px は使えない）"
        ),
    );
    None
}

/// `Layout` を読む。
fn read_layout(cx: &mut Ctx<'_>, node: Node<'_>, settings: &mut Settings) {
    if let Some(v) = optional_px(cx, node, "gap") {
        settings.layout.gap = v;
    }
    if let Some(v) = optional_px(cx, node, "outerGap") {
        settings.layout.outer_gap = v;
    }
    if let Some(mode) = cx.optional::<String>(node, "mode") {
        match mode.as_str() {
            "equal" => settings.layout.mode = LayoutMode::Equal,
            "scrolling" => settings.layout.mode = LayoutMode::Scrolling,
            other => cx.error(
                node,
                codes::BAD_PROP,
                format!("`mode` は \"equal\" か \"scrolling\"（\"{other}\" は使えない）"),
            ),
        }
    }
    if let Some(w) = cx.optional::<f64>(node, "defaultColumnWidth") {
        match proportion(w) {
            Some(r) => settings.layout.default_column_width = Some(r),
            None => cx.error(
                node,
                codes::BAD_PROP,
                format!("`defaultColumnWidth` は 0 より大きく 1 以下の数（{w} は使えない）"),
            ),
        }
    }
    if let Some(ms) = cx.optional::<f64>(node, "animationMs") {
        match integer_in(ms, 0, MAX_ANIMATION_MS) {
            Some(n) => settings.layout.animation_ms = n,
            None => cx.error(
                node,
                codes::BAD_PROP,
                format!("`animationMs` は 0〜{MAX_ANIMATION_MS} の整数（{ms} は使えない）"),
            ),
        }
    }
    if let Some(text) = cx.optional::<String>(node, "presetColumnWidths") {
        match parse_presets(&text) {
            Some(list) => settings.layout.preset_column_widths = list,
            None => cx.error(
                node,
                codes::BAD_PROP,
                format!(
                    "`presetColumnWidths` は 0 より大きく 1 以下の数を、空白かコンマで区切って 1〜{MAX_PRESET_COLUMNS} 個（\"{text}\" は使えない）"
                ),
            ),
        }
    }
    if let Some(text) = cx.optional::<String>(node, "windowHeightPresets") {
        match parse_presets(&text) {
            Some(list) => settings.layout.window_height_presets = list,
            None => cx.error(
                node,
                codes::BAD_PROP,
                format!(
                    "`windowHeightPresets` は 0 より大きく 1 以下の数を、空白かコンマで区切って 1〜{MAX_PRESET_COLUMNS} 個（\"{text}\" は使えない）"
                ),
            ),
        }
    }
}

/// プリセットの列幅の最大数（core の `MAX_PRESETS` と同じ）。
const MAX_PRESET_COLUMNS: usize = 8;

/// 割合 `x`（0 より大きく 1 以下）を、分母 1000 の `Ratio` にする（小数第 4 位を四捨五入、1/1000 未満は 1/1000）。
fn proportion(x: f64) -> Option<Ratio> {
    if !x.is_finite() || x <= 0.0 || x > 1.0 {
        return None;
    }
    let num = (x * 1000.0).round().clamp(1.0, 1000.0);
    // `num` は 1.0〜1000.0 の整数値なので、切り捨てても変わらない。
    Ratio::new(num as u32, 1000)
}

/// パーセントの増減 `delta`（0 以外で -100〜100 の整数）。範囲外・非整数・なしは `None`。
fn percent_delta(delta: Option<f64>) -> Option<i8> {
    let d = delta?;
    let n = integer_in(d.abs(), 1, 100)?;
    let v = i8::try_from(n).ok()?;
    if d < 0.0 { v.checked_neg() } else { Some(v) }
}

/// 空白かコンマで区切った割合の並び。1〜`MAX_PRESET_COLUMNS` 個で、どれも `proportion` が通るときだけ。
fn parse_presets(text: &str) -> Option<Vec<Ratio>> {
    let list: Option<Vec<Ratio>> = text
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .map(|t| t.parse::<f64>().ok().and_then(proportion))
        .collect();
    list.filter(|l| !l.is_empty() && l.len() <= MAX_PRESET_COLUMNS)
}

/// `Cursor` を読む。`size` は 8〜256 の整数。
fn read_cursor(cx: &mut Ctx<'_>, node: Node<'_>, settings: &mut Settings) {
    if let Some(theme) = cx.optional::<String>(node, "theme") {
        settings.cursor.theme = Some(theme).filter(|t| !t.trim().is_empty());
    }
    if let Some(size) = cx.optional::<f64>(node, "size") {
        match integer_in(size, 8, 256) {
            Some(n) => settings.cursor.size = Some(n),
            None => cx.error(
                node,
                codes::BAD_PROP,
                format!("`size` は 8〜256 の整数（{size} は使えない）"),
            ),
        }
    }
    if let Some(ms) = cx.optional::<f64>(node, "hideAfterInactiveMs") {
        match integer_in(ms, 1, MAX_HIDE_AFTER_INACTIVE_MS) {
            Some(n) => settings.cursor.hide_after_inactive_ms = Some(n),
            None => cx.error(
                node,
                codes::BAD_PROP,
                format!(
                    "`hideAfterInactiveMs` は 1〜{MAX_HIDE_AFTER_INACTIVE_MS} の整数（{ms} は使えない）"
                ),
            ),
        }
    }
}

/// `Startup` を読む。`command` は空でない文字列（空白で区切った先頭がプログラム、残りが引数）。並びの順に足す。
fn read_startup(cx: &mut Ctx<'_>, node: Node<'_>, settings: &mut Settings) {
    let Some(command) = cx.prop::<String>(node, "command") else {
        return;
    };
    let command = command.trim().to_owned();
    if command.is_empty() {
        cx.error(node, codes::BAD_PROP, "`Startup` の `command` が空");
        return;
    }
    settings.startup.push(command);
}

/// `DataControlClient` を読む。`name` は wlr-data-control を使ってよいプログラム名（実行ファイルの名前）。
/// 1 つでも書くと既定の一覧を置き換える。空・空白や `/` を含む名前・重複は CT003。
fn read_data_control_clients(cx: &mut Ctx<'_>, nodes: &[Node<'_>], settings: &mut Settings) {
    if nodes.is_empty() {
        return;
    }
    let mut names: Vec<String> = Vec::new();
    for node in nodes {
        let Some(name) = cx.prop::<String>(*node, "name") else {
            continue;
        };
        let name = name.trim().to_owned();
        if name.is_empty()
            || name
                .chars()
                .any(|c| c.is_whitespace() || c == '/' || c == '\0')
        {
            cx.error(
                *node,
                codes::BAD_PROP,
                "`DataControlClient` の `name` は、空でなく、空白と `/` を含まない名前",
            );
        } else if names.contains(&name) {
            cx.error(
                *node,
                codes::BAD_PROP,
                format!("`DataControlClient` の `name` が重複している: {name}"),
            );
        } else {
            names.push(name);
        }
    }
    settings.data_control_clients = names;
}

/// `Environment` を読む。子は `Env`（`name` と `value`）。名前の重複・空の名前・`=` や NUL を含む名前は CT003。
fn read_environment(cx: &mut Ctx<'_>, node: Node<'_>, settings: &mut Settings) {
    for env in node.children() {
        if env.name() != "Env" {
            cx.error(
                env,
                codes::TREE_MISMATCH,
                format!(
                    "`<Environment>` の子は `<Env>` だけ（`<{}>` は置けない）",
                    env.name()
                ),
            );
            continue;
        }
        let name = cx.prop::<String>(env, "name");
        let value = cx.prop::<String>(env, "value");
        let (Some(name), Some(value)) = (name, value) else {
            continue;
        };
        if name.is_empty() || name.contains(['=', '\0']) || value.contains('\0') {
            cx.error(
                env,
                codes::BAD_PROP,
                format!("`Env` の名前は空でなく、`=` と NUL を含まない（\"{name}\"）。値も NUL を含まない"),
            );
            continue;
        }
        if settings.environment.insert(name.clone(), value).is_some() {
            cx.error(
                env,
                codes::BAD_PROP,
                format!("`Env` の名前 \"{name}\" が重複している"),
            );
        }
    }
}

/// `Workspaces` を読む。
fn read_workspaces(cx: &mut Ctx<'_>, node: Node<'_>, settings: &mut Settings) {
    let Some(count) = cx.prop::<f64>(node, "count") else {
        return;
    };
    match integer_in(count, 1, u32::from(MAX_WORKSPACES)).and_then(|n| u8::try_from(n).ok()) {
        Some(n) => settings.workspaces = n,
        None => cx.error(
            node,
            codes::BAD_WORKSPACE_COUNT,
            format!("`count` は 1〜{MAX_WORKSPACES} の整数（{count} は使えない）"),
        ),
    }
}

/// `Input` を読む。省略した prop は既定のまま。`accelSpeed` は -1〜1 の数。
fn read_input(cx: &mut Ctx<'_>, node: Node<'_>, settings: &mut Settings) {
    if let Some(v) = cx.optional::<bool>(node, "tapToClick") {
        settings.input.tap_to_click = v;
    }
    if let Some(v) = cx.optional::<bool>(node, "naturalScroll") {
        settings.input.natural_scroll = v;
    }
    if let Some(v) = cx.optional::<bool>(node, "disableWhileTyping") {
        settings.input.disable_while_typing = v;
    }
    if let Some(v) = cx.optional::<bool>(node, "focusFollowsMouse") {
        settings.input.focus_follows_mouse = v;
    }
    if let Some(speed) = cx.optional::<f64>(node, "accelSpeed") {
        if speed.is_finite() && (-1.0..=1.0).contains(&speed) {
            settings.input.accel_speed = speed;
        } else {
            cx.error(
                node,
                codes::BAD_PROP,
                format!("`accelSpeed` は -1〜1 の数（{speed} は使えない）"),
            );
        }
    }
}

/// `Keybind` を読む。誤りがあれば診断を足して `None`。
fn read_keybind(cx: &mut Ctx<'_>, node: Node<'_>, settings: &Settings) -> Option<KeyBinding> {
    let keys = cx.prop::<String>(node, "keys");
    let action = cx.prop::<String>(node, "action");
    let command = cx.optional::<String>(node, "command");
    let workspace = cx.optional::<f64>(node, "workspace");
    let width = cx.optional::<f64>(node, "width");
    let delta = cx.optional::<f64>(node, "delta");
    let height = cx.optional::<f64>(node, "height");
    let height_px = cx.optional::<f64>(node, "heightPx");
    let (keys, action) = (keys?, action?);

    let spec: Option<KeySpec> = match parse_key_spec(&keys, settings.mod_key) {
        Ok(spec) => Some(spec),
        Err(e) => {
            cx.error(node, codes::BAD_KEY, format!("`keys` \"{keys}\": {e}"));
            None
        }
    };
    let args = ActionArgs {
        command,
        workspace,
        width,
        delta,
        height,
        height_px,
        count: settings.workspaces,
    };
    let action = read_action(cx, node, &action, args);
    Some(KeyBinding {
        key: spec?,
        action: action?,
    })
}

/// `Keybind` の action に付く引数。
struct ActionArgs {
    /// `command`（`spawn`）。
    command: Option<String>,
    /// `workspace`（1 始まり）。
    workspace: Option<f64>,
    /// `width`（`set-column-width`。0 より大きく 1 以下の割合）。
    width: Option<f64>,
    /// `delta`（`adjust-column-width` と `adjust-window-height`。パーセント、0 以外で -100〜100 の整数）。
    delta: Option<f64>,
    /// `height`（`set-window-height`。0 より大きく 1 以下の割合。`height_px` とどちらか一方）。
    height: Option<f64>,
    /// `heightPx`（`set-window-height`。1〜`MAX_WINDOW_HEIGHT_PX` の整数 px。`height` とどちらか一方）。
    height_px: Option<f64>,
    /// ワークスペースの数。
    count: u8,
}

/// `action` の名前と引数から `Action` を作る。
fn read_action(cx: &mut Ctx<'_>, node: Node<'_>, name: &str, args: ActionArgs) -> Option<Action> {
    let ActionArgs {
        command,
        workspace,
        width,
        delta,
        height,
        height_px,
        count,
    } = args;
    let simple = match name {
        "close" => Some(Action::CloseFocused),
        "focus-left" => Some(Action::FocusPrev),
        "focus-right" => Some(Action::FocusNext),
        "move-left" => Some(Action::MoveLeft),
        "move-right" => Some(Action::MoveRight),
        "toggle-floating" => Some(Action::ToggleFloating),
        "toggle-fullscreen" => Some(Action::ToggleFullscreen),
        "workspace-next" => Some(Action::WorkspaceNext),
        "workspace-prev" => Some(Action::WorkspacePrev),
        "move-to-workspace-next" => Some(Action::MoveToWorkspaceNext),
        "move-to-workspace-prev" => Some(Action::MoveToWorkspacePrev),
        "reload-config" => Some(Action::ReloadConfig),
        "quit" => Some(Action::Quit),
        "focus-up" => Some(Action::FocusUp),
        "focus-down" => Some(Action::FocusDown),
        "move-up" => Some(Action::MoveUp),
        "move-down" => Some(Action::MoveDown),
        "focus-column-first" => Some(Action::FocusColumnFirst),
        "focus-column-last" => Some(Action::FocusColumnLast),
        "move-column-first" => Some(Action::MoveColumnFirst),
        "move-column-last" => Some(Action::MoveColumnLast),
        "consume-window-into-column" => Some(Action::ConsumeWindowIntoColumn),
        "expel-window-from-column" => Some(Action::ExpelWindowFromColumn),
        "switch-preset-column-width" => Some(Action::SwitchPresetColumnWidth),
        "maximize-column" => Some(Action::MaximizeColumn),
        "center-column" => Some(Action::CenterColumn),
        "switch-preset-window-height" => Some(Action::SwitchPresetWindowHeight),
        "reset-window-height" => Some(Action::ResetWindowHeight),
        _ => None,
    };
    if simple.is_some() {
        return simple;
    }
    match name {
        "spawn" => match command.map(|c| c.trim().to_owned()) {
            Some(c) if !c.is_empty() => Some(Action::Spawn(c)),
            _ => {
                cx.error(
                    node,
                    codes::MISSING_COMMAND,
                    "`spawn` には `command` が要る",
                );
                None
            }
        },
        "set-column-width" => match width.and_then(proportion) {
            Some(r) => Some(Action::SetColumnWidth(r)),
            None => {
                cx.error(
                    node,
                    codes::BAD_PROP,
                    "`set-column-width` には 0 より大きく 1 以下の `width` が要る",
                );
                None
            }
        },
        "adjust-column-width" | "adjust-window-height" => match percent_delta(delta) {
            Some(p) if name == "adjust-column-width" => Some(Action::AdjustColumnWidth(p)),
            Some(p) => Some(Action::AdjustWindowHeight(p)),
            None => {
                cx.error(
                    node,
                    codes::BAD_PROP,
                    format!(
                        "`{name}` には 0 以外で -100〜100 の整数の `delta`（パーセント）が要る"
                    ),
                );
                None
            }
        },
        "set-window-height" => {
            let height = match (height, height_px) {
                (Some(h), None) => proportion(h).map(WindowHeight::Proportion),
                (None, Some(px)) => integer_in(px, 1, MAX_WINDOW_HEIGHT_PX)
                    .and_then(|n| i32::try_from(n).ok())
                    .and_then(WindowHeight::fixed),
                _ => None,
            };
            match height {
                Some(h) => Some(Action::SetWindowHeight(h)),
                None => {
                    cx.error(
                        node,
                        codes::BAD_PROP,
                        format!(
                            "`set-window-height` には、0 より大きく 1 以下の `height`（割合）か、1〜{MAX_WINDOW_HEIGHT_PX} の整数の `heightPx` のどちらか一方が要る"
                        ),
                    );
                    None
                }
            }
        }
        "workspace" | "move-to-workspace" => {
            let Some(n) = workspace else {
                cx.error(
                    node,
                    codes::BAD_ACTION_WORKSPACE,
                    format!("`{name}` には `workspace` が要る"),
                );
                return None;
            };
            let Some(index) = workspace_index(n, count) else {
                cx.error(
                    node,
                    codes::BAD_ACTION_WORKSPACE,
                    format!("`workspace` は 1〜{count} の整数（{n} は使えない）"),
                );
                return None;
            };
            if name == "workspace" {
                Some(Action::SwitchWorkspace(index))
            } else {
                Some(Action::MoveToWorkspace(index))
            }
        }
        other => {
            cx.error(
                node,
                codes::UNKNOWN_ACTION,
                format!(
                    "未知の action \"{other}\"（spawn / close / focus-left / focus-right / move-left / move-right / \
                     workspace / move-to-workspace / workspace-next / workspace-prev / move-to-workspace-next / \
                     move-to-workspace-prev / focus-up / focus-down / move-up / move-down / focus-column-first / \
                     focus-column-last / move-column-first / move-column-last / consume-window-into-column / \
                     expel-window-from-column / switch-preset-column-width / maximize-column / set-column-width / \
                     adjust-column-width / center-column / switch-preset-window-height / reset-window-height / \
                     set-window-height / adjust-window-height / toggle-floating / toggle-fullscreen / reload-config / quit）"
                ),
            );
            None
        }
    }
}
