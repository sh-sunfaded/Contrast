//! XWayland（niri と同じく、xwayland-satellite を必要になったときに起動する。niri の `src/utils/xwayland/`）。
//!
//! 起動時に、`xwayland-satellite :0 --test-listenfd-support` で対応を確かめ、空いている X のディスプレイ番号（`/tmp/.X{n}-lock`）を取り、
//! 抽象ソケット `@/tmp/.X11-unix/X{n}` と `/tmp/.X11-unix/X{n}` で待つ。X のクライアントが接続してきたら、
//! `xwayland-satellite :n -listenfd …` を起動してソケットを渡す（satellite が Xwayland を起動し、Contrast に Wayland のクライアントとしてつなぐ）。
//! satellite が終わったら、また待つ。子プロセスには `DISPLAY=:n` を渡す（使えなければ `DISPLAY` を取り除く。niri と同じ）。
//! 終了時に、ロックファイルとソケットのファイルを消す（このプロセスが作ったもの）。

use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::os::unix::net::{SocketAddr, UnixListener};
use std::process::{Command, Stdio};

use smithay::reexports::calloop::{
    Interest, LoopHandle, Mode, PostAction, RegistrationToken,
    channel::{self, Event},
    generic::Generic,
};
use smithay::reexports::rustix;

use crate::state::Contrast;

/// satellite のプログラム（niri の既定の `xwayland-satellite { path "xwayland-satellite"; }`）。
const SATELLITE: &str = "xwayland-satellite";

/// 作ったファイルを、終わるときに消す。
struct Unlink(String);

impl Drop for Unlink {
    fn drop(&mut self) {
        let _ = rustix::fs::unlink(&self.0);
    }
}

/// 待っている X のソケット。
pub struct Xwayland {
    /// ディスプレイの名前（`:n`）。
    pub display_name: String,
    abstract_fd: OwnedFd,
    unix_fd: OwnedFd,
    tokens: Vec<RegistrationToken>,
    _unix_guard: Unlink,
    _lock_guard: Unlink,
}

/// satellite が `-listenfd` に対応しているか（niri の `test_ondemand`）。
fn supports_listenfd() -> bool {
    Command::new(SATELLITE)
        .args([":0", "--test-listenfd-support"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env_remove("DISPLAY")
        .status()
        .is_ok_and(|s| s.success())
}

/// `/tmp/.X11-unix` を用意する（無ければ 1777 で作る）。
fn ensure_x11_dir() -> Result<(), String> {
    match rustix::fs::mkdir("/tmp/.X11-unix", 0o1777.into()) {
        Ok(()) | Err(rustix::io::Errno::EXIST) => Ok(()),
        Err(e) => Err(format!("mkdir /tmp/.X11-unix: {e}")),
    }
}

/// 空いているディスプレイ番号を取り、ロックファイルに pid を書く。
fn pick_display() -> Result<(u32, Unlink), String> {
    use rustix::fs::OFlags;
    for n in 0..50 {
        let path = format!("/tmp/.X{n}-lock");
        let flags = OFlags::WRONLY | OFlags::CLOEXEC | OFlags::CREATE | OFlags::EXCL;
        let Ok(fd) = rustix::fs::open(&path, flags, 0o444.into()) else {
            continue;
        };
        let pid = format!("{:>10}\n", std::process::id());
        rustix::io::write(&fd, pid.as_bytes()).map_err(|e| format!("write {path}: {e}"))?;
        return Ok((n, Unlink(path)));
    }
    Err("no free X11 display after 50 attempts".to_string())
}

/// ディスプレイ `n` の 2 つのソケットで待つ。
fn bind(n: u32) -> Result<(UnixListener, UnixListener, Unlink), String> {
    use std::os::linux::net::SocketAddrExt;
    let name = format!("/tmp/.X11-unix/X{n}");
    let abstract_addr = SocketAddr::from_abstract_name(&name).map_err(|e| e.to_string())?;
    let a = UnixListener::bind_addr(&abstract_addr).map_err(|e| format!("bind @{name}: {e}"))?;
    // 前のものが残っていれば消す（niri と同じ）。
    let _ = rustix::fs::unlink(&name);
    let u = UnixListener::bind(&name).map_err(|e| format!("bind {name}: {e}"))?;
    Ok((a, u, Unlink(name)))
}

/// 接続を待っている間にたまった接続を受けて捨てる（satellite が起動できなかったときの空回りを防ぐ。niri と同じ）。
fn drain(fd: BorrowedFd<'_>) {
    use rustix::net::{SocketFlags, accept_with};
    while accept_with(fd, SocketFlags::NONBLOCK | SocketFlags::CLOEXEC).is_ok() {}
}

impl Contrast {
    /// XWayland の用意（起動時に 1 回）。使えなければ、ログに出して何もしない。
    pub fn setup_xwayland(&mut self, handle: &LoopHandle<'static, Contrast>) {
        if !supports_listenfd() {
            tracing::warn!(
                "xwayland: {SATELLITE} is missing or has no -listenfd support, X11 clients are disabled"
            );
            return;
        }
        let made = ensure_x11_dir().and_then(|()| {
            let (n, lock) = pick_display()?;
            let (a, u, unix_guard) = bind(n)?;
            Ok((n, lock, a, u, unix_guard))
        });
        let (n, lock, a, u, unix_guard) = match made {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("xwayland: {e}");
                return;
            }
        };
        for l in [&a, &u] {
            let _ = l.set_nonblocking(true);
        }
        self.xwayland = Some(Xwayland {
            display_name: format!(":{n}"),
            abstract_fd: OwnedFd::from(a),
            unix_fd: OwnedFd::from(u),
            tokens: Vec::new(),
            _unix_guard: unix_guard,
            _lock_guard: lock,
        });
        tracing::info!(
            name = format!(":{n}"),
            "xwayland: listening (xwayland-satellite starts on the first X11 client)"
        );
        self.watch_xwayland(handle);
    }

    /// X のソケットへの接続を待つ（来たら satellite を起動する）。
    fn watch_xwayland(&mut self, handle: &LoopHandle<'static, Contrast>) {
        let Some(x) = self.xwayland.as_mut() else {
            return;
        };
        drain(x.abstract_fd.as_fd());
        drain(x.unix_fd.as_fd());
        let mut tokens = Vec::new();
        for fd in [&x.abstract_fd, &x.unix_fd] {
            let Ok(dup) = fd.try_clone() else { continue };
            let h = handle.clone();
            let source = Generic::new(dup, Interest::READ, Mode::Level);
            match handle.insert_source(source, move |_, _, state| {
                state.spawn_satellite(&h);
                Ok(PostAction::Remove)
            }) {
                Ok(t) => tokens.push(t),
                Err(e) => tracing::warn!("xwayland: cannot watch the socket: {e}"),
            }
        }
        x.tokens = tokens;
    }

    /// satellite を起動する（別スレッドで待ち、終わったら、また待つ）。
    fn spawn_satellite(&mut self, handle: &LoopHandle<'static, Contrast>) {
        let Some(x) = self.xwayland.as_mut() else {
            return;
        };
        for t in std::mem::take(&mut x.tokens) {
            handle.remove(t);
        }
        let (Ok(a), Ok(u)) = (x.abstract_fd.try_clone(), x.unix_fd.try_clone()) else {
            return;
        };
        let display = x.display_name.clone();
        let (tx, rx) = channel::channel::<()>();
        let h = handle.clone();
        if let Err(e) = handle.insert_source(rx, move |event, _, state| {
            if let Event::Msg(()) = event {
                state.watch_xwayland(&h);
            }
        }) {
            tracing::warn!("xwayland: {e}");
        }
        let wayland = self.socket_name.clone();
        let spawned = std::thread::Builder::new()
            .name("xwl-satellite".into())
            .spawn(move || {
                run_satellite(&display, &wayland, a, u);
                let _ = tx.send(());
            });
        if let Err(e) = spawned {
            tracing::warn!("xwayland: cannot start the spawner thread: {e}");
        }
    }
}

/// satellite を起動して終わるまで待つ。
fn run_satellite(display_name: &str, wayland: &std::ffi::OsStr, a: OwnedFd, u: OwnedFd) {
    use std::os::unix::process::CommandExt;
    let (a_raw, u_raw) = (a.as_raw_fd(), u.as_raw_fd());
    let mut cmd = Command::new(SATELLITE);
    cmd.arg(display_name)
        .args([
            "-listenfd",
            &u_raw.to_string(),
            "-listenfd",
            &a_raw.to_string(),
        ])
        .env_remove("DISPLAY")
        .env("WAYLAND_DISPLAY", wayland)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: fork と exec の間では、渡す 2 つの fd の CLOEXEC を外すだけ（async-signal-safe な fcntl だけを使い、確保や
    // ロックをしない）。fd はこの関数が持っていて、spawn が終わるまで閉じない。
    unsafe {
        cmd.pre_exec(move || {
            for raw in [a_raw, u_raw] {
                let fd = BorrowedFd::borrow_raw(raw);
                rustix::io::fcntl_setfd(fd, rustix::io::FdFlags::empty())?;
            }
            Ok(())
        });
    }
    tracing::info!(name = display_name, "xwayland: starting xwayland-satellite");
    match cmd.spawn() {
        Ok(mut child) => {
            drop((a, u));
            match child.wait() {
                Ok(status) => tracing::warn!(%status, "xwayland: xwayland-satellite exited"),
                Err(e) => tracing::warn!("xwayland: wait: {e}"),
            }
        }
        Err(e) => tracing::warn!("xwayland: cannot start {SATELLITE}: {e}"),
    }
}
