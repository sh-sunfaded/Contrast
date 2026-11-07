//! niri 互換の IPC のうち、ironbar 0.19.1 の workspaces が使う部分の JSON（純粋。I/O も Smithay も知らない）。
//!
//! 形は niri の `niri-ipc` と同じ（serde の外部タグ、1 行 1 メッセージ）。受けるリクエストは
//! `EventStream`、`Action(FocusWorkspace { reference: Id(n) })`、`Workspaces` だけで、ほかは
//! `{"Err":"unsupported request"}`。名前での参照（`Name`）は、名前を持たないので専用のエラーにする。
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

/// niri の `Response` のうち使うもの。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) enum Response {
    /// 返す中身のない成功。
    Handled,
    /// ワークスペースの一覧。
    Workspaces(Vec<Workspace>),
}

/// niri の `Reply`（`{"Ok":...}` か `{"Err":"..."}`）。
pub(crate) type Reply = Result<Response, String>;

/// niri の `Event` のうち送るもの。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
}

/// 解釈したリクエスト。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Request {
    /// イベントの購読。
    EventStream,
    /// ワークスペースの一覧。
    Workspaces,
    /// id でワークスペースに切り替える。
    FocusWorkspaceId(u64),
    /// 名前でワークスペースに切り替える（未対応）。
    FocusWorkspaceName,
    /// 上記以外（解析できないものを含む）。
    Unsupported,
}

/// 線上のリクエスト（niri の `Request` のうち受けるもの）。
#[derive(Deserialize)]
enum Wire {
    EventStream,
    Workspaces,
    Action(WireAction),
}

/// 線上のアクション（niri の `Action` のうち受けるもの）。
#[derive(Deserialize)]
enum WireAction {
    FocusWorkspace { reference: WireReference },
}

/// 線上のワークスペースの参照（niri の `WorkspaceReferenceArg`）。`Index` は受けない。
#[derive(Deserialize)]
enum WireReference {
    Id(u64),
    Name(String),
}

/// リクエスト 1 行（末尾の改行は有っても無くてもよい）を解釈する。
pub(crate) fn parse_request(line: &[u8]) -> Request {
    match serde_json::from_slice::<Wire>(line) {
        Ok(Wire::EventStream) => Request::EventStream,
        Ok(Wire::Workspaces) => Request::Workspaces,
        Ok(Wire::Action(WireAction::FocusWorkspace { reference })) => match reference {
            WireReference::Id(id) => Request::FocusWorkspaceId(id),
            WireReference::Name(_) => Request::FocusWorkspaceName,
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
            Request::FocusWorkspaceName
        );
    }

    #[test]
    fn everything_else_is_unsupported() {
        for line in [
            &br#""Version""#[..],
            br#""Outputs""#,
            br#"{"Action":{"Quit":{"skip_confirmation":true}}}"#,
            br#"{"Action":{"FocusWorkspace":{"reference":{"Index":2}}}}"#,
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
        Ok(())
    }
}
