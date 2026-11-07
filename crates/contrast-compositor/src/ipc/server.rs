//! niri 互換の IPC のサーバー（calloop の Unix ソケット）。
//!
//! - ソケットは `$XDG_RUNTIME_DIR/contrast.<pid>.sock`、0600。既にあれば作らずにエラー（上書きしない）。
//!   `Drop`（パニックの unwind を含む）で unlink する。
//! - リクエストは改行で終わるものも、書き込み側の shutdown（EOF）で終わるものも受ける。
//! - `EventStream` の接続は、サーバーからは閉じない（ironbar は EOF を読むと空転するため）。
//!   リクエストの読み込みは止め、初期状態（`WorkspacesChanged`）のあと、状態の変化を 1 行ずつ送る。
//!   書き込みの待ち行列が `MAX_QUEUED` 件を超えた遅い接続は切る（niri と同じ）。
//! - それ以外の接続は、リクエストごとに返事を書き、相手が閉じたら閉じる。

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use smithay::reexports::calloop::{Interest, LoopHandle, Mode, PostAction, generic::Generic};

use super::protocol::{
    self, Event, NAMES_UNSUPPORTED, NOT_FOUND, Reply, Request, Response, UNSUPPORTED, Workspace,
};
use super::state::diff;

/// 書き込み待ちのメッセージの上限。超えた接続は切る。
const MAX_QUEUED: usize = 64;
/// 1 つのリクエストの最大の大きさ（改行までに溜める上限）。
const MAX_REQUEST_BYTES: usize = 64 * 1024;

/// サーバーが状態を読み、切り替えを頼む相手（`Contrast`。テストでは偽物）。
pub(crate) trait Host {
    /// いまのワークスペースの一覧。
    fn ipc_workspaces(&self) -> Vec<Workspace>;
    /// 0 始まりの位置のワークスペースに切り替える。
    fn ipc_focus(&mut self, index: u8);
}

/// 接続 1 つの書き込み側。
struct Client {
    /// 書き込み用（読み込み用の source が持つ fd の複製）。
    writer: UnixStream,
    /// 書き込み待ちのメッセージ。
    queue: VecDeque<Vec<u8>>,
    /// 先頭のメッセージのうち、書き終えたバイト数。
    offset: usize,
    /// `EventStream` を要求したか。
    subscribed: bool,
}

impl Client {
    /// 書けるだけ書く。詰まったら（`WouldBlock`）残りは待ち行列に残す。
    fn flush(&mut self) -> io::Result<()> {
        loop {
            let written = {
                let Some(front) = self.queue.front() else {
                    return Ok(());
                };
                let rest = front.get(self.offset..).unwrap_or_default();
                match (&self.writer).write(rest) {
                    Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                    Ok(n) => Some((n, front.len())),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => None,
                    Err(e) => return Err(e),
                }
            };
            if let Some((n, len)) = written {
                self.offset += n;
                if self.offset >= len {
                    self.queue.pop_front();
                    self.offset = 0;
                }
            }
        }
    }
}

/// 接続の表と、最後に送った一覧。
struct Shared {
    clients: BTreeMap<u64, Client>,
    next_id: u64,
    /// 購読者に最後に伝えた一覧（差分の元）。
    last: Vec<Workspace>,
}

impl Shared {
    /// 接続 `id` にメッセージを積んで書く。書けない・遅すぎる接続は切る。
    fn send(&mut self, id: u64, message: Vec<u8>) {
        let Some(client) = self.clients.get_mut(&id) else {
            return;
        };
        client.queue.push_back(message);
        let failed = client.flush().is_err();
        if failed || client.queue.len() > MAX_QUEUED {
            tracing::warn!(
                client = id,
                "ipc: disconnecting a client that cannot keep up"
            );
            self.clients.remove(&id);
        }
    }

    /// 購読者すべてにイベントを送る。
    fn broadcast(&mut self, message: &[u8]) {
        let ids: Vec<u64> = self
            .clients
            .iter()
            .filter(|(_, c)| c.subscribed)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.send(id, message.to_vec());
        }
    }
}

/// IPC のサーバー。`Drop` でソケットのファイルを消す。
pub(crate) struct IpcServer {
    path: PathBuf,
    shared: Rc<RefCell<Shared>>,
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl IpcServer {
    /// `dir`（`$XDG_RUNTIME_DIR`）にソケットを作って、イベントループに登録する。
    /// `initial` は、最初の差分の元になる一覧。同じパスが既にあればエラー（上書きしない）。
    pub(crate) fn start<D: Host + 'static>(
        handle: &LoopHandle<'static, D>,
        dir: &Path,
        initial: Vec<Workspace>,
    ) -> io::Result<Self> {
        let path = dir.join(format!("contrast.{}.sock", std::process::id()));
        let listener = UnixListener::bind(&path)?;
        // 以降の失敗では、作ったファイルを残さないよう、`Drop` に任せる。
        let server = Self {
            path,
            shared: Rc::new(RefCell::new(Shared {
                clients: BTreeMap::new(),
                next_id: 1,
                last: initial,
            })),
        };
        std::fs::set_permissions(&server.path, std::fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;

        let shared = server.shared.clone();
        let accept_handle = handle.clone();
        handle
            .insert_source(
                Generic::new(listener, Interest::READ, Mode::Level),
                move |_, listener, _host| {
                    loop {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                if let Err(e) = accept_client(&accept_handle, &shared, stream) {
                                    tracing::warn!("ipc: cannot accept a client: {e}");
                                }
                            }
                            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                            Err(e) => return Err(e),
                        }
                    }
                    Ok(PostAction::Continue)
                },
            )
            .map_err(|e| io::Error::other(e.to_string()))?;
        Ok(server)
    }

    /// ソケットのパス。
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// 状態が変わったかもしれない: 前回との差分を、購読者に送る。
    pub(crate) fn publish(&self, new: Vec<Workspace>) {
        let mut shared = self.shared.borrow_mut();
        let events = diff(&shared.last, &new);
        shared.last = new;
        for event in events {
            shared.broadcast(&protocol::encode_event(&event));
        }
    }

    /// つながっている接続の数（テスト用）。
    #[cfg(test)]
    fn client_count(&self) -> usize {
        self.shared.borrow().clients.len()
    }
}

/// 新しい接続を表に入れ、読み込みの source を登録する。
fn accept_client<D: Host + 'static>(
    handle: &LoopHandle<'static, D>,
    shared: &Rc<RefCell<Shared>>,
    stream: UnixStream,
) -> io::Result<()> {
    stream.set_nonblocking(true)?;
    let writer = stream.try_clone()?;
    let id = {
        let mut s = shared.borrow_mut();
        let id = s.next_id;
        s.next_id += 1;
        s.clients.insert(
            id,
            Client {
                writer,
                queue: VecDeque::new(),
                offset: 0,
                subscribed: false,
            },
        );
        id
    };
    let shared = shared.clone();
    let mut buffer: Vec<u8> = Vec::new();
    handle
        .insert_source(
            Generic::new(stream, Interest::READ, Mode::Level),
            move |_, stream, host| on_readable(id, &shared, &mut buffer, stream, host),
        )
        .map_err(|e| io::Error::other(e.to_string()))?;
    Ok(())
}

/// リクエスト 1 行を処理した結果。
#[derive(PartialEq, Eq)]
enum Outcome {
    /// 次の行を読む。
    Continue,
    /// `EventStream` になった。読み込みはやめる。
    Subscribed,
}

/// 接続から読めるだけ読み、完成した行（と EOF で終わる最後の行）を処理する。
fn on_readable<D: Host>(
    id: u64,
    shared: &Rc<RefCell<Shared>>,
    buffer: &mut Vec<u8>,
    stream: &UnixStream,
    host: &mut D,
) -> io::Result<PostAction> {
    if !shared.borrow().clients.contains_key(&id) {
        return Ok(PostAction::Remove);
    }
    let mut eof = false;
    let mut chunk = [0u8; 4096];
    loop {
        match (&*stream).read(&mut chunk) {
            Ok(0) => {
                eof = true;
                break;
            }
            Ok(n) => buffer.extend_from_slice(chunk.get(..n).unwrap_or_default()),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => {
                eof = true;
                break;
            }
        }
        if buffer.len() > MAX_REQUEST_BYTES {
            tracing::warn!(client = id, "ipc: request too large, disconnecting");
            shared.borrow_mut().clients.remove(&id);
            return Ok(PostAction::Remove);
        }
    }
    while let Some(pos) = buffer.iter().position(|b| *b == b'\n') {
        let line: Vec<u8> = buffer.drain(..=pos).collect();
        if handle_line(id, shared, &line, host) == Outcome::Subscribed {
            return Ok(PostAction::Remove);
        }
    }
    if eof {
        // 改行なしで終わる最後の行（書き込み側の shutdown で終わるリクエスト）。
        if !buffer.is_empty() {
            let line = std::mem::take(buffer);
            if handle_line(id, shared, &line, host) == Outcome::Subscribed {
                return Ok(PostAction::Remove);
            }
        }
        // 購読していない接続は、相手が閉じたら閉じる。
        shared.borrow_mut().clients.remove(&id);
        return Ok(PostAction::Remove);
    }
    Ok(PostAction::Continue)
}

/// リクエスト 1 行を処理して、返事を積む。
fn handle_line<D: Host>(
    id: u64,
    shared: &Rc<RefCell<Shared>>,
    line: &[u8],
    host: &mut D,
) -> Outcome {
    let request = protocol::parse_request(line);
    // `Host` の呼び出しは、`shared` を借りる前に済ませる（切り替えは `publish` を通って借りる）。
    let reply: Reply = match &request {
        Request::EventStream | Request::Workspaces => Ok(Response::Handled),
        Request::FocusWorkspaceId(n) => {
            let count = host.ipc_workspaces().len();
            match u8::try_from(n.saturating_sub(1)) {
                Ok(index) if *n >= 1 && usize::from(index) < count => {
                    host.ipc_focus(index);
                    Ok(Response::Handled)
                }
                _ => Err(NOT_FOUND.to_owned()),
            }
        }
        Request::FocusWorkspaceName(name) => {
            tracing::debug!(name, "ipc: focus by name is not supported");
            Err(NAMES_UNSUPPORTED.to_owned())
        }
        Request::Unsupported => Err(UNSUPPORTED.to_owned()),
    };
    match request {
        Request::EventStream => {
            let fresh = host.ipc_workspaces();
            let mut s = shared.borrow_mut();
            s.last = fresh.clone();
            s.send(id, protocol::encode_reply(&reply));
            s.send(
                id,
                protocol::encode_event(&Event::WorkspacesChanged { workspaces: fresh }),
            );
            if let Some(client) = s.clients.get_mut(&id) {
                client.subscribed = true;
            }
            Outcome::Subscribed
        }
        Request::Workspaces => {
            let reply = Ok(Response::Workspaces(host.ipc_workspaces()));
            shared.borrow_mut().send(id, protocol::encode_reply(&reply));
            Outcome::Continue
        }
        _ => {
            shared.borrow_mut().send(id, protocol::encode_reply(&reply));
            Outcome::Continue
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    use smithay::reexports::calloop::EventLoop;

    use super::*;

    /// 偽のホスト: 一覧を持ち、切り替えを記録する。
    struct Fake {
        list: Vec<Workspace>,
        focused: Vec<u8>,
    }

    fn list(count: u8, active: u8) -> Vec<Workspace> {
        (0..count)
            .map(|i| Workspace {
                id: u64::from(i) + 1,
                idx: i + 1,
                name: None,
                output: Some("eDP-1".to_owned()),
                is_urgent: false,
                is_active: i == active,
                is_focused: i == active,
                active_window_id: None,
            })
            .collect()
    }

    impl Host for Fake {
        fn ipc_workspaces(&self) -> Vec<Workspace> {
            self.list.clone()
        }
        fn ipc_focus(&mut self, index: u8) {
            self.focused.push(index);
            self.list = list(u8::try_from(self.list.len()).unwrap_or(0), index);
        }
    }

    /// テスト用のディレクトリ（プロセスとテストの名前で分ける）。
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("contrast-ipc-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    struct Rig {
        event_loop: EventLoop<'static, Fake>,
        host: Fake,
        server: IpcServer,
        dir: PathBuf,
    }

    fn rig(name: &str) -> Result<Rig, String> {
        let event_loop: EventLoop<'static, Fake> =
            EventLoop::try_new().map_err(|e| e.to_string())?;
        let dir = temp_dir(name);
        let host = Fake {
            list: list(4, 0),
            focused: Vec::new(),
        };
        let server = IpcServer::start(&event_loop.handle(), &dir, host.list.clone())
            .map_err(|e| e.to_string())?;
        Ok(Rig {
            event_loop,
            host,
            server,
            dir,
        })
    }

    impl Rig {
        /// イベントループを少し回す。
        fn pump(&mut self) {
            for _ in 0..2 {
                let _ = self
                    .event_loop
                    .dispatch(Some(Duration::from_millis(5)), &mut self.host);
            }
        }

        /// 接続する（読みはノンブロッキング）。
        fn connect(&self) -> Result<UnixStream, String> {
            let stream = UnixStream::connect(self.server.path()).map_err(|e| e.to_string())?;
            stream.set_nonblocking(true).map_err(|e| e.to_string())?;
            Ok(stream)
        }

        /// 行が `want` 本そろうまで（または見込みがなくなるまで）回して読む。EOF なら `eof` が true。
        fn read_lines(&mut self, stream: &UnixStream, want: usize) -> (Vec<String>, bool) {
            let mut bytes = Vec::new();
            let mut eof = false;
            for _ in 0..40 {
                self.pump();
                let mut chunk = [0u8; 4096];
                loop {
                    match (&*stream).read(&mut chunk) {
                        Ok(0) => {
                            eof = true;
                            break;
                        }
                        Ok(n) => bytes.extend_from_slice(chunk.get(..n).unwrap_or_default()),
                        Err(_) => break,
                    }
                }
                let lines = bytes.iter().filter(|b| **b == b'\n').count();
                if lines >= want || eof {
                    break;
                }
            }
            let lines = String::from_utf8_lossy(&bytes)
                .lines()
                .map(str::to_owned)
                .collect();
            (lines, eof)
        }
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir(&self.dir);
        }
    }

    #[test]
    fn the_socket_is_0600_never_overwritten_and_removed_on_drop() -> Result<(), String> {
        let r = rig("socket")?;
        let path = r.server.path().to_owned();
        let mode = std::fs::metadata(&path)
            .map_err(|e| e.to_string())?
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        assert!(
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("contrast.") && n.ends_with(".sock"))
        );
        // 同じパスがあれば、作らずにエラー（上書きしない）。
        let event_loop: EventLoop<'static, Fake> =
            EventLoop::try_new().map_err(|e| e.to_string())?;
        let second = IpcServer::start(&event_loop.handle(), &r.dir, Vec::new());
        assert!(second.is_err());
        assert!(path.exists(), "the first socket must survive");
        drop(second);
        assert!(
            path.exists(),
            "a failed start must not remove the first socket"
        );
        drop(r);
        assert!(!path.exists());
        Ok(())
    }

    #[test]
    fn event_stream_by_shutdown_gets_the_initial_state_then_diffs_and_stays_open()
    -> Result<(), String> {
        let mut r = rig("stream-shutdown")?;
        let stream = r.connect()?;
        // ironbar と同じ: 改行なしで書いて、書き込み側を shutdown する。
        (&stream)
            .write_all(br#""EventStream""#)
            .map_err(|e| e.to_string())?;
        stream
            .shutdown(std::net::Shutdown::Write)
            .map_err(|e| e.to_string())?;
        let (lines, eof) = r.read_lines(&stream, 2);
        assert!(!eof);
        assert_eq!(
            lines.first().map(String::as_str),
            Some(r#"{"Ok":"Handled"}"#)
        );
        assert!(
            lines
                .get(1)
                .is_some_and(|l| l.starts_with(r#"{"WorkspacesChanged":{"workspaces":[{"id":1,"#)),
            "{lines:?}"
        );
        assert_eq!(lines.len(), 2);

        // 切り替えの差分。
        r.server.publish(list(4, 2));
        let (lines, eof) = r.read_lines(&stream, 1);
        assert!(!eof);
        assert_eq!(
            lines,
            [r#"{"WorkspaceActivated":{"id":3,"focused":true}}"#.to_owned()]
        );

        // 変化がなければ何も来ず、接続は開いたまま（サーバーから閉じない）。
        r.server.publish(list(4, 2));
        let (lines, eof) = r.read_lines(&stream, 1);
        assert!(lines.is_empty() && !eof, "{lines:?} eof={eof}");
        assert_eq!(r.server.client_count(), 1);
        Ok(())
    }

    #[test]
    fn event_stream_with_a_newline_works_too() -> Result<(), String> {
        let mut r = rig("stream-newline")?;
        let stream = r.connect()?;
        (&stream)
            .write_all(b"\"EventStream\"\n")
            .map_err(|e| e.to_string())?;
        let (lines, _) = r.read_lines(&stream, 2);
        assert_eq!(lines.len(), 2, "{lines:?}");
        r.server.publish(list(4, 1));
        let (lines, _) = r.read_lines(&stream, 1);
        assert_eq!(
            lines,
            [r#"{"WorkspaceActivated":{"id":2,"focused":true}}"#.to_owned()]
        );
        Ok(())
    }

    #[test]
    fn focus_workspace_switches_through_the_host_and_replies() -> Result<(), String> {
        let mut r = rig("focus")?;
        let stream = r.connect()?;
        (&stream)
            .write_all(br#"{"Action":{"FocusWorkspace":{"reference":{"Id":3}}}}"#)
            .map_err(|e| e.to_string())?;
        stream
            .shutdown(std::net::Shutdown::Write)
            .map_err(|e| e.to_string())?;
        let (lines, eof) = r.read_lines(&stream, 1);
        assert_eq!(lines, [r#"{"Ok":"Handled"}"#.to_owned()]);
        assert_eq!(r.host.focused, [2]);
        // 購読していない接続は、相手が閉じたら閉じる。
        let (_, eof_after) = r.read_lines(&stream, 1);
        assert!(eof || eof_after);
        assert_eq!(r.server.client_count(), 0);
        Ok(())
    }

    #[test]
    fn bad_requests_get_errors_and_do_not_switch() -> Result<(), String> {
        let mut r = rig("errors")?;
        let stream = r.connect()?;
        for request in [
            &br#"{"Action":{"FocusWorkspace":{"reference":{"Id":9}}}}"#[..],
            br#"{"Action":{"FocusWorkspace":{"reference":{"Id":0}}}}"#,
            br#"{"Action":{"FocusWorkspace":{"reference":{"Name":"web"}}}}"#,
            br#""Version""#,
            b"garbage",
        ] {
            (&stream)
                .write_all(request)
                .and_then(|()| (&stream).write_all(b"\n"))
                .map_err(|e| e.to_string())?;
        }
        let (lines, _) = r.read_lines(&stream, 5);
        assert_eq!(
            lines,
            [
                r#"{"Err":"workspace not found"}"#,
                r#"{"Err":"workspace not found"}"#,
                r#"{"Err":"workspace names are not supported"}"#,
                r#"{"Err":"unsupported request"}"#,
                r#"{"Err":"unsupported request"}"#,
            ]
        );
        assert!(r.host.focused.is_empty());
        Ok(())
    }

    #[test]
    fn workspaces_request_returns_the_list() -> Result<(), String> {
        let mut r = rig("workspaces")?;
        let stream = r.connect()?;
        (&stream)
            .write_all(b"\"Workspaces\"\n")
            .map_err(|e| e.to_string())?;
        let (lines, _) = r.read_lines(&stream, 1);
        assert!(
            lines
                .first()
                .is_some_and(|l| l.starts_with(r#"{"Ok":{"Workspaces":[{"id":1,"idx":1,"#)),
            "{lines:?}"
        );
        Ok(())
    }

    #[test]
    fn a_client_that_never_reads_is_dropped_but_others_keep_working() -> Result<(), String> {
        let mut r = rig("slow")?;
        let slow = r.connect()?;
        let fast = r.connect()?;
        for stream in [&slow, &fast] {
            let mut writer = stream;
            writer
                .write_all(b"\"EventStream\"\n")
                .map_err(|e| e.to_string())?;
        }
        let _ = r.read_lines(&fast, 2);
        r.pump();
        assert_eq!(r.server.client_count(), 2);
        // 大きな一覧を何度も送り、`slow` は読まない。`fast` は読み続ける。
        let big = |active: u8| -> Vec<Workspace> {
            (0..32u8)
                .map(|i| Workspace {
                    id: u64::from(i) + 1,
                    idx: i + 1,
                    name: Some("x".repeat(512)),
                    output: Some("eDP-1".to_owned()),
                    is_urgent: false,
                    is_active: i == active,
                    is_focused: i == active,
                    active_window_id: None,
                })
                .collect()
        };
        let mut drained = 0;
        for round in 0..400u32 {
            // 構成が変わるよう、名前を毎回変える。
            let mut next = big(u8::try_from(round % 32).unwrap_or(0));
            if let Some(first) = next.first_mut() {
                first.name = Some(format!("n{round}"));
            }
            r.server.publish(next);
            let (lines, _) = r.read_lines(&fast, 1);
            drained += lines.len();
        }
        assert!(drained > 0);
        assert_eq!(
            r.server.client_count(),
            1,
            "the slow client must be dropped"
        );
        Ok(())
    }
}
