//! 観測の記録（L3。`docs/ui-spec.md` §4.3・§6）。フォーカス・ワークスペース・窓の出現消滅・設定の読み込み・キー層の遷移を、
//! 時刻つきの JSONL（1 行 1 JSON）で残す。実験（H1〜H4）の事後分析用で、**動作に影響しない**（INV-L1）。
//!
//! - 既定は無効。環境変数 `CONTRAST_RECORD` で有効（空・`0`・`false`・`off`・`no` は無効、`/` で始まる値はそのパス、
//!   それ以外は既定の場所 `$XDG_STATE_HOME/contrast/events.jsonl`、無ければ `~/.local/state/contrast/events.jsonl`）。
//! - 差分は IPC と同じ `diff`・`diff_extra` から作る（新しい差分の計算を足さない）。**`title` は記録しない。**
//! - 書き込みの失敗は握りつぶす（最初の 1 回だけ警告し、以後は書かない）。状態へは戻さない。
//! - 時刻は引数（`Stamp`）で受ける。この型は時計を読まない。

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::ipc::protocol::{Event, Window, Workspace};
use crate::ipc::state::{Extra, diff, diff_extra};

/// 記録の時刻: 起動からの単調な経過ミリ秒と、壁時計（UNIX 時刻のミリ秒）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stamp {
    /// 起動からの経過（ミリ秒。単調）。
    pub mono_ms: u64,
    /// 壁時計（UNIX 時刻のミリ秒）。
    pub wall_ms: u64,
}

/// 記録の置き場所。`value` は `CONTRAST_RECORD` の値。無効なら `None`。
pub(crate) fn record_path(
    value: Option<&str>,
    xdg_state_home: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    let value = value?.trim();
    if matches!(value, "" | "0" | "false" | "off" | "no") {
        return None;
    }
    if value.starts_with('/') {
        return Some(PathBuf::from(value));
    }
    let base = xdg_state_home
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            home.filter(|h| !h.is_empty())
                .map(|h| PathBuf::from(h).join(".local/state"))
        })?;
    Some(base.join("contrast/events.jsonl"))
}

/// 1 行にする（時刻つき）。`fields` はオブジェクト。
fn line(stamp: Stamp, event: &str, fields: Value) -> String {
    let mut object = serde_json::Map::new();
    object.insert("t_mono_ms".to_owned(), json!(stamp.mono_ms));
    object.insert("t_wall_ms".to_owned(), json!(stamp.wall_ms));
    object.insert("event".to_owned(), json!(event));
    if let Value::Object(map) = fields {
        object.extend(map);
    }
    Value::Object(object).to_string()
}

/// IPC のイベントから記録の行を作る。`windows` は新しい窓の一覧（`app_id` を引く）。`known` は出現済みの窓の id
/// （`WindowOpenedOrChanged` は初めての id だけを「出現」とし、タイトルの変化などは記録しない）。
pub(crate) fn event_lines(
    events: &[Event],
    windows: &[Window],
    known: &mut BTreeSet<u64>,
    stamp: Stamp,
) -> Vec<String> {
    let app_of = |id: u64| -> Option<String> {
        windows
            .iter()
            .find(|w| w.id == id)
            .and_then(|w| w.app_id.clone())
    };
    let mut lines = Vec::new();
    for event in events {
        match event {
            Event::WorkspaceActivated { id, focused } => lines.push(line(
                stamp,
                "workspace_activated",
                json!({ "workspace": id, "focused": focused }),
            )),
            Event::WorkspacesChanged { workspaces } => lines.push(line(
                stamp,
                "workspaces_changed",
                json!({ "count": workspaces.len() }),
            )),
            Event::WindowFocusChanged { id } => lines.push(line(
                stamp,
                "focus",
                json!({ "window": id, "app_id": id.and_then(app_of) }),
            )),
            Event::WindowOpenedOrChanged { window } => {
                if known.insert(window.id) {
                    lines.push(line(
                        stamp,
                        "window_opened",
                        json!({
                            "window": window.id,
                            "app_id": window.app_id,
                            "workspace": window.workspace_id,
                            "floating": window.is_floating,
                        }),
                    ));
                }
            }
            Event::WindowClosed { id } => {
                known.remove(id);
                lines.push(line(stamp, "window_closed", json!({ "window": id })));
            }
            _ => {}
        }
    }
    lines
}

/// 設定を読み込んだ行（`profile` つき）。
pub(crate) fn config_line(profile: &str, failed: bool, stamp: Stamp) -> String {
    line(
        stamp,
        "config_loaded",
        json!({ "profile": profile, "failed": failed }),
    )
}

/// キー層が変わった行。
pub(crate) fn mode_line(mode: &str, stamp: Stamp) -> String {
    line(stamp, "key_mode", json!({ "mode": mode }))
}

/// 記録の書き手。状態（`Contrast`・core）を触る口を持たない（INV-L1）。
pub(crate) struct Recorder {
    /// 出力先。
    out: Box<dyn Write>,
    /// 前回のワークスペースの一覧（IPC と同じ差分の元）。
    last: Vec<Workspace>,
    /// 前回の窓などの状態。
    last_extra: Extra,
    /// 出現済みの窓の id。
    known: BTreeSet<u64>,
    /// 書き込みに失敗した（以後は書かない）。
    broken: bool,
}

impl Recorder {
    /// `out` に書く記録を作る。
    pub(crate) fn new(out: Box<dyn Write>) -> Self {
        Self {
            out,
            last: Vec::new(),
            last_extra: Extra::default(),
            known: BTreeSet::new(),
            broken: false,
        }
    }

    /// 新しい状態を渡す。前回との差分（IPC と同じ `diff`・`diff_extra`）を記録する。
    pub(crate) fn record_state(&mut self, new: &[Workspace], new_extra: &Extra, stamp: Stamp) {
        let mut events = diff(&self.last, new);
        events.extend(diff_extra(&self.last_extra, new_extra));
        let lines = event_lines(&events, &new_extra.windows, &mut self.known, stamp);
        self.last = new.to_vec();
        self.last_extra = new_extra.clone();
        self.write(&lines);
    }

    /// 設定を読み込んだことを記録する。
    pub(crate) fn record_config(&mut self, profile: &str, failed: bool, stamp: Stamp) {
        self.write(&[config_line(profile, failed, stamp)]);
    }

    /// キー層が変わったことを記録する。
    pub(crate) fn record_mode(&mut self, mode: &str, stamp: Stamp) {
        self.write(&[mode_line(mode, stamp)]);
    }

    /// 行を書く。失敗は握りつぶす（最初の 1 回だけ警告）。
    fn write(&mut self, lines: &[String]) {
        if self.broken {
            return;
        }
        for l in lines {
            if let Err(e) = writeln!(self.out, "{l}") {
                self.fail(&e);
                return;
            }
        }
        if let Err(e) = self.out.flush() {
            self.fail(&e);
        }
    }

    fn fail(&mut self, e: &std::io::Error) {
        self.broken = true;
        tracing::warn!("record: write failed, recording stops: {e}");
    }
}

/// 環境変数から記録を始める（無効なら `None`。ファイルを開けなければ警告して `None`）。
pub(crate) fn from_env() -> Option<Recorder> {
    let value = std::env::var("CONTRAST_RECORD").ok();
    let path = record_path(
        value.as_deref(),
        std::env::var_os("XDG_STATE_HOME"),
        std::env::var_os("HOME"),
    )?;
    let opened = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
        });
    match opened {
        Ok(file) => {
            tracing::info!("record: writing events to {}", path.display());
            Some(Recorder::new(Box::new(file)))
        }
        Err(e) => {
            tracing::warn!("record: cannot open {}: {e}", path.display());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;
    use crate::ipc::protocol::WindowLayout;

    const T: Stamp = Stamp {
        mono_ms: 1500,
        wall_ms: 1_700_000_000_000,
    };

    fn window(id: u64, title: &str, app: &str, focused: bool) -> Window {
        Window {
            id,
            title: Some(title.to_owned()),
            app_id: Some(app.to_owned()),
            pid: None,
            workspace_id: Some(1),
            is_focused: focused,
            is_floating: false,
            is_urgent: false,
            is_fullscreen: false,
            is_minimized: false,
            layout: WindowLayout {
                pos_in_scrolling_layout: None,
                tile_size: (0.0, 0.0),
                window_size: (0, 0),
                tile_pos_in_workspace_view: None,
                window_offset_in_tile: (0.0, 0.0),
            },
            focus_timestamp: None,
        }
    }

    fn extra(windows: Vec<Window>, focus: Option<u64>) -> Extra {
        Extra {
            windows,
            focus,
            ..Extra::default()
        }
    }

    /// 書き込み先を共有する（テストで中身を読む）。
    #[derive(Clone, Default)]
    struct Sink(Rc<RefCell<Vec<u8>>>);

    impl Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// 必ず失敗する書き込み先。
    struct Broken;

    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disk full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("disk full"))
        }
    }

    fn text(sink: &Sink) -> String {
        String::from_utf8_lossy(&sink.0.borrow()).into_owned()
    }

    #[test]
    fn recording_is_off_unless_asked_and_has_the_default_and_explicit_paths() {
        let os = |s: &str| Some(OsString::from(s));
        for off in [
            None,
            Some(""),
            Some("0"),
            Some("false"),
            Some("off"),
            Some(" no "),
        ] {
            assert_eq!(record_path(off, os("/s"), os("/h")), None, "{off:?}");
        }
        assert_eq!(
            record_path(Some("1"), os("/s"), os("/h")),
            Some(PathBuf::from("/s/contrast/events.jsonl"))
        );
        assert_eq!(
            record_path(Some("1"), None, os("/h")),
            Some(PathBuf::from("/h/.local/state/contrast/events.jsonl"))
        );
        assert_eq!(
            record_path(Some("/tmp/x.jsonl"), None, None),
            Some(PathBuf::from("/tmp/x.jsonl"))
        );
        assert_eq!(record_path(Some("1"), None, None), None);
    }

    #[test]
    fn a_first_window_and_its_focus_are_recorded_without_the_title() {
        let sink = Sink::default();
        let mut r = Recorder::new(Box::new(sink.clone()));
        let w = window(7, "SECRET TITLE", "foot", true);
        r.record_state(&[], &extra(vec![w.clone()], Some(7)), T);
        let out = text(&sink);
        assert!(out.contains(r#""event":"window_opened""#), "{out}");
        assert!(out.contains(r#""event":"focus""#), "{out}");
        assert!(out.contains(r#""app_id":"foot""#), "{out}");
        assert!(
            out.contains(r#""t_mono_ms":1500"#) && out.contains(r#""t_wall_ms":1700000000000"#)
        );
        // タイトルは記録しない。
        assert!(!out.contains("SECRET"), "{out}");
        assert!(!out.contains("title"), "{out}");
        // 各行が JSON として読める。
        for l in out.lines() {
            assert!(serde_json::from_str::<Value>(l).is_ok(), "{l}");
        }
    }

    #[test]
    fn a_title_change_is_not_an_event_but_a_close_and_a_focus_move_are() {
        let sink = Sink::default();
        let mut r = Recorder::new(Box::new(sink.clone()));
        let a = window(1, "a", "foot", true);
        let b = window(2, "b", "firefox", false);
        r.record_state(&[], &extra(vec![a.clone(), b.clone()], Some(1)), T);
        let before = text(&sink).lines().count();
        // タイトルだけ変わる: 記録しない。
        let a2 = window(1, "a changed", "foot", true);
        r.record_state(&[], &extra(vec![a2.clone(), b.clone()], Some(1)), T);
        assert_eq!(text(&sink).lines().count(), before);
        // フォーカスが移る。
        r.record_state(
            &[],
            &extra(vec![a2.clone(), window(2, "b", "firefox", true)], Some(2)),
            T,
        );
        // 窓 1 が閉じる。
        r.record_state(
            &[],
            &extra(vec![window(2, "b", "firefox", true)], Some(2)),
            T,
        );
        let out = text(&sink);
        assert!(
            out.contains(r#""event":"window_closed","window":1"#) || out.contains(r#""window":1"#)
        );
        assert!(out.matches(r#""event":"focus""#).count() >= 2, "{out}");
        assert!(out.contains(r#""event":"window_closed""#), "{out}");
    }

    #[test]
    fn config_and_key_mode_lines_carry_the_profile_and_the_mode() {
        let c = config_line("dim", false, T);
        assert!(c.contains(r#""event":"config_loaded""#) && c.contains(r#""profile":"dim""#));
        assert!(c.contains(r#""failed":false"#));
        let m = mode_line("resize", T);
        assert!(m.contains(r#""event":"key_mode""#) && m.contains(r#""mode":"resize""#));
    }

    #[test]
    fn a_broken_writer_never_panics_and_stops_after_the_first_failure() {
        let mut r = Recorder::new(Box::new(Broken));
        r.record_config("default", false, T);
        r.record_mode("m", T);
        r.record_state(&[], &extra(vec![window(1, "a", "x", true)], Some(1)), T);
        assert!(r.broken);
        // 失敗しても状態の追跡は続く（再開はしないが、呼び出し側には何も返さない）。
    }

    #[test]
    fn recording_does_not_change_what_the_core_does() {
        // 記録の有無で、同じ命令列に対する core の `Effect` 列が同じ（記録は core にも `Contrast` にも触れない）。
        use contrast_core::{
            Columns, Command, Core, CoreConfig, DefaultPolicy, ExternalKey, OutputName, Rect,
            WindowMeta,
        };
        fn run(record: bool) -> Vec<String> {
            let mut core = Core::new(CoreConfig::new(4), DefaultPolicy::new(), Columns::new());
            let sink = Sink::default();
            let mut recorder = Recorder::new(Box::new(sink.clone()));
            let mut effects = Vec::new();
            let commands = vec![
                Command::OutputAdded {
                    key: ExternalKey::new(1),
                    name: OutputName::new("t"),
                    geometry: Rect::new(0, 0, 1000, 600).unwrap(),
                },
                Command::WindowMapped {
                    key: ExternalKey::new(10),
                    meta: WindowMeta::default(),
                },
                Command::WindowMapped {
                    key: ExternalKey::new(11),
                    meta: WindowMeta::default(),
                },
                Command::WindowUnmapped(ExternalKey::new(10)),
            ];
            for (i, command) in commands.into_iter().enumerate() {
                let applied = core.apply(command).unwrap();
                effects.push(format!("{:?}", applied.effects));
                if record {
                    recorder.record_state(
                        &[],
                        &extra(
                            vec![window(u64::try_from(i).unwrap(), "t", "a", true)],
                            Some(1),
                        ),
                        T,
                    );
                }
            }
            effects
        }
        assert_eq!(run(true), run(false));
    }
}
