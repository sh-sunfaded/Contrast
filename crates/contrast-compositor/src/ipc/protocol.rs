//! niri 互換の IPC の JSON（純粋。I/O も Smithay も知らない）。基準は niri 26.4.0 の `niri-ipc`。
//!
//! 形は `niri-ipc` と同じ（serde の外部タグ、1 行 1 メッセージ）。受けるリクエストは
//! `Version`・`Outputs`・`Workspaces`・`Windows`・`Layers`・`KeyboardLayouts`・`FocusedOutput`・`FocusedWindow`・
//! `OverviewState`・`Casts`・`EventStream`・`Action(FocusWorkspace { reference: Id(n) | Index(n) })`。
//! ほかは `{"Err":"unsupported request"}`。名前での参照（`Name`）は、名前を持たないので専用のエラーにする。
//! リクエストは、改行で終わるものも、書き込み側の shutdown で終わる（改行なしの）ものも受ける。

use serde::{Deserialize, Serialize};

/// 解釈できないリクエストへの返事。
pub(crate) const UNSUPPORTED: &str = "unsupported request";
/// 名前でのワークスペースの参照への返事。
pub(crate) const NAMES_UNSUPPORTED: &str = "workspace names are not supported";
/// 範囲外のワークスペースへの返事。
pub(crate) const NOT_FOUND: &str = "workspace not found";

/// niri の `Workspace`（ironbar が読むのは `id`・`idx`・`name`・`output`・`is_active`・`is_focused`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Workspace {
    /// 一意の番号（1 始まり）。
    pub id: u64,
    /// 出力の中の位置（1 始まり）。
    pub idx: u8,
    /// 名前（Contrast には無いので `None`）。
    pub name: Option<String>,
    /// 表示している出力の名前。
    pub output: Option<String>,
    /// 緊急（Contrast には無いので常に false）。
    pub is_urgent: bool,
    /// その出力で表示中か。
    pub is_active: bool,
    /// フォーカス中か（全体で 1 つ）。
    pub is_focused: bool,
    /// アクティブな窓の id（Contrast は出さないので `None`）。
    pub active_window_id: Option<u64>,
}

/// niri の `Timestamp`（単調時計）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct Timestamp {
    /// 秒。
    pub secs: u64,
    /// 秒の小数部（ナノ秒）。
    pub nanos: u32,
}

/// niri の `WindowLayout`。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct WindowLayout {
    /// スクロール列での（列, 列の中の位置）。どちらも 1 始まり。浮動の窓は `None`。
    pub pos_in_scrolling_layout: Option<(usize, usize)>,
    /// タイルの大きさ。
    pub tile_size: (f64, f64),
    /// 窓の大きさ。
    pub window_size: (i32, i32),
    /// ワークスペースの表示の中のタイルの位置。
    pub tile_pos_in_workspace_view: Option<(f64, f64)>,
    /// タイルの中の窓の位置。
    pub window_offset_in_tile: (f64, f64),
}

/// niri の `Window`。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Window {
    /// 一意の番号。
    pub id: u64,
    /// タイトル。
    pub title: Option<String>,
    /// app_id。
    pub app_id: Option<String>,
    /// クライアントのプロセス番号。
    pub pid: Option<i32>,
    /// ワークスペースの id。
    pub workspace_id: Option<u64>,
    /// フォーカス中か。
    pub is_focused: bool,
    /// 浮動か。
    pub is_floating: bool,
    /// 緊急か（Contrast には無いので false）。
    pub is_urgent: bool,
    /// 全画面か。
    pub is_fullscreen: bool,
    /// 最小化か（Contrast には無いので false）。
    pub is_minimized: bool,
    /// 配置。
    pub layout: WindowLayout,
    /// 最後にフォーカスされた時刻。
    pub focus_timestamp: Option<Timestamp>,
}

/// niri の `Mode`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct Mode {
    /// 幅。
    pub width: u16,
    /// 高さ。
    pub height: u16,
    /// リフレッシュレート（mHz）。
    pub refresh_rate: u32,
    /// 推奨のモードか。
    pub is_preferred: bool,
}

/// niri の `Transform`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum Transform {
    /// 回転なし。
    Normal,
    /// 90 度。
    #[serde(rename = "90")]
    Rotate90,
    /// 180 度。
    #[serde(rename = "180")]
    Rotate180,
    /// 270 度。
    #[serde(rename = "270")]
    Rotate270,
    /// 反転。
    Flipped,
    /// 反転して 90 度。
    Flipped90,
    /// 反転して 180 度。
    Flipped180,
    /// 反転して 270 度。
    Flipped270,
}

/// niri の `LogicalOutput`。
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub(crate) struct LogicalOutput {
    /// 全体の論理座標の x。
    pub x: i32,
    /// 全体の論理座標の y。
    pub y: i32,
    /// 論理の幅。
    pub width: u32,
    /// 論理の高さ。
    pub height: u32,
    /// 拡大率。
    pub scale: f64,
    /// 回転。
    pub transform: Transform,
}

/// niri の `Output`。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Output {
    /// 名前（接続の名前）。
    pub name: String,
    /// 製造元。
    pub make: String,
    /// モデル。
    pub model: String,
    /// シリアル（Contrast は読まないので `None`）。
    pub serial: Option<String>,
    /// 物理の大きさ（mm）。
    pub physical_size: Option<(u32, u32)>,
    /// モードの一覧。
    pub modes: Vec<Mode>,
    /// 今のモードの `modes` の中の位置。
    pub current_mode: Option<usize>,
    /// 利用者が指定したモードか。
    pub is_custom_mode: bool,
    /// VRR に対応しているか（Contrast は使わないので false）。
    pub vrr_supported: bool,
    /// VRR が有効か。
    pub vrr_enabled: bool,
    /// 論理の配置。
    pub logical: Option<LogicalOutput>,
    /// 最大の色深度（Contrast は設定しないので `None`）。
    pub max_bpc: Option<u8>,
}

/// niri の `Layer`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum Layer {
    /// 背景。
    Background,
    /// 下。
    Bottom,
    /// 上。
    Top,
    /// 最前面。
    Overlay,
}

/// niri の `LayerSurfaceKeyboardInteractivity`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum KeyboardInteractivity {
    /// キーボードを受けない。
    None,
    /// 排他。
    Exclusive,
    /// 要求に応じて。
    OnDemand,
}

/// niri の `LayerSurface`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct LayerSurface {
    /// 名前空間。
    pub namespace: String,
    /// 出力の名前。
    pub output: String,
    /// 層。
    pub layer: Layer,
    /// キーボードの扱い。
    pub keyboard_interactivity: KeyboardInteractivity,
}

/// niri の `KeyboardLayouts`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct KeyboardLayouts {
    /// 配列の名前。
    pub names: Vec<String>,
    /// 今の配列の位置。
    pub current_idx: u8,
}

/// niri の `Overview`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct Overview {
    /// 開いているか。
    pub is_open: bool,
}

/// niri の `Response` のうち使うもの。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) enum Response {
    /// 返す中身のない成功。
    Handled,
    /// バージョンの文字列。
    Version(String),
    /// 出力の名前から出力への対応。
    Outputs(std::collections::BTreeMap<String, Output>),
    /// ワークスペースの一覧。
    Workspaces(Vec<Workspace>),
    /// 窓の一覧。
    Windows(Vec<Window>),
    /// レイヤー面の一覧。
    Layers(Vec<LayerSurface>),
    /// キーボードの配列。
    KeyboardLayouts(KeyboardLayouts),
    /// フォーカス中の出力。
    FocusedOutput(Option<Output>),
    /// フォーカス中の窓。
    FocusedWindow(Option<Window>),
    /// オーバービューの状態。
    OverviewState(Overview),
    /// 画面の共有（Contrast は持たないので常に空）。
    Casts(Vec<serde_json::Value>),
}

/// niri の `Reply`（`{"Ok":...}` か `{"Err":"..."}`）。
pub(crate) type Reply = Result<Response, String>;

/// niri の `Event` のうち送るもの。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) enum Event {
    /// ワークスペースの構成が変わった（完全な状態）。
    WorkspacesChanged {
        /// 新しい一覧。
        workspaces: Vec<Workspace>,
    },
    /// ワークスペースが、その出力で表示中になった。`focused` なら、フォーカスも移った。
    WorkspaceActivated {
        /// ワークスペースの id。
        id: u64,
        /// フォーカスも移ったか。
        focused: bool,
    },
    /// 窓の一覧（完全な状態）。
    WindowsChanged {
        /// 新しい一覧。
        windows: Vec<Window>,
    },
    /// 窓が開いた・変わった。
    WindowOpenedOrChanged {
        /// その窓。
        window: Window,
    },
    /// 窓が閉じた。
    WindowClosed {
        /// 窓の id。
        id: u64,
    },
    /// フォーカスが移った。
    WindowFocusChanged {
        /// フォーカスした窓（無ければ `None`）。
        id: Option<u64>,
    },
    /// 窓の最後のフォーカスの時刻が変わった。
    WindowFocusTimestampChanged {
        /// 窓の id。
        id: u64,
        /// 時刻。
        focus_timestamp: Option<Timestamp>,
    },
    /// 窓の配置が変わった。
    WindowLayoutsChanged {
        /// 変わった窓の（id, 配置）。
        changes: Vec<(u64, WindowLayout)>,
    },
    /// キーボードの配列の構成が変わった。
    KeyboardLayoutsChanged {
        /// 新しい配列の情報。
        keyboard_layouts: KeyboardLayouts,
    },
    /// キーボードの配列が切り替わった。
    KeyboardLayoutSwitched {
        /// 新しい位置。
        idx: u8,
    },
    /// オーバービューが開いた・閉じた。
    OverviewOpenedOrClosed {
        /// 開いているか。
        is_open: bool,
    },
    /// 設定を読み込んだ。
    ConfigLoaded {
        /// 失敗したか。
        failed: bool,
    },
    /// スクリーンショットを撮った。
    ScreenshotCaptured {
        /// 保存したパス（保存しなかったときは `None`）。
        path: Option<String>,
    },
    /// 画面の共有の一覧（Contrast は持たないので常に空）。
    CastsChanged {
        /// 空。
        casts: Vec<serde_json::Value>,
    },
}

/// 解釈したリクエスト。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Request {
    /// イベントの購読。
    EventStream,
    /// ワークスペースの一覧。
    Workspaces,
    /// バージョン。
    Version,
    /// 出力の一覧。
    Outputs,
    /// 窓の一覧。
    Windows,
    /// レイヤー面の一覧。
    Layers,
    /// キーボードの配列。
    KeyboardLayouts,
    /// フォーカス中の出力。
    FocusedOutput,
    /// フォーカス中の窓。
    FocusedWindow,
    /// オーバービューの状態。
    OverviewState,
    /// 画面の共有。
    Casts,
    /// フォーカス中の出力の `idx`（1 始まり）のワークスペースに切り替える。
    FocusWorkspaceIndex(u8),
    /// id でワークスペースに切り替える。
    FocusWorkspaceId(u64),
    /// 名前でワークスペースに切り替える（未対応。名前はログ用）。
    FocusWorkspaceName(String),
    /// 上記以外（解析できないものを含む）。
    Unsupported,
}

/// 線上のリクエスト（niri の `Request` のうち受けるもの）。
#[derive(Deserialize)]
enum Wire {
    EventStream,
    Workspaces,
    Version,
    Outputs,
    Windows,
    Layers,
    KeyboardLayouts,
    FocusedOutput,
    FocusedWindow,
    OverviewState,
    Casts,
    Action(WireAction),
}

/// 線上のアクション（niri の `Action` のうち受けるもの）。
#[derive(Deserialize)]
enum WireAction {
    FocusWorkspace { reference: WireReference },
}

/// 線上のワークスペースの参照（niri の `WorkspaceReferenceArg`）。
#[derive(Deserialize)]
enum WireReference {
    Id(u64),
    Index(u8),
    Name(String),
}

/// リクエスト 1 行（末尾の改行は有っても無くてもよい）を解釈する。
pub(crate) fn parse_request(line: &[u8]) -> Request {
    match serde_json::from_slice::<Wire>(line) {
        Ok(Wire::EventStream) => Request::EventStream,
        Ok(Wire::Workspaces) => Request::Workspaces,
        Ok(Wire::Version) => Request::Version,
        Ok(Wire::Outputs) => Request::Outputs,
        Ok(Wire::Windows) => Request::Windows,
        Ok(Wire::Layers) => Request::Layers,
        Ok(Wire::KeyboardLayouts) => Request::KeyboardLayouts,
        Ok(Wire::FocusedOutput) => Request::FocusedOutput,
        Ok(Wire::FocusedWindow) => Request::FocusedWindow,
        Ok(Wire::OverviewState) => Request::OverviewState,
        Ok(Wire::Casts) => Request::Casts,
        Ok(Wire::Action(WireAction::FocusWorkspace { reference })) => match reference {
            WireReference::Id(id) => Request::FocusWorkspaceId(id),
            WireReference::Index(index) => Request::FocusWorkspaceIndex(index),
            WireReference::Name(name) => Request::FocusWorkspaceName(name),
        },
        Err(_) => Request::Unsupported,
    }
}

/// 値を JSON 1 行（改行つき）にする。整形に失敗したら（起きないはず）、固定のエラーを返す。
fn line<T: Serialize>(value: &T) -> Vec<u8> {
    let mut out =
        serde_json::to_vec(value).unwrap_or_else(|_| br#"{"Err":"internal error"}"#.to_vec());
    out.push(b'\n');
    out
}

/// 返事を 1 行にする。
pub(crate) fn encode_reply(reply: &Reply) -> Vec<u8> {
    line(reply)
}

/// イベントを 1 行にする。
pub(crate) fn encode_event(event: &Event) -> Vec<u8> {
    line(event)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(id: u64, active: bool, focused: bool) -> Workspace {
        Workspace {
            id,
            idx: u8::try_from(id).unwrap_or(0),
            name: None,
            output: Some("eDP-1".to_owned()),
            is_urgent: false,
            is_active: active,
            is_focused: focused,
            active_window_id: None,
        }
    }

    fn text(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }

    #[test]
    fn the_requests_ironbar_sends_are_understood_with_or_without_a_newline() {
        let stream = br#""EventStream""#;
        let focus = br#"{"Action":{"FocusWorkspace":{"reference":{"Id":3}}}}"#;
        for (line, want) in [
            (&stream[..], Request::EventStream),
            (&focus[..], Request::FocusWorkspaceId(3)),
        ] {
            assert_eq!(parse_request(line), want);
            let mut with_newline = line.to_vec();
            with_newline.push(b'\n');
            assert_eq!(parse_request(&with_newline), want);
        }
    }

    #[test]
    fn workspaces_and_names_are_understood() {
        assert_eq!(parse_request(br#""Workspaces""#), Request::Workspaces);
        assert_eq!(
            parse_request(br#"{"Action":{"FocusWorkspace":{"reference":{"Name":"web"}}}}"#),
            Request::FocusWorkspaceName("web".to_owned())
        );
    }

    #[test]
    fn the_query_requests_and_index_references_are_understood() {
        for (text, want) in [
            (r#""Version""#, Request::Version),
            (r#""Outputs""#, Request::Outputs),
            (r#""Windows""#, Request::Windows),
            (r#""Layers""#, Request::Layers),
            (r#""KeyboardLayouts""#, Request::KeyboardLayouts),
            (r#""FocusedOutput""#, Request::FocusedOutput),
            (r#""FocusedWindow""#, Request::FocusedWindow),
            (r#""OverviewState""#, Request::OverviewState),
            (r#""Casts""#, Request::Casts),
            (
                r#"{"Action":{"FocusWorkspace":{"reference":{"Index":2}}}}"#,
                Request::FocusWorkspaceIndex(2),
            ),
        ] {
            assert_eq!(parse_request(text.as_bytes()), want, "{text}");
        }
    }

    #[test]
    fn everything_else_is_unsupported() {
        for line in [
            &br#""PickWindow""#[..],
            br#""PickColor""#,
            br#"{"Action":{"Quit":{"skip_confirmation":true}}}"#,
            br#"{"Action":{"FocusWorkspace":{"reference":{"Index":300}}}}"#,
            br#"{"Action":{"FocusWorkspace":{"reference":{"Id":-1}}}}"#,
            b"not json",
            b"",
            b"\n",
        ] {
            assert_eq!(parse_request(line), Request::Unsupported, "{}", text(line));
        }
    }

    #[test]
    fn replies_have_the_shape_ironbar_reads() {
        assert_eq!(
            text(&encode_reply(&Ok(Response::Handled))),
            "{\"Ok\":\"Handled\"}\n"
        );
        assert_eq!(
            text(&encode_reply(&Err(UNSUPPORTED.to_owned()))),
            "{\"Err\":\"unsupported request\"}\n"
        );
        assert_eq!(
            text(&encode_reply(&Ok(Response::Workspaces(vec![ws(
                1, true, true
            )])))),
            "{\"Ok\":{\"Workspaces\":[{\"id\":1,\"idx\":1,\"name\":null,\"output\":\"eDP-1\",\
             \"is_urgent\":false,\"is_active\":true,\"is_focused\":true,\"active_window_id\":null}]}}\n"
        );
    }

    #[test]
    fn events_have_the_shape_ironbar_reads() {
        assert_eq!(
            text(&encode_event(&Event::WorkspaceActivated {
                id: 2,
                focused: true
            })),
            "{\"WorkspaceActivated\":{\"id\":2,\"focused\":true}}\n"
        );
        let changed = encode_event(&Event::WorkspacesChanged {
            workspaces: vec![ws(1, true, true), ws(2, false, false)],
        });
        assert!(text(&changed).starts_with("{\"WorkspacesChanged\":{\"workspaces\":[{\"id\":1,"));
        assert!(changed.ends_with(b"\n"));
        assert_eq!(changed.iter().filter(|b| **b == b'\n').count(), 1);
    }

    /// ironbar 0.19.1 の `connection.rs` の型と同じ定義。ここで読めれば、ironbar も読める。
    mod ironbar {
        use serde::Deserialize;

        #[derive(Deserialize, Debug, PartialEq, Eq)]
        pub struct Workspace {
            pub id: u64,
            pub idx: u8,
            pub name: Option<String>,
            pub output: Option<String>,
            pub is_active: bool,
            pub is_focused: bool,
        }

        #[derive(Deserialize, Debug)]
        pub enum Response {
            Handled,
            Workspaces(Vec<Workspace>),
        }

        pub type Reply = Result<Response, String>;

        #[derive(Deserialize, Debug)]
        pub enum Event {
            WorkspacesChanged { workspaces: Vec<Workspace> },
            WorkspaceActivated { id: u64, focused: bool },
            WorkspaceUrgencyChanged { id: u64, urgent: bool },
            Other,
        }
    }

    #[test]
    fn ironbars_own_types_read_what_we_send() -> Result<(), String> {
        let reply: ironbar::Reply = serde_json::from_slice(&encode_reply(&Ok(Response::Handled)))
            .map_err(|e| e.to_string())?;
        assert!(matches!(reply, Ok(ironbar::Response::Handled)));
        let reply: ironbar::Reply = serde_json::from_slice(&encode_reply(&Err("x".to_owned())))
            .map_err(|e| e.to_string())?;
        assert_eq!(reply.err().as_deref(), Some("x"));

        let changed = encode_event(&Event::WorkspacesChanged {
            workspaces: vec![ws(1, true, true), ws(2, false, false)],
        });
        let event: ironbar::Event = serde_json::from_slice(&changed).map_err(|e| e.to_string())?;
        let ironbar::Event::WorkspacesChanged { workspaces } = event else {
            return Err(format!("not WorkspacesChanged: {event:?}"));
        };
        assert_eq!(workspaces.len(), 2);
        assert_eq!(
            workspaces.first(),
            Some(&ironbar::Workspace {
                id: 1,
                idx: 1,
                name: None,
                output: Some("eDP-1".to_owned()),
                is_active: true,
                is_focused: true,
            })
        );

        let activated = encode_event(&Event::WorkspaceActivated {
            id: 2,
            focused: false,
        });
        let event: ironbar::Event =
            serde_json::from_slice(&activated).map_err(|e| e.to_string())?;
        assert!(matches!(
            event,
            ironbar::Event::WorkspaceActivated {
                id: 2,
                focused: false
            }
        ));

        // `Workspaces` の返事（`niri msg workspaces` 用）も、ironbar の型で読める。
        let reply: ironbar::Reply = serde_json::from_slice(&encode_reply(&Ok(
            Response::Workspaces(vec![ws(1, true, true)]),
        )))
        .map_err(|e| e.to_string())?;
        match reply {
            Ok(ironbar::Response::Workspaces(list)) => assert_eq!(list.len(), 1),
            other => return Err(format!("not Workspaces: {other:?}")),
        }
        // ironbar が読める `WorkspaceUrgencyChanged`（Contrast は送らない）と、読めない行は `Other` になる側の型。
        let urgent: ironbar::Event =
            serde_json::from_str(r#"{"WorkspaceUrgencyChanged":{"id":2,"urgent":true}}"#)
                .map_err(|e| e.to_string())?;
        assert!(matches!(
            urgent,
            ironbar::Event::WorkspaceUrgencyChanged {
                id: 2,
                urgent: true
            }
        ));
        assert!(matches!(ironbar::Event::Other, ironbar::Event::Other));
        Ok(())
    }
}
