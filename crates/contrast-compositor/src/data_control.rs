//! wlr-data-control（`zwlr_data_control_manager_v1`）の公開と、使えるクライアントの絞り込み。
//!
//! このプロトコルは、フォーカスがなくてもクリップボードを読み書きできる（ironbar の clipboard モジュール、
//! wl-clipboard、cliphist が使う）。信頼できないクライアントに読まれないよう、クライアントのプロセス
//! （接続のソケットの資格情報の pid → `/proc/<pid>/exe`、取れなければ `comm`）の名前が、
//! 設定の許可リスト（`DataControlClient`）にあるときだけ、グローバルを見せる。名前が取れなければ見せない。
//! 許可リストはリロードで更新する（以後に接続するクライアントから効く）。

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, PoisonError};

use smithay::reexports::wayland_server::Client;

use crate::state::{ClientState, Contrast};

/// 許可リスト。フィルタ（別スレッドから呼べる必要がある）と `Contrast` が共有する。
pub(crate) type AllowList = Arc<Mutex<Vec<String>>>;

/// `/proc/<pid>/comm` の長さの上限（NUL を除く）。
const COMM_LEN: usize = 15;

/// 実行ファイルの名前から、照合に使うプログラム名を取り出す。
/// パスの最後の要素にし、nix のラッパー（`.name-wrapped`）は `name` とみなす。
pub(crate) fn program_name(exe: &Path) -> Option<String> {
    let base = exe.file_name()?.to_str()?;
    let name = base
        .strip_prefix('.')
        .and_then(|rest| rest.strip_suffix("-wrapped"))
        .filter(|inner| !inner.is_empty())
        .unwrap_or(base);
    (!name.is_empty()).then(|| name.to_owned())
}

/// `exe`（実行ファイルのプログラム名）か、それが取れないときの `comm` が、許可リストにあるか。
/// どちらも取れなければ許可しない。`comm` は 15 バイトに切られるので、許可の名前も同じ長さに切って比べる。
pub(crate) fn is_allowed(allowed: &[String], exe: Option<&str>, comm: Option<&str>) -> bool {
    match (exe, comm) {
        (Some(exe), _) => allowed.iter().any(|name| name == exe),
        (None, Some(comm)) => allowed
            .iter()
            .any(|name| name.as_bytes().iter().take(COMM_LEN).eq(comm.as_bytes())),
        (None, None) => false,
    }
}

/// `pid` のプロセスの `(exe のプログラム名, comm)`。読めないものは `None`。
fn process_names(pid: i32) -> (Option<String>, Option<String>) {
    let exe = std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .and_then(|path| program_name(&path));
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|text| text.trim_end_matches('\n').to_owned());
    (exe, comm)
}

/// 許可リストでクライアントを絞るフィルタを作る。許可しないクライアントは、pid ごとに一度だけ警告に出す。
pub(crate) fn client_filter(
    allow: AllowList,
) -> impl for<'c> Fn(&'c Client) -> bool + Send + Sync + 'static {
    let warned: Mutex<BTreeSet<i32>> = Mutex::new(BTreeSet::new());
    move |client| {
        // pid は接続時に ClientState へ残してある（ここはディスパッチの中なので、資格情報は調べ直さない）。
        let pid = client
            .get_data::<ClientState>()
            .map_or(0, |data| data.pid.load(Ordering::Relaxed));
        if pid <= 0 {
            return false;
        }
        let (exe, comm) = process_names(pid);
        let allowed = {
            let list = allow.lock().unwrap_or_else(PoisonError::into_inner);
            is_allowed(&list, exe.as_deref(), comm.as_deref())
        };
        if !allowed {
            let first = warned
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(pid);
            if first {
                tracing::warn!(
                    pid = pid,
                    exe = ?exe,
                    comm = ?comm,
                    "data-control: hidden from a client that is not in the allow list"
                );
            }
        }
        allowed
    }
}

impl Contrast {
    /// 設定の許可リストを、フィルタが見る共有の一覧に写す。
    pub(crate) fn refresh_data_control_clients(&self) {
        let clients = self.policy.data_control_clients();
        *self
            .data_control_clients
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = clients;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn program_name_takes_the_file_name_and_unwraps_nix_wrappers() {
        let name = |p: &str| program_name(Path::new(p));
        assert_eq!(name("/usr/bin/wl-copy").as_deref(), Some("wl-copy"));
        assert_eq!(
            name("/nix/store/x-ironbar-0.19.1/bin/.ironbar-wrapped").as_deref(),
            Some("ironbar")
        );
        // 先頭が `.` だけ、`-wrapped` だけは外さない。
        assert_eq!(name("/a/.hidden").as_deref(), Some(".hidden"));
        assert_eq!(name("/a/x-wrapped").as_deref(), Some("x-wrapped"));
        assert_eq!(name("/a/.-wrapped").as_deref(), Some(".-wrapped"));
        assert_eq!(name("/"), None);
    }

    #[test]
    fn only_listed_names_are_allowed() {
        let allow = list(&["ironbar", "wl-copy"]);
        assert!(is_allowed(&allow, Some("wl-copy"), Some("wl-copy")));
        assert!(!is_allowed(&allow, Some("evil-paste"), Some("wl-copy")));
        assert!(!is_allowed(&allow, Some("wl-copy2"), None));
        assert!(!is_allowed(&[], Some("wl-copy"), None));
    }

    #[test]
    fn a_missing_name_is_denied_and_comm_is_the_fallback_when_exe_is_unreadable() {
        let allow = list(&["wl-clip-persist", "very-long-program-name"]);
        assert!(!is_allowed(&allow, None, None));
        assert!(is_allowed(&allow, None, Some("wl-clip-persist")));
        // 15 バイトに切られた comm。
        assert!(is_allowed(&allow, None, Some("very-long-progr")));
        assert!(!is_allowed(&allow, None, Some("very-long-prog")));
        assert!(!is_allowed(&allow, None, Some("other")));
    }

    #[test]
    fn the_current_process_has_readable_names() {
        let pid = i32::try_from(std::process::id()).unwrap_or(-1);
        let (exe, comm) = process_names(pid);
        assert!(exe.is_some());
        assert!(comm.is_some());
        // 存在しない pid は、どちらも取れない。
        assert_eq!(process_names(-1), (None, None));
    }
}
