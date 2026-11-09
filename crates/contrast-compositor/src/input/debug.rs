//! 動作確認用の stdin コマンド（`CONTRAST_DEBUG_STDIN=1` のときだけ有効）。
//!
//! 画面を見られない環境でキーバインドとクリックの経路を試すためのもの。1 行 1 コマンド:
//! - `key alt+shift+q` … 修飾キーとキーシンボル名をポリシーで解決して実行する
//!   （xkb のキーコード→キーシンボル変換は通らない）
//! - `click X Y` … 論理座標 (X, Y) をクリックしたときのフォーカス移動
//! - `wheel MODS H V` … ホイールの軸イベント（`MODS` は `alt+ctrl` のように修飾キー、無ければ `-`。`H`・`V` は横・縦の `v120`。
//!   1 目盛りが 120、正は右／下）。ホイールのバインドに当たれば、実行してログに出す
//! - `state` … 管理中の窓の一覧をログに出す
//! - `pointer down|move|up X Y [mod]` … スクリーンショットの UI が開いているときの、左ボタンとポインタの移動（`mod` は Mod を押したまま）
//! - `shot` … アクティブな出力の今の絵を保存する（キーバインドを通さない。スクリーンショットの UI を開いたままでも撮れる）
//! - `unplug NAME` / `replug NAME` … 出力を外す／戻す（ホットプラグの経路を nested で試す。winit の分割した出力など）

use std::io::BufRead;

use smithay::reexports::calloop::{
    EventLoop,
    channel::{self, Event},
};

use smithay::wayland::input_method::InputMethodSeat;

use contrast_policy::Mods;

use crate::state::Contrast;

/// 環境変数が立っていれば、stdin を読むスレッドとイベントループへの経路を作る。
pub fn init(event_loop: &mut EventLoop<Contrast>) {
    if std::env::var_os("CONTRAST_DEBUG_STDIN").is_none() {
        return;
    }
    let (sender, receiver) = channel::channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    event_loop
        .handle()
        .insert_source(receiver, |event, _, state| {
            if let Event::Msg(line) = event {
                state.debug_command(line.trim());
            }
        })
        .expect("failed to insert debug stdin source");
    tracing::info!("debug stdin commands enabled");
}

/// `alt+ctrl` のような修飾キーの並びを `Mods` にする（`-` は無し）。未知の名前は警告して無視する。
fn parse_mods(text: &str) -> Mods {
    let mut mods = Mods::default();
    for m in text.split('+').filter(|m| !m.is_empty() && *m != "-") {
        match m {
            "alt" => mods.alt = true,
            "shift" => mods.shift = true,
            "ctrl" => mods.ctrl = true,
            "logo" | "super" => mods.logo = true,
            other => tracing::warn!(modifier = other, "debug: unknown modifier"),
        }
    }
    mods
}

impl Contrast {
    /// `wheel MODS H V`: ホイールの軸イベントを、修飾キー `MODS` を押しているものとして処理する。
    fn debug_wheel(&mut self, line: &str) {
        let words: Vec<&str> = line.split_whitespace().collect();
        let [_, mods, h, v] = words.as_slice() else {
            tracing::warn!(line, "debug: usage: wheel MODS H V");
            return;
        };
        let (Ok(h), Ok(v)) = (h.parse::<f64>(), v.parse::<f64>()) else {
            tracing::warn!(line, "debug: bad wheel amounts");
            return;
        };
        let consumed = self.wheel_with_mods(parse_mods(mods), Some(h), Some(v));
        tracing::info!(consumed, "debug wheel");
    }

    /// `unplug NAME`: 出力を外したことにする（core から外し、Space から外す）。
    fn debug_unplug(&mut self, name: &str) {
        let Some(output) = self
            .shell
            .outputs
            .values()
            .find(|o| o.name() == name)
            .cloned()
        else {
            tracing::warn!(name, "debug: no such output");
            return;
        };
        let Some(geo) = self.space.output_geometry(&output) else {
            return;
        };
        let Some(rect) = contrast_core::Rect::new(geo.loc.x, geo.loc.y, geo.size.w, geo.size.h)
        else {
            return;
        };
        self.output_removed(&output);
        self.space.unmap_output(&output);
        self.unplugged.push((output, rect));
        tracing::info!(name, "debug: output unplugged");
    }

    /// `replug NAME`: `unplug` で外した出力を、同じ位置に戻す。
    fn debug_replug(&mut self, name: &str) {
        let Some(at) = self.unplugged.iter().position(|(o, _)| o.name() == name) else {
            tracing::warn!(name, "debug: not unplugged");
            return;
        };
        let (output, rect) = self.unplugged.remove(at);
        self.space.map_output(&output, (rect.x(), rect.y()));
        self.output_added(&output, rect);
        tracing::info!(name, "debug: output replugged");
    }

    /// `pointer down|move|up X Y [mod]`: スクリーンショットの UI を、ポインタで操作する。
    fn debug_pointer(&mut self, line: &str) {
        let words: Vec<&str> = line.split_whitespace().collect();
        let (Some(kind), Some(Ok(x)), Some(Ok(y))) = (
            words.get(1),
            words.get(2).map(|v| v.parse::<f64>()),
            words.get(3).map(|v| v.parse::<f64>()),
        ) else {
            tracing::warn!(line, "debug: usage: pointer down|move|up X Y [mod]");
            return;
        };
        let pos = smithay::utils::Point::from((x, y));
        let mod_down = words.get(4) == Some(&"mod");
        match *kind {
            "down" => self.screenshot_ui_button(true, pos, mod_down),
            "up" => self.screenshot_ui_button(false, pos, mod_down),
            "move" => self.screenshot_ui_motion(pos),
            _ => tracing::warn!(line, "debug: unknown pointer command"),
        }
        tracing::info!(line, "debug pointer");
    }

    /// stdin から来た 1 行を実行する。
    fn debug_command(&mut self, line: &str) {
        if line.split_whitespace().next() == Some("pointer") {
            self.debug_pointer(line);
            return;
        }
        if line.split_whitespace().next() == Some("wheel") {
            self.debug_wheel(line);
            return;
        }
        let mut words = line.split_whitespace();
        match (words.next(), words.next(), words.next()) {
            (Some("key"), Some(combo), None) => {
                let mut mods = Mods::default();
                let mut parts: Vec<&str> = combo.split('+').collect();
                let key = parts.pop().unwrap_or_default();
                for m in parts {
                    match m {
                        "alt" => mods.alt = true,
                        "shift" => mods.shift = true,
                        "ctrl" => mods.ctrl = true,
                        "logo" | "super" => mods.logo = true,
                        other => tracing::warn!(modifier = other, "debug: unknown modifier"),
                    }
                }
                // スクリーンショットの UI が開いていれば、UI のキーを先に試す（実機のキーの経路と同じ順）。
                if self.screenshot_ui.is_open()
                    && self.screenshot_ui_key(key, mods.ctrl, mods.alt, mods.shift)
                {
                    tracing::info!(combo, "debug key: screenshot ui");
                    return;
                }
                match self.policy.resolve_key(mods, key) {
                    Some(action) => {
                        tracing::info!(combo, ?action, "debug keybinding");
                        self.run_action(action);
                    }
                    None => tracing::info!(combo, "debug key: no binding"),
                }
            }
            (Some("shot"), None, None) => self.screenshot_pending = true,
            (Some("unplug"), Some(name), None) => self.debug_unplug(name),
            (Some("replug"), Some(name), None) => self.debug_replug(name),
            (Some("click"), Some(x), Some(y)) => match (x.parse::<f64>(), y.parse::<f64>()) {
                (Ok(x), Ok(y)) => self.focus_at(x, y),
                _ => tracing::warn!(line, "debug: bad click"),
            },
            (Some("state"), None, None) => {
                use contrast_core::IdKind;
                let view = self.core.view();
                for (key, window) in &self.shell.windows {
                    let Some(w) = view.window_id(*key).and_then(|id| view.window(id)) else {
                        continue;
                    };
                    tracing::info!(
                        window = w.id.raw(),
                        workspace = w.workspace.get() + 1,
                        visible = w.visible,
                        state = ?w.state,
                        location = ?self.space.element_location(window),
                        "debug state"
                    );
                }
                let shown = view.active_output().and_then(|o| view.shown_workspace(o));
                tracing::info!(
                    shown_workspace = ?shown.map(|i| i.get() + 1),
                    focused = ?view.focus().map(|id| id.raw()),
                    "debug state"
                );
                tracing::info!(
                    keyboard_grabbed = self.seat.get_keyboard().is_some_and(|k| k.is_grabbed()),
                    input_method_grab = self.seat.input_method().keyboard_grabbed(),
                    ime_popups = self.ime_popups.len(),
                    "debug state ime"
                );
            }
            _ => tracing::warn!(line, "debug: unknown command"),
        }
    }
}
