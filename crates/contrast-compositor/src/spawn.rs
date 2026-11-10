//! 子プロセスの起動。`Startup`（起動時に一度だけ）と `spawn` のキーバインドが共有する。
//!
//! 子の環境変数は、コンポジタの環境から開発シェル（`nix develop`）由来のものを除き、
//! `Environment` の組を足し、最後に `WAYLAND_DISPLAY`（このコンポジタのソケット）を置く
//! （`Environment` で上書きさせない）。コンポジタ自身の環境は変えない。
//!
//! 子の標準入力は閉じ、標準出力・標準エラー出力は
//! `${XDG_STATE_HOME:-~/.local/state}/contrast/spawn/<名前>-<UTC 日時>-<pid>.log` に追記で保存する
//! （既存のファイルは上書きしない）。子が起動に失敗しても、あとから読める。

use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use contrast_policy::CursorSettings;

use crate::state::Contrast;

/// 開発シェル（`nix develop`）が足す変数。子には引き継がない。
///
/// コンポジタは開発シェルの中で動くことがあり、そのときの `LD_LIBRARY_PATH` などは
/// 開発シェルのビルド用の値で、利用者のセッションのものではない。
/// シェルに入る前の値は残っていないので、戻すのではなく、取り除く。
const DEVSHELL_VARS: &[&str] = &[
    // `nix develop` は SHELL を stdenv の bash（readline なし）にする。端末がこれを起動すると、プロンプトの
    // `\[` `\]` が文字のまま出る。外すと、端末は利用者のログインシェル（passwd）を使う。
    "SHELL",
    "LD_LIBRARY_PATH",
    "LIBRARY_PATH",
    "PKG_CONFIG",
    "PKG_CONFIG_PATH",
    "RUSTFLAGS",
    "IN_NIX_SHELL",
    "NIX_BINTOOLS",
    "NIX_BINTOOLS_FOR_BUILD",
    "NIX_BUILD_CORES",
    "NIX_BUILD_TOP",
    "NIX_CC",
    "NIX_CC_FOR_BUILD",
    "NIX_CFLAGS_COMPILE",
    "NIX_CFLAGS_COMPILE_FOR_BUILD",
    "NIX_ENFORCE_NO_NATIVE",
    "NIX_GCROOT",
    "NIX_HARDENING_ENABLE",
    "NIX_LDFLAGS",
    "NIX_LDFLAGS_FOR_BUILD",
];

/// 子の環境から取り除く変数の名前。コンポジタが開発シェルの中（`IN_NIX_SHELL` が有る）のときだけ。
pub(crate) fn devshell_vars(in_nix_shell: bool) -> &'static [&'static str] {
    if in_nix_shell { DEVSHELL_VARS } else { &[] }
}

/// 子の環境から取り除く変数の名前。開発シェルの変数に、`NIRI_SOCKET`（`niri_ipc` の指定のない子には、
/// ホストから継承した値も渡さない）を足す。
pub(crate) fn scrub_list(in_nix_shell: bool, niri_ipc: bool) -> Vec<&'static str> {
    let mut list = devshell_vars(in_nix_shell).to_vec();
    if !niri_ipc {
        list.push("NIRI_SOCKET");
    }
    list
}

/// 子に渡す `XCURSOR_THEME`・`XCURSOR_SIZE`（niri が `cursor` の設定から環境変数に設定するのと同じ）。
///
/// テーマ・大きさは、設定（`Cursor`）> コンポジタ自身の環境変数 > 既定（`default`・24）。`Environment` の指定より弱い。
pub(crate) fn cursor_environment(
    cursor: &CursorSettings,
    host_theme: Option<&str>,
    host_size: Option<&str>,
) -> Vec<(String, String)> {
    let theme = cursor
        .theme
        .clone()
        .or_else(|| host_theme.filter(|t| !t.is_empty()).map(str::to_owned))
        .unwrap_or_else(|| "default".to_owned());
    let size = cursor
        .size
        .map(|s| s.to_string())
        .or_else(|| host_size.filter(|s| !s.is_empty()).map(str::to_owned))
        .unwrap_or_else(|| "24".to_owned());
    vec![
        ("XCURSOR_THEME".to_owned(), theme),
        ("XCURSOR_SIZE".to_owned(), size),
    ]
}

/// `command`（空白で区切った先頭がプログラム、残りが引数）から起動するコマンドを作る。空なら `None`。
/// `scrub` の変数は取り除き、そのあとで `environment` を足す（利用者の指定が勝つ）。
pub(crate) fn build_command(
    command: &str,
    socket: &OsStr,
    environment: &[(String, String)],
    scrub: &[&str],
) -> Option<Command> {
    let mut words = command.split_whitespace();
    let program = words.next()?;
    let mut cmd = Command::new(program);
    cmd.args(words);
    cmd.stdin(Stdio::null());
    for name in scrub {
        cmd.env_remove(name);
    }
    for (name, value) in environment {
        cmd.env(name, value);
    }
    cmd.env("WAYLAND_DISPLAY", socket);
    Some(cmd)
}

/// 子のログを置くディレクトリ。`XDG_STATE_HOME`（絶対パスのとき）か `HOME/.local/state` の下の `contrast/spawn`。
pub(crate) fn log_dir(xdg_state_home: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    let state = match xdg_state_home.map(Path::new) {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        _ => Path::new(home.filter(|h| !h.is_empty())?).join(".local/state"),
    };
    Some(state.join("contrast").join("spawn"))
}

/// ログのファイル名に使う、`command` のプログラム名（先頭の語の最後の要素）。
/// 英数字・`.`・`_`・`-` 以外は `_` にする。空なら `spawn`。
pub(crate) fn program_name(command: &str) -> String {
    let first = command.split_whitespace().next().unwrap_or("");
    let base = first.rsplit('/').next().unwrap_or("");
    let name: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() || name.chars().all(|c| c == '.') {
        "spawn".to_owned()
    } else {
        name
    }
}

/// UNIX 時刻（秒）を `YYYYMMDDTHHMMSSZ`（UTC）にする。
pub(crate) fn file_stamp(unix_secs: u64) -> String {
    let days = i64::try_from(unix_secs / 86_400).unwrap_or(0);
    let rest = unix_secs % 86_400;
    // 暦日への変換（days_from_civil の逆。1970-01-01 が 0 日）。
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

/// ログのファイル名 `<名前>-<日時>-<pid>.log`。
pub(crate) fn log_file_name(name: &str, unix_secs: u64, pid: u32) -> String {
    format!("{name}-{}-{pid}.log", file_stamp(unix_secs))
}

/// ログのファイルを新しく作る（追記モード）。同じ名前のファイルがあれば失敗する（上書きしない）。
fn create_log(dir: &Path, file_name: &str) -> io::Result<File> {
    OpenOptions::new()
        .create_new(true)
        .append(true)
        .open(dir.join(file_name))
}

/// `reader` を最後まで `sink` に写す。別スレッドで動かす。
fn pump(mut reader: impl Read + Send + 'static, mut sink: impl io::Write + Send + 'static) {
    std::thread::spawn(move || io::copy(&mut reader, &mut sink));
}

/// 子の標準出力・標準エラー出力を `dir` の新しいログに写し始める。ログのパスを返す。
/// ファイルを作れなかったときは、子が詰まらないよう出力を捨てて `None` を返す。
fn attach_log(child: &mut Child, dir: &Path, name: &str, pid: u32) -> Option<PathBuf> {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let file_name = log_file_name(name, secs, pid);
    let file = match create_log(dir, &file_name) {
        Ok(file) => Some(file),
        Err(err) => {
            tracing::warn!(dir = %dir.display(), file_name, %err, "spawn log: cannot create");
            None
        }
    };
    let sinks = file.and_then(|f| f.try_clone().ok().map(|clone| (f, clone)));
    match (child.stdout.take(), child.stderr.take(), sinks) {
        (Some(out), Some(err), Some((for_out, for_err))) => {
            pump(out, for_out);
            pump(err, for_err);
            Some(dir.join(file_name))
        }
        (out, err, _) => {
            if let Some(out) = out {
                pump(out, io::sink());
            }
            if let Some(err) = err {
                pump(err, io::sink());
            }
            None
        }
    }
}

/// D-Bus の起動用の環境へ渡す変数の名前。niri の `import_environment`（`src/main.rs`）と同じ 5 個。
const ACTIVATION_VARS: [&str; 5] = [
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_TYPE",
    "NIRI_SOCKET",
];

/// `dbus-update-activation-environment` の終了を待つ上限。バスが応答しないときに起動を止めないため。
const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(5);

/// D-Bus の起動用の環境へ渡す `名前=値` の組（`ACTIVATION_VARS` の順）。値が無い変数は含めない。
///
/// 値は、`Startup` の子が見るものと同じ。`WAYLAND_DISPLAY` はこのコンポジタのソケット、`DISPLAY` は XWayland、
/// `NIRI_SOCKET` は IPC のソケット（それぞれ無ければ渡さない）。`XDG_*` は `Environment`（後の指定が勝つ）、
/// 無ければコンポジタ自身の環境（`inherited`）。
pub(crate) fn activation_environment(
    socket: &OsStr,
    display: Option<&str>,
    niri_socket: Option<&Path>,
    environment: &[(String, String)],
    inherited: impl Fn(&str) -> Option<String>,
) -> Vec<(String, String)> {
    let configured = |name: &str| {
        environment
            .iter()
            .rev()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    };
    ACTIVATION_VARS
        .iter()
        .filter_map(|&name| {
            let value = match name {
                "WAYLAND_DISPLAY" => Some(socket.to_string_lossy().into_owned()),
                "DISPLAY" => display.map(str::to_owned),
                "NIRI_SOCKET" => niri_socket.map(|p| p.to_string_lossy().into_owned()),
                _ => configured(name).or_else(|| inherited(name)),
            };
            value
                .filter(|v| !v.is_empty())
                .map(|v| (name.to_owned(), v))
        })
        .collect()
}

/// `program` を `NAME=VALUE ...` で呼んで、`bus_address` のバスの起動用の環境を更新する（待つ）。
/// 開発シェルの変数（`scrub`）は子に渡さない。起動できない・失敗する・`timeout` を過ぎるときは `Err`。
pub(crate) fn update_activation_environment(
    program: &str,
    bus_address: &OsStr,
    variables: &[(String, String)],
    scrub: &[&str],
    timeout: Duration,
) -> io::Result<()> {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null());
    for name in scrub {
        cmd.env_remove(name);
    }
    cmd.env("DBUS_SESSION_BUS_ADDRESS", bus_address);
    for (name, value) in variables {
        cmd.arg(format!("{name}={value}"));
    }
    let mut child = cmd.spawn()?;
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(io::Error::other(format!("{program} exited with {status}")))
            };
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other(format!(
                "{program} did not finish in {}s",
                timeout.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

impl Contrast {
    /// niri の `import_environment` のうち D-Bus の部分。`DBUS_SESSION_BUS_ADDRESS` のバスの起動用の環境に
    /// `ACTIVATION_VARS` を渡す。Wayland のソケットができたあと、`Startup` の前に一度だけ呼ぶ。
    /// 失敗しても起動は続ける（警告だけ）。`systemctl --user import-environment` は呼ばない
    /// （ホストの niri と同じ systemd --user を共有するため）。
    pub(crate) fn import_activation_environment(&self) {
        let Some(address) = std::env::var_os("DBUS_SESSION_BUS_ADDRESS").filter(|a| !a.is_empty())
        else {
            tracing::warn!(
                "dbus: DBUS_SESSION_BUS_ADDRESS is not set; activation environment not updated"
            );
            return;
        };
        let display = self.xwayland.as_ref().map(|x| x.display_name.as_str());
        let variables = activation_environment(
            &self.socket_name,
            display,
            self.ipc_socket.as_deref(),
            &self.policy.environment(),
            |name| std::env::var(name).ok(),
        );
        let scrub = devshell_vars(std::env::var_os("IN_NIX_SHELL").is_some());
        let names: Vec<_> = variables.iter().map(|(k, _)| k.as_str()).collect();
        match update_activation_environment(
            "dbus-update-activation-environment",
            &address,
            &variables,
            scrub,
            ACTIVATION_TIMEOUT,
        ) {
            Ok(()) => tracing::info!(?names, "dbus: activation environment updated"),
            Err(err) => {
                tracing::warn!(%err, ?names, "dbus: cannot update the activation environment")
            }
        }
    }

    /// `command` をこのコンポジタのソケットにつないで起動する。失敗はログに出すだけ。`kind` はログ用の起動元。
    /// `niri_ipc` なら、niri 互換の IPC のソケットのパスを `NIRI_SOCKET` で渡す（IPC が無ければ渡せない）。
    pub(crate) fn spawn_command(&self, command: &str, kind: &'static str, niri_ipc: bool) {
        // カーソルの環境変数を先に、利用者の `Environment` を後に置く（同じ名前は後の指定が勝つ）。
        let mut environment = cursor_environment(
            &self.policy.cursor_settings(),
            std::env::var("XCURSOR_THEME").ok().as_deref(),
            std::env::var("XCURSOR_SIZE").ok().as_deref(),
        );
        environment.extend(self.policy.environment());
        let niri_socket = if niri_ipc {
            if self.ipc_socket.is_none() {
                tracing::warn!(
                    kind,
                    command,
                    "ipc=\"niri\" but the IPC socket is not available"
                );
            }
            self.ipc_socket.as_deref()
        } else {
            None
        };
        let scrub = scrub_list(
            std::env::var_os("IN_NIX_SHELL").is_some(),
            niri_socket.is_some(),
        );
        let Some(mut cmd) = build_command(command, &self.socket_name, &environment, &scrub) else {
            return;
        };
        if let Some(path) = niri_socket {
            // コンポジタの値が勝つ（`Environment` で上書きさせない）。
            cmd.env("NIRI_SOCKET", path);
        }
        // X のディスプレイは、このコンポジタの XWayland（niri と同じく、無ければ取り除き、ホストのものを渡さない）。
        match &self.xwayland {
            Some(x) => cmd.env("DISPLAY", &x.display_name),
            None => cmd.env_remove("DISPLAY"),
        };
        let dir = log_dir(
            std::env::var_os("XDG_STATE_HOME").as_deref(),
            std::env::var_os("HOME").as_deref(),
        )
        .and_then(|dir| match std::fs::create_dir_all(&dir) {
            Ok(()) => Some(dir),
            Err(err) => {
                tracing::warn!(dir = %dir.display(), %err, "spawn log: cannot create the directory");
                None
            }
        });
        if dir.is_some() {
            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        }
        match cmd.spawn() {
            Ok(mut child) => {
                let pid = child.id();
                let log =
                    dir.and_then(|dir| attach_log(&mut child, &dir, &program_name(command), pid));
                tracing::info!(kind, command, pid, log = ?log, "spawned");
                let command = command.to_owned();
                // ゾンビを残さないよう、別スレッドで終了を待つ。消えたことが分かるよう、終了もログに出す。
                std::thread::spawn(move || match child.wait() {
                    Ok(status) => tracing::info!(kind, command, pid, %status, "child exited"),
                    Err(err) => tracing::warn!(kind, command, pid, %err, "child wait failed"),
                });
            }
            Err(err) => tracing::warn!(kind, command, %err, "spawn failed"),
        }
    }

    /// `Startup` のコマンドを、並びの順に起動する（Wayland のソケットを公開したあとに一度だけ呼ぶ）。
    /// 失敗しても残りを続ける。リロードでは呼ばない。
    pub(crate) fn run_startup(&self) {
        let commands = self.policy.startup_commands();
        tracing::info!(count = commands.len(), "startup: running commands");
        for command in &commands {
            self.spawn_command(&command.command, "startup", command.niri_ipc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// コマンドの環境変数の指定（`env_remove` は含まない）。
    fn envs(cmd: &Command) -> Vec<(String, String)> {
        cmd.get_envs()
            .filter_map(|(k, v)| {
                Some((
                    k.to_string_lossy().into_owned(),
                    v?.to_string_lossy().into_owned(),
                ))
            })
            .collect()
    }

    /// コマンドが子の環境から取り除く変数の名前（`env_remove`）。
    fn removed(cmd: &Command) -> Vec<String> {
        cmd.get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect()
    }

    /// テスト用の空のディレクトリ（プロセスごとに別。テストごとに別の名前を渡す）。
    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("contrast-spawn-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn program_and_arguments_are_split_on_whitespace() {
        let cmd = build_command("ironbar -c  /a.toml", OsStr::new("wayland-9"), &[], &[]);
        let cmd = cmd.expect("command");
        assert_eq!(cmd.get_program(), "ironbar");
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args, ["-c", "/a.toml"]);
    }

    #[test]
    fn empty_command_is_none() {
        assert!(build_command("", OsStr::new("w"), &[], &[]).is_none());
        assert!(build_command("   ", OsStr::new("w"), &[], &[]).is_none());
    }

    #[test]
    fn environment_is_passed_and_the_socket_wins() {
        let env = vec![
            ("QT_IM_MODULE".to_owned(), "fcitx5".to_owned()),
            ("WAYLAND_DISPLAY".to_owned(), "evil".to_owned()),
        ];
        let cmd = build_command("foot", OsStr::new("wayland-9"), &env, &[]).expect("command");
        let got = envs(&cmd);
        assert!(got.contains(&("QT_IM_MODULE".to_owned(), "fcitx5".to_owned())));
        // 同じ名前は後の指定が勝つ（Command は最後の指定を持つ）。
        let wayland: Vec<_> = got.iter().filter(|(k, _)| k == "WAYLAND_DISPLAY").collect();
        assert_eq!(
            wayland,
            [&("WAYLAND_DISPLAY".to_owned(), "wayland-9".to_owned())]
        );
    }

    #[test]
    fn niri_socket_is_scrubbed_unless_the_command_asks_for_it() {
        assert!(scrub_list(false, false).contains(&"NIRI_SOCKET"));
        assert!(scrub_list(true, false).contains(&"NIRI_SOCKET"));
        assert!(!scrub_list(false, true).contains(&"NIRI_SOCKET"));
        assert!(!scrub_list(true, true).contains(&"NIRI_SOCKET"));
        // 開発シェルの変数は、`niri_ipc` に関係なく同じ条件で外す。
        assert!(scrub_list(true, true).contains(&"LD_LIBRARY_PATH"));
        assert!(!scrub_list(false, true).contains(&"LD_LIBRARY_PATH"));
    }

    #[test]
    fn a_child_without_ipc_loses_niri_socket_and_one_with_ipc_gets_ours() {
        let run = |niri_ipc: bool| {
            let scrub = scrub_list(false, niri_ipc);
            let env = vec![("NIRI_SOCKET".to_owned(), "/user/set".to_owned())];
            let mut cmd = build_command("sh -c", OsStr::new("w"), &env, &scrub).expect("command");
            if niri_ipc {
                cmd.env("NIRI_SOCKET", "/run/user/1/contrast.1.sock");
            }
            cmd.arg("printf '%s' \"${NIRI_SOCKET-unset}\"").output()
        };
        // `Environment` の指定は、IPC のない子では（取り除いたあとで足すので）残る。
        if let Ok(out) = run(false) {
            assert_eq!(String::from_utf8_lossy(&out.stdout), "/user/set");
        }
        // IPC のある子では、コンポジタの値が勝つ。
        if let Ok(out) = run(true) {
            assert_eq!(
                String::from_utf8_lossy(&out.stdout),
                "/run/user/1/contrast.1.sock"
            );
        }
    }

    #[test]
    fn cursor_environment_follows_the_config_then_the_host_then_the_default() {
        let cfg = |theme: Option<&str>, size: Option<u32>| CursorSettings {
            theme: theme.map(str::to_owned),
            size,
            hide_after_inactive_ms: None,
        };
        let pair = |e: Vec<(String, String)>| {
            let get = |k: &str| e.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
            (get("XCURSOR_THEME"), get("XCURSOR_SIZE"))
        };
        let some = |a: &str, b: &str| (Some(a.to_owned()), Some(b.to_owned()));
        // 設定が最優先（niri の cursor { xcursor-theme "Cecilia"; xcursor-size 24 }）。
        assert_eq!(
            pair(cursor_environment(
                &cfg(Some("Cecilia"), Some(24)),
                Some("x"),
                Some("48")
            )),
            some("Cecilia", "24")
        );
        // 設定が無ければコンポジタ自身の環境変数。
        assert_eq!(
            pair(cursor_environment(
                &cfg(None, None),
                Some("Adwaita"),
                Some("32")
            )),
            some("Adwaita", "32")
        );
        // どちらも無ければ既定（niri と同じ default・24）。空の環境変数は無いものとして扱う。
        assert_eq!(
            pair(cursor_environment(&cfg(None, None), Some(""), None)),
            some("default", "24")
        );
    }

    #[test]
    fn the_compositor_environment_is_not_touched() {
        let before = std::env::var_os("CONTRAST_SPAWN_TEST_VAR");
        let env = vec![("CONTRAST_SPAWN_TEST_VAR".to_owned(), "x".to_owned())];
        let _ = build_command("true", OsStr::new("w"), &env, &[]);
        assert_eq!(std::env::var_os("CONTRAST_SPAWN_TEST_VAR"), before);
    }

    #[test]
    fn devshell_vars_are_removed_only_inside_a_devshell() {
        assert!(devshell_vars(false).is_empty());
        let scrub = devshell_vars(true);
        for name in [
            "LD_LIBRARY_PATH",
            "IN_NIX_SHELL",
            "NIX_CFLAGS_COMPILE",
            "SHELL",
        ] {
            assert!(scrub.contains(&name), "{name}");
        }
        // システムの変数は残す。
        for name in [
            "PATH",
            "NIX_PATH",
            "NIX_PROFILES",
            "NIX_LD",
            "NIX_LD_LIBRARY_PATH",
        ] {
            assert!(!scrub.contains(&name), "{name}");
        }
        let cmd = build_command("foot", OsStr::new("w"), &[], scrub).expect("command");
        assert!(removed(&cmd).contains(&"LD_LIBRARY_PATH".to_owned()));
    }

    #[test]
    fn a_value_in_environment_wins_over_the_scrub() {
        let env = vec![("LD_LIBRARY_PATH".to_owned(), "/user/lib".to_owned())];
        let cmd =
            build_command("foot", OsStr::new("w"), &env, devshell_vars(true)).expect("command");
        assert!(envs(&cmd).contains(&("LD_LIBRARY_PATH".to_owned(), "/user/lib".to_owned())));
        assert!(!removed(&cmd).contains(&"LD_LIBRARY_PATH".to_owned()));
    }

    /// 実際に子を起動して、環境変数が子に届き、取り除いた変数が見えないことを確かめる（`sh` がある環境）。
    #[test]
    fn the_child_sees_the_environment() {
        let env = vec![("QT_IM_MODULE".to_owned(), "fcitx5".to_owned())];
        let scrub = ["HOME"];
        let mut cmd =
            build_command("sh -c", OsStr::new("wayland-9"), &env, &scrub).expect("command");
        let out = cmd
            .arg("printf '%s %s %s' \"$QT_IM_MODULE\" \"$WAYLAND_DISPLAY\" \"${HOME-unset}\"")
            .output();
        // `sh` が無い環境では起動できないので、起動できたときだけ中身を見る。
        if let Ok(out) = out {
            assert_eq!(
                String::from_utf8_lossy(&out.stdout),
                "fcitx5 wayland-9 unset"
            );
        }
    }

    #[test]
    fn the_child_has_no_stdin() {
        let mut cmd = build_command("sh -c", OsStr::new("w"), &[], &[]).expect("command");
        // 標準入力が閉じている（/dev/null）ので、読んでもすぐ終わる。
        let out = cmd.arg("cat; printf done").output();
        if let Ok(out) = out {
            assert_eq!(String::from_utf8_lossy(&out.stdout), "done");
        }
    }

    #[test]
    fn log_dir_follows_xdg_state_home_then_home() {
        let some = |s: &'static str| Some(OsStr::new(s));
        assert_eq!(
            log_dir(some("/state"), some("/home/u")),
            Some(PathBuf::from("/state/contrast/spawn"))
        );
        assert_eq!(
            log_dir(None, some("/home/u")),
            Some(PathBuf::from("/home/u/.local/state/contrast/spawn"))
        );
        // 空や相対パスの XDG_STATE_HOME は使わない。
        assert_eq!(
            log_dir(some(""), some("/home/u")),
            Some(PathBuf::from("/home/u/.local/state/contrast/spawn"))
        );
        assert_eq!(
            log_dir(some("rel/state"), some("/home/u")),
            Some(PathBuf::from("/home/u/.local/state/contrast/spawn"))
        );
        assert_eq!(log_dir(None, None), None);
        assert_eq!(log_dir(None, some("")), None);
    }

    #[test]
    fn program_name_is_a_safe_file_name() {
        assert_eq!(program_name("ironbar -c /a.toml"), "ironbar");
        assert_eq!(program_name("/usr/bin/alacritty"), "alacritty");
        assert_eq!(program_name("env DISPLAY=:0 x"), "env");
        assert_eq!(program_name("we ird"), "we");
        assert_eq!(program_name("a\u{3042}b"), "a_b");
        assert_eq!(program_name(""), "spawn");
        assert_eq!(program_name(".."), "spawn");
        assert_eq!(program_name("/usr/bin/"), "spawn");
    }

    #[test]
    fn file_stamp_is_utc_and_handles_leap_days() {
        assert_eq!(file_stamp(0), "19700101T000000Z");
        assert_eq!(file_stamp(1_793_955_058), "20261106T085058Z");
        assert_eq!(file_stamp(1_709_208_000), "20240229T120000Z");
        assert_eq!(file_stamp(951_782_400), "20000229T000000Z");
        assert_eq!(file_stamp(4_102_444_799), "20991231T235959Z");
    }

    #[test]
    fn log_file_name_has_name_stamp_and_pid() {
        assert_eq!(
            log_file_name("ironbar", 1_793_955_058, 2884857),
            "ironbar-20261106T085058Z-2884857.log"
        );
    }

    #[test]
    fn an_existing_log_is_never_overwritten() {
        let dir = temp_dir("no-overwrite");
        let name = "x-20261106T085058Z-1.log";
        let mut first = create_log(&dir, name).expect("first");
        io::Write::write_all(&mut first, b"old").expect("write");
        assert!(create_log(&dir, name).is_err());
        assert_eq!(std::fs::read(dir.join(name)).expect("read"), b"old");
    }

    #[test]
    fn attach_log_saves_stdout_and_stderr_of_the_child() {
        let dir = temp_dir("attach");
        let mut cmd = build_command("sh -c", OsStr::new("w"), &[], &[]).expect("command");
        cmd.arg("echo to-out; echo to-err >&2")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let Ok(mut child) = cmd.spawn() else { return };
        let pid = child.id();
        let path = attach_log(&mut child, &dir, "sh", pid).expect("log path");
        child.wait().expect("wait");
        // 写すスレッドは子が閉じたあとに終わるので、少し待って読む。
        let mut text = String::new();
        for _ in 0..200 {
            text = std::fs::read_to_string(&path).unwrap_or_default();
            if text.contains("to-out") && text.contains("to-err") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            text.contains("to-out") && text.contains("to-err"),
            "{text:?}"
        );
        assert!(path.starts_with(&dir));
        assert!(path.to_string_lossy().ends_with(&format!("-{pid}.log")));
    }

    fn pair(k: &str, v: &str) -> (String, String) {
        (k.to_owned(), v.to_owned())
    }

    #[test]
    fn activation_environment_uses_what_startup_children_see() {
        let environment = vec![
            pair("XDG_CURRENT_DESKTOP", "first"),
            pair("XDG_CURRENT_DESKTOP", "niri"),
        ];
        let got = activation_environment(
            OsStr::new("wayland-7"),
            Some(":3"),
            Some(Path::new("/run/user/1/contrast.sock")),
            &environment,
            |name| (name == "XDG_SESSION_TYPE").then(|| "tty".to_owned()),
        );
        assert_eq!(
            got,
            [
                pair("WAYLAND_DISPLAY", "wayland-7"),
                pair("DISPLAY", ":3"),
                pair("XDG_CURRENT_DESKTOP", "niri"),
                pair("XDG_SESSION_TYPE", "tty"),
                pair("NIRI_SOCKET", "/run/user/1/contrast.sock"),
            ]
        );
    }

    #[test]
    fn activation_environment_skips_what_has_no_value() {
        let got = activation_environment(OsStr::new("w"), None, None, &[], |_| Some(String::new()));
        assert_eq!(got, [pair("WAYLAND_DISPLAY", "w")]);
    }

    #[test]
    fn update_activation_environment_reports_failures() {
        let t = Duration::from_secs(5);
        let addr = OsStr::new("unix:path=/nonexistent");
        assert!(update_activation_environment("/nonexistent/prog", addr, &[], &[], t).is_err());
        assert!(update_activation_environment("false", addr, &[], &[], t).is_err());
        assert!(update_activation_environment("true", addr, &[], &[], t).is_ok());
    }

    #[test]
    fn update_activation_environment_gives_up_after_the_timeout() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("slow-updater");
        let script = dir.join("slow.sh");
        std::fs::write(&script, "#!/bin/sh\nexec sleep 30\n").expect("write");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let started = Instant::now();
        let result = update_activation_environment(
            script.to_str().expect("utf8"),
            OsStr::new("unix:path=/nonexistent"),
            &[],
            &[],
            Duration::from_millis(200),
        );
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    /// サービスを起動しない設定の専用 dbus-daemon（テスト用のサービス 1 個だけ。ホストのサービスは見ない）で、
    /// 更新の前は起動されたプロセスに `WAYLAND_DISPLAY` が無く、更新の後は有ることを確かめる。
    /// dbus-daemon・dbus-send・dbus-update-activation-environment が無い環境では何もしない。
    #[test]
    fn update_activation_environment_reaches_services_started_by_a_private_bus() {
        use std::os::unix::fs::PermissionsExt;
        let on_path = |p: &str| {
            Command::new(p)
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok()
        };
        if !(on_path("dbus-daemon")
            && on_path("dbus-send")
            && on_path("dbus-update-activation-environment"))
        {
            eprintln!("skipped: dbus tools are not installed");
            return;
        }
        let dir = temp_dir("private-bus");
        let services = dir.join("services");
        std::fs::create_dir_all(&services).expect("services dir");
        let probe = dir.join("probe.sh");
        let out = dir.join("out.txt");
        std::fs::write(
            &probe,
            format!("#!/bin/sh\nprintf 'WD=%s DISPLAY=%s\\n' \"$WAYLAND_DISPLAY\" \"$DISPLAY\" >> {}\nsleep 1\n", out.display()),
        )
        .expect("probe");
        std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        std::fs::write(
            services.join("probe-before.service"),
            format!(
                "[D-BUS Service]\nName=org.contrast.ProbeBefore\nExec={}\n",
                probe.display()
            ),
        )
        .expect("service");
        std::fs::write(
            services.join("probe-after.service"),
            format!(
                "[D-BUS Service]\nName=org.contrast.ProbeAfter\nExec={}\n",
                probe.display()
            ),
        )
        .expect("service");
        let conf = dir.join("bus.conf");
        std::fs::write(
            &conf,
            format!(
                "<busconfig><type>session</type><listen>unix:path={}/bus</listen><auth>EXTERNAL</auth>\
                 <servicedir>{}</servicedir>\
                 <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
                 <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>",
                dir.display(),
                services.display()
            ),
        )
        .expect("conf");
        let mut daemon = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", conf.display()))
            .args(["--nofork", "--print-address=1"])
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon");
        let mut address = String::new();
        io::BufRead::read_line(
            &mut io::BufReader::new(daemon.stdout.take().expect("stdout")),
            &mut address,
        )
        .expect("address");
        let address = address.trim().to_owned();
        let activate = |name: &str| {
            let _ = Command::new("dbus-send")
                .env("DBUS_SESSION_BUS_ADDRESS", &address)
                .args([
                    "--session",
                    "--print-reply",
                    "--reply-timeout=3000",
                    &format!("--dest=org.contrast.{name}"),
                    "/",
                    "org.freedesktop.DBus.Peer.Ping",
                ])
                .output();
        };
        activate("ProbeBefore");
        let result = update_activation_environment(
            "dbus-update-activation-environment",
            OsStr::new(&address),
            &[pair("WAYLAND_DISPLAY", "wayland-test-9")],
            &[],
            Duration::from_secs(5),
        );
        activate("ProbeAfter");
        let _ = daemon.kill();
        let _ = daemon.wait();
        assert!(result.is_ok(), "{result:?}");
        let text = std::fs::read_to_string(&out).unwrap_or_default();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(
            lines,
            ["WD= DISPLAY=", "WD=wayland-test-9 DISPLAY="],
            "{text:?}"
        );
    }

    #[test]
    fn attach_log_drains_the_child_when_the_file_cannot_be_created() {
        // ディレクトリが無いのでファイルは作れない。子の出力は捨てられ、子は詰まらずに終わる。
        let dir = temp_dir("missing").join("nope");
        let mut cmd = build_command("sh -c", OsStr::new("w"), &[], &[]).expect("command");
        cmd.arg("echo out; echo err >&2")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let Ok(mut child) = cmd.spawn() else { return };
        let pid = child.id();
        assert_eq!(attach_log(&mut child, &dir, "sh", pid), None);
        assert!(child.wait().expect("wait").success());
    }
}
