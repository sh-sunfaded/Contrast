//! ホットキーのオーバーレイ（niri の `show-hotkey-overlay` と、起動時の表示。`src/ui/hotkey_overlay.rs`）。
//!
//! 出す行と順は niri の `collect_actions`・`format_bind` と同じ: 決まった操作の並び（割り当てが無ければ "(not bound)"）、
//! スクリーンショット（割り当てがあるときだけ）、Mod か Super の付いた spawn（同じコマンドは 1 つ）。キーは各操作の最初のバインド。
//! 見た目も niri と同じ: 背景 (0.1, 0.1, 0.1)、白い文字、見出し "Important Hotkeys"（太字・中央）、キーは等幅で背景つき、
//! 外周に (0.5, 0.8, 1.0) の枠、出力の中央に透明度 0.9。文字は `text`（fontdue）で描く。
//! 起動時に開き（niri の `hotkey-overlay { skip-at-startup }` の既定）、キーを押す・ボタン・ホイール・ジェスチャーで閉じる。

use std::collections::BTreeMap;

use contrast_policy::{Action, KeyBinding, ModKey, Mods};
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::{
    ImportMem, Renderer,
    element::{Id, Kind, texture::TextureRenderElement},
    gles::{GlesRenderer, GlesTexture},
};
use smithay::input::keyboard::xkb;
use smithay::output::Output;
use smithay::utils::{Logical, Point, Size, Transform};

use crate::text::{Canvas, Color, fonts, line_height, text_width};

/// niri の定数（論理 px）。
const PADDING: f32 = 8.0;
const FONT_PX: f32 = 14.0;
const BORDER: f32 = 4.0;
const LINE_INTERVAL: f32 = 2.0;
const TITLE: &str = "Important Hotkeys";

/// 1 行: キーの表示（前後に空白）と、操作の表示（本文と、等幅で背景が黒の部分）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// キー。割り当てが無ければ " (not bound) "。
    pub key: String,
    /// 操作の名前。
    pub title: String,
    /// spawn のコマンド（等幅・黒い背景で、`title` の後ろに描く）。
    pub program: Option<String>,
}

/// 決まった操作（niri の並び）。`(Contrast の操作, niri の名前)`。
fn fixed_actions(binds: &[KeyBinding]) -> Vec<(Option<Action>, &'static str)> {
    let bound = |a: &Action| binds.iter().any(|b| b.action == *a);
    let mut out: Vec<(Option<Action>, &'static str)> = vec![
        (Some(Action::ShowHotkeyOverlay), "Show Important Hotkeys"),
        (Some(Action::Quit), "Exit niri"),
        (Some(Action::CloseFocused), "Close Focused Window"),
        (Some(Action::FocusPrev), "Focus Column to the Left"),
        (Some(Action::FocusNext), "Focus Column to the Right"),
        (Some(Action::MoveLeft), "Move Column Left"),
        (Some(Action::MoveRight), "Move Column Right"),
        (Some(Action::WorkspaceNext), "Switch Workspace Down"),
        (Some(Action::WorkspacePrev), "Switch Workspace Up"),
    ];
    // 列ごとの移動を優先し、無ければ窓の移動、どちらも無ければ列（niri と同じ）。
    if bound(&Action::MoveColumnToWorkspaceNext) || !bound(&Action::MoveToWorkspaceNext) {
        out.push((
            Some(Action::MoveColumnToWorkspaceNext),
            "Move Column to Workspace Down",
        ));
    } else {
        out.push((
            Some(Action::MoveToWorkspaceNext),
            "Move Window to Workspace Down",
        ));
    }
    if bound(&Action::MoveColumnToWorkspacePrev) || !bound(&Action::MoveToWorkspacePrev) {
        out.push((
            Some(Action::MoveColumnToWorkspacePrev),
            "Move Column to Workspace Up",
        ));
    } else {
        out.push((
            Some(Action::MoveToWorkspacePrev),
            "Move Window to Workspace Up",
        ));
    }
    out.extend([
        (
            Some(Action::SwitchPresetColumnWidth),
            "Switch Preset Column Widths",
        ),
        (Some(Action::MaximizeColumn), "Maximize Column"),
        (
            Some(Action::ConsumeOrExpelWindowLeft),
            "Consume or Expel Window Left",
        ),
        (
            Some(Action::ConsumeOrExpelWindowRight),
            "Consume or Expel Window Right",
        ),
        (
            Some(Action::ToggleFloating),
            "Move Window Between Floating and Tiling",
        ),
        // Contrast に無い操作は、niri で割り当てが無いときと同じく "(not bound)"。
        (None, "Switch Focus Between Floating and Tiling"),
        (None, "Open the Overview"),
    ]);
    if bound(&Action::Screenshot) {
        out.push((Some(Action::Screenshot), "Take a Screenshot"));
    }
    out
}

/// 修飾キーのうち、`mod` に当たるものが押されているか。
fn has_comp_mod(mods: Mods, mod_key: ModKey) -> bool {
    match mod_key {
        ModKey::Super => mods.logo,
        ModKey::Alt => mods.alt,
    }
}

/// niri の `prettify_keysym_name`（画面表示）。`name` は xkb の正式な名前。
fn prettify(name: &str) -> String {
    let name = match name {
        "slash" => "/",
        "comma" => ",",
        "period" => ".",
        "minus" => "-",
        "equal" => "=",
        "grave" => "`",
        "bracketleft" => "[",
        "bracketright" => "]",
        other => other,
    };
    let name = match name {
        "Next" => "Page Down",
        "Prior" => "Page Up",
        "Print" => "PrtSc",
        "Return" => "Enter",
        "space" => "Space",
        other => other,
    };
    if name.len() == 1 && name.is_ascii() {
        name.to_ascii_uppercase()
    } else {
        name.to_string()
    }
}

/// キーの名前を xkb の正式な名前にする（`page_down` → `Next` など。xkb が知らなければそのまま）。
fn canonical_keysym(name: &str) -> String {
    let sym = xkb::keysym_from_name(name, xkb::KEYSYM_CASE_INSENSITIVE);
    if sym.raw() == 0 {
        return name.to_string();
    }
    xkb::keysym_get_name(sym)
}

/// niri の `key_name`（画面表示）: Mod を先に、残りは Super・Ctrl・Shift・Alt の順。
pub fn key_name(b: &KeyBinding, mod_key: ModKey) -> String {
    let m = b.key.mods;
    let comp = has_comp_mod(m, mod_key);
    let mut name = String::new();
    if comp {
        name.push_str(match mod_key {
            ModKey::Super => "Super + ",
            ModKey::Alt => "Alt + ",
        });
    }
    if m.logo && !(comp && mod_key == ModKey::Super) {
        name.push_str("Super + ");
    }
    if m.ctrl {
        name.push_str("Ctrl + ");
    }
    if m.shift {
        name.push_str("Shift + ");
    }
    if m.alt && !(comp && mod_key == ModKey::Alt) {
        name.push_str("Alt + ");
    }
    let key = &b.key.keysym;
    let pretty = match key.to_ascii_lowercase().as_str() {
        "wheelscrolldown" => "Wheel Scroll Down".to_string(),
        "wheelscrollup" => "Wheel Scroll Up".to_string(),
        "wheelscrollleft" => "Wheel Scroll Left".to_string(),
        "wheelscrollright" => "Wheel Scroll Right".to_string(),
        _ => prettify(&canonical_keysym(key)),
    };
    name.push_str(&pretty);
    name
}

/// ホイールのバインドか（spawn の行から除く）。
fn is_wheel(b: &KeyBinding) -> bool {
    b.key.keysym.to_ascii_lowercase().starts_with("wheelscroll")
}

/// オーバーレイの行（niri の `collect_actions` と `format_bind`）。
pub fn rows(binds: &[KeyBinding], mod_key: ModKey) -> Vec<Row> {
    let key_of = |a: &Action| {
        binds
            .iter()
            .find(|b| b.action == *a)
            .map(|b| format!(" {} ", key_name(b, mod_key)))
            .unwrap_or_else(|| " (not bound) ".to_string())
    };
    let mut out: Vec<Row> = fixed_actions(binds)
        .into_iter()
        .map(|(action, title)| Row {
            key: action
                .as_ref()
                .map_or_else(|| " (not bound) ".to_string(), key_of),
            title: title.to_string(),
            program: None,
        })
        .collect();
    let mut seen: Vec<&str> = Vec::new();
    for b in binds {
        let Action::Spawn(command) = &b.action else {
            continue;
        };
        let m = b.key.mods;
        if !(m.logo || has_comp_mod(m, mod_key)) || is_wheel(b) || seen.contains(&command.as_str())
        {
            continue;
        }
        seen.push(command);
        out.push(Row {
            key: format!(" {} ", key_name(b, mod_key)),
            title: "Spawn ".to_string(),
            program: Some(
                command
                    .split_ascii_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string(),
            ),
        });
    }
    out
}

/// 論理 px を物理 px にする。
fn phys(scale: f64, v: f32) -> i32 {
    (f64::from(v) * scale).round() as i32
}

/// オーバーレイを RGBA に描く。フォントが無ければ `None`。
pub fn draw(rows: &[Row], scale: f64) -> Option<Canvas> {
    let f = fonts()?;
    let px = (f64::from(FONT_PX) * scale).round() as f32;
    let padding = phys(scale, PADDING);
    let interval = phys(scale, LINE_INTERVAL);
    let line = line_height(&f.regular, px).max(line_height(&f.mono, px));
    let title_w = text_width(&f.bold, px, TITLE);
    let title_h = line_height(&f.bold, px);
    let key_w = rows.iter().map(|r| text_width(&f.mono, px, &r.key)).max()?;
    let action_w = rows
        .iter()
        .map(|r| {
            text_width(&f.regular, px, &r.title)
                + r.program
                    .as_deref()
                    .map_or(0, |p| text_width(&f.mono, px, p))
        })
        .max()?;
    let n = i32::try_from(rows.len()).ok()?;
    let width = (key_w + padding + action_w).max(title_w) + padding * 2;
    let height = n * line + (n - 1) * interval + title_h + padding + padding * 2;
    let white: Color = [1.0, 1.0, 1.0, 1.0];
    let mut c = Canvas::new(width, height, [0.1, 0.1, 0.1, 1.0]);
    c.text(&f.bold, px, (width - title_w) / 2, padding, TITLE, white);
    let mut y = padding + title_h + padding;
    // pango の背景色 12000/65535。
    let key_bg: Color = [0.183, 0.183, 0.183, 1.0];
    for r in rows {
        let w = text_width(&f.mono, px, &r.key);
        c.fill(padding, y, w, line, key_bg);
        c.text(&f.mono, px, padding, y, &r.key, white);
        let x = padding + key_w + padding;
        let tw = c.text(&f.regular, px, x, y, &r.title, white);
        if let Some(p) = &r.program {
            let pw = text_width(&f.mono, px, p);
            c.fill(x + tw, y, pw, line, [0.0, 0.0, 0.0, 1.0]);
            c.text(&f.mono, px, x + tw, y, p, white);
        }
        y += line + interval;
    }
    // 枠の太さは偶数（niri: (BORDER / 2 × scale) を丸めて 2 倍）で、半分が内側に見える。
    let border = ((f64::from(BORDER) / 2.0 * scale).round() as i32 * 2) / 2;
    c.border(border.max(1), [0.5, 0.8, 1.0, 1.0]);
    Some(c)
}

/// オーバーレイの状態。
#[derive(Default)]
pub struct HotkeyOverlay {
    /// 開いているか。
    pub open: bool,
    /// 出力ごと（名前と拡大率）のテクスチャと大きさ（物理 px）。設定のリロードで捨てる。
    cache: BTreeMap<String, (GlesTexture, (i32, i32), f64)>,
}

impl HotkeyOverlay {
    /// 起動時の状態（niri の既定では開く）。
    pub fn at_startup() -> Self {
        Self {
            open: true,
            cache: BTreeMap::new(),
        }
    }

    /// 描いたものを捨てる（設定が変わった）。
    pub fn invalidate(&mut self) {
        self.cache.clear();
    }

    /// 出力 `output` に重ねる要素（中央、透明度 0.9）。描画先の左上は全体の論理座標の `origin`。
    pub fn element(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
        output_geo: smithay::utils::Rectangle<i32, Logical>,
        origin: Point<i32, Logical>,
        rows: impl FnOnce() -> Vec<Row>,
    ) -> Option<TextureRenderElement<GlesTexture>> {
        if !self.open {
            return None;
        }
        let scale = output.current_scale().fractional_scale();
        let name = output.name();
        let stale = self.cache.get(&name).is_none_or(|(_, _, s)| *s != scale);
        if stale {
            let canvas = draw(&rows(), scale)?;
            let texture = renderer
                .import_memory(
                    &canvas.rgba,
                    Fourcc::Abgr8888,
                    Size::from((canvas.width, canvas.height)),
                    false,
                )
                .map_err(|e| tracing::warn!("hotkey overlay: {e}"))
                .ok()?;
            self.cache.insert(
                name.clone(),
                (texture, (canvas.width, canvas.height), scale),
            );
        }
        let (texture, (w, h), _) = self.cache.get(&name)?;
        let size: Size<i32, Logical> =
            Size::<f64, smithay::utils::Physical>::from((f64::from(*w), f64::from(*h)))
                .to_logical(scale)
                .to_i32_round();
        let x = ((output_geo.size.w - size.w) / 2).max(0);
        let y = ((output_geo.size.h - size.h) / 2).max(0);
        let loc = (output_geo.loc - origin + Point::from((x, y))).to_physical_precise_round(scale);
        Some(TextureRenderElement::from_static_texture(
            Id::new(),
            renderer.context_id(),
            loc,
            texture.clone(),
            1,
            Transform::Normal,
            Some(0.9),
            None,
            Some(size),
            None,
            Kind::Unspecified,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use contrast_policy::KeySpec;

    fn bind(
        logo: bool,
        shift: bool,
        ctrl: bool,
        alt: bool,
        key: &str,
        action: Action,
    ) -> KeyBinding {
        KeyBinding {
            key: KeySpec {
                mods: Mods {
                    alt,
                    shift,
                    ctrl,
                    logo,
                },
                keysym: key.to_ascii_lowercase(),
            },
            action,
            cooldown_ms: None,
        }
    }

    #[test]
    fn key_names_follow_niri() {
        let b = bind(true, true, false, false, "Slash", Action::ShowHotkeyOverlay);
        assert_eq!(key_name(&b, ModKey::Super), "Super + Shift + /");
        let b = bind(
            true,
            false,
            true,
            false,
            "Page_Down",
            Action::MoveColumnToWorkspaceNext,
        );
        assert_eq!(key_name(&b, ModKey::Super), "Super + Ctrl + Page Down");
        let b = bind(
            true,
            false,
            false,
            true,
            "l",
            Action::Spawn("swaylock".into()),
        );
        assert_eq!(key_name(&b, ModKey::Super), "Super + Alt + L");
        let b = bind(false, false, false, false, "Print", Action::Screenshot);
        assert_eq!(key_name(&b, ModKey::Super), "PrtSc");
        let b = bind(
            false,
            false,
            false,
            true,
            "BracketLeft",
            Action::ConsumeOrExpelWindowLeft,
        );
        assert_eq!(key_name(&b, ModKey::Alt), "Alt + [");
    }

    #[test]
    fn rows_are_niris_list_with_not_bound_and_spawns() {
        let binds = vec![
            bind(
                true,
                false,
                false,
                false,
                "b",
                Action::Spawn("firefox-nightly".into()),
            ),
            bind(
                false,
                true,
                true,
                false,
                "space",
                Action::Spawn("fcitx5-remote -t".into()),
            ),
            bind(true, false, false, false, "q", Action::CloseFocused),
            bind(true, false, false, false, "Left", Action::FocusPrev),
            bind(true, false, false, false, "h", Action::FocusPrev),
            bind(
                true,
                false,
                false,
                false,
                "t",
                Action::Spawn("alacritty".into()),
            ),
            bind(
                true,
                true,
                false,
                false,
                "d",
                Action::Spawn("strat fedora /x".into()),
            ),
            bind(false, false, false, false, "Print", Action::Screenshot),
        ];
        let r = rows(&binds, ModKey::Super);
        let keys: Vec<(&str, &str)> = r
            .iter()
            .map(|r| (r.key.as_str(), r.title.as_str()))
            .collect();
        assert_eq!(
            keys.first(),
            Some(&(" (not bound) ", "Show Important Hotkeys"))
        );
        assert!(keys.contains(&(" Super + Q ", "Close Focused Window")));
        assert!(keys.contains(&(" Super + Left ", "Focus Column to the Left")));
        assert!(keys.contains(&(" (not bound) ", "Switch Focus Between Floating and Tiling")));
        assert!(keys.contains(&(" PrtSc ", "Take a Screenshot")));
        let spawns: Vec<(&str, Option<&str>)> = r
            .iter()
            .filter(|r| r.program.is_some())
            .map(|r| (r.key.as_str(), r.program.as_deref()))
            .collect();
        // fcitx5-remote（Mod も Super も無い）は出さない。順はバインドの順。
        assert_eq!(
            spawns,
            [
                (" Super + B ", Some("firefox-nightly")),
                (" Super + T ", Some("alacritty")),
                (" Super + Shift + D ", Some("strat")),
            ]
        );
        // 割り当てが無いので、スクリーンショットの行は最後の決まった行のあと。
        let without: Vec<KeyBinding> = binds
            .into_iter()
            .filter(|b| b.action != Action::Screenshot)
            .collect();
        assert!(
            !rows(&without, ModKey::Super)
                .iter()
                .any(|r| r.title == "Take a Screenshot")
        );
    }
}
