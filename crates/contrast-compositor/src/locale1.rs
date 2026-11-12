//! systemd-localed（`org.freedesktop.locale1`）の X11 の配列の変更への追従（niri の `dbus/freedesktop_locale1.rs`）。
//!
//! zbus は依存に無いので、`dbus-monitor --system` の出力を別スレッドで読み、`PropertiesChanged` が来るたびに
//! `busctl get-property` で配列を読み直して、イベントループへ送る。環境変数 `CONTRAST_XKB_*` で配列を
//! 指定しているときは、niri が設定の xkb を優先するのと同じく、追従しない。

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::Duration;

use calloop::{
    LoopHandle,
    channel::{self, Event},
};

use crate::state::{Contrast, XkbNames, xkb_from_locale1};

/// `dbus-monitor` の 1 行が、localed のプロパティの変更の通知か。
pub(crate) fn is_properties_changed(line: &str) -> bool {
    line.contains("member=PropertiesChanged")
}

impl Contrast {
    /// localed の変更を監視し、配列が変わったらキーボードの xkb を差し替える。`CONTRAST_XKB_*` があれば何もしない。
    pub fn watch_locale1(&self, handle: &LoopHandle<'static, Contrast>) {
        if std::env::vars().any(|(k, v)| k.starts_with("CONTRAST_XKB_") && !v.is_empty()) {
            return;
        }
        let (sender, receiver) = channel::channel::<XkbNames>();
        let inserted = handle.insert_source(receiver, |event, _, state| {
            if let Event::Msg(names) = event {
                state.apply_xkb(&names);
            }
        });
        if let Err(err) = inserted {
            tracing::warn!(%err, "xkb: cannot watch systemd-localed");
            return;
        }
        let spawned = std::thread::Builder::new()
            .name("locale1-watch".into())
            .spawn(move || {
                // `busctl monitor` はシステムバスで Access denied になるので、権限の要らない `dbus-monitor`（通常の match）を使う。
                let child = Command::new("dbus-monitor")
                    .args([
                        "--system",
                        "type='signal',sender='org.freedesktop.locale1',member='PropertiesChanged'",
                    ])
                    .stdin(Stdio::null())
                    .stderr(Stdio::null())
                    .stdout(Stdio::piped())
                    .spawn();
                let Ok(mut child) = child else {
                    tracing::warn!("xkb: cannot start dbus-monitor; not following systemd-localed");
                    return;
                };
                let Some(out) = child.stdout.take() else {
                    return;
                };
                let mut last = xkb_from_locale1();
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    if !is_properties_changed(&line) {
                        continue;
                    }
                    // 通知のあとに属性が揃うのを少し待つ。
                    std::thread::sleep(Duration::from_millis(100));
                    let now = xkb_from_locale1();
                    if now.is_some() && now != last {
                        last = now.clone();
                        if let Some(names) = now
                            && sender.send(names).is_err()
                        {
                            return;
                        }
                    }
                }
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "xkb: cannot start the localed watcher thread");
        }
    }

    /// キーボードの xkb を `names` に差し替える。作れなければ、警告して今のままにする。
    pub(crate) fn apply_xkb(&mut self, names: &XkbNames) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let config = smithay::input::keyboard::XkbConfig {
            layout: &names.layout,
            variant: &names.variant,
            model: &names.model,
            options: names.options.clone(),
            ..Default::default()
        };
        match keyboard.set_xkb_config(self, config) {
            Ok(()) => tracing::info!(?names, "xkb: systemd-localed changed, keymap updated"),
            Err(err) => tracing::warn!(
                ?err,
                ?names,
                "xkb: invalid keymap settings, keeping the current one"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_properties_changed;

    #[test]
    fn dbus_monitor_lines_are_recognised() {
        assert!(is_properties_changed(
            "signal time=1.2 sender=:1.4 -> destination=(null destination) serial=3 path=/org/freedesktop/locale1; interface=org.freedesktop.DBus.Properties; member=PropertiesChanged"
        ));
        assert!(!is_properties_changed("   string \"X11Layout\""));
        assert!(!is_properties_changed(
            "signal ...; member=NameOwnerChanged"
        ));
    }
}
