//! VT 切り替えのあいだの描画の失敗の扱い（純粋な判断）。
//!
//! VT を離れると DRM master が外れるので、直前に出した描画の依頼（commit・page flip）が
//! `EACCES`（Permission denied）で失敗する。これは正常な流れで、致命的にしてはいけない。
//! 一方、セッションが active で VT 切り替え中でもないときの `EACCES` は、本物の不具合なので終了する。

use std::error::Error;
use std::io;

/// 失敗した描画の依頼に対する扱い。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FailureAction {
    /// 致命的: 原因つきで終了する。
    Fatal,
    /// VT 切り替えの途中の失敗: debug ログに出して、描画を止めて待つ（active に戻れば再開する）。
    Wait,
}

/// 失敗の種類と状態から、扱いを決める。
///
/// `Wait` になるのは、`EACCES` で、かつ、セッションが inactive か VT 切り替えの要求中のときだけ。
pub(crate) fn failure_action(
    permission_denied: bool,
    session_active: bool,
    vt_switching: bool,
) -> FailureAction {
    if permission_denied && (!session_active || vt_switching) {
        FailureAction::Wait
    } else {
        FailureAction::Fatal
    }
}

/// エラーが `EACCES`（`io::ErrorKind::PermissionDenied`）を原因に持つか。
/// `source()` の連鎖をたどり、`io::Error` を探す。Smithay のエラーの文字列に含まれる `(os error 13)` も見る。
pub(crate) fn is_permission_denied(error: &(dyn Error + 'static)) -> bool {
    let mut current: Option<&(dyn Error + 'static)> = Some(error);
    while let Some(e) = current {
        if let Some(io_error) = e.downcast_ref::<io::Error>()
            && io_error.kind() == io::ErrorKind::PermissionDenied
        {
            return true;
        }
        current = e.source();
    }
    error.to_string().contains("(os error 13)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt;

    /// `source` を持つだけのエラー（Smithay の `AccessError` の形）。
    #[derive(Debug)]
    struct Wrapper(Box<dyn Error + 'static>);

    impl fmt::Display for Wrapper {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "Page flip commit failed")
        }
    }

    impl Error for Wrapper {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(self.0.as_ref())
        }
    }

    #[test]
    fn eacces_while_the_session_is_inactive_waits() {
        assert_eq!(failure_action(true, false, false), FailureAction::Wait);
    }

    #[test]
    fn eacces_while_a_vt_switch_is_requested_waits() {
        assert_eq!(failure_action(true, true, true), FailureAction::Wait);
        assert_eq!(failure_action(true, false, true), FailureAction::Wait);
    }

    #[test]
    fn eacces_while_active_and_not_switching_is_fatal() {
        assert_eq!(failure_action(true, true, false), FailureAction::Fatal);
    }

    #[test]
    fn other_errors_are_always_fatal() {
        for (active, switching) in [(true, false), (false, false), (true, true), (false, true)] {
            assert_eq!(
                failure_action(false, active, switching),
                FailureAction::Fatal,
                "active={active} switching={switching}"
            );
        }
    }

    #[test]
    fn permission_denied_is_found_through_the_source_chain() {
        let denied = io::Error::from_raw_os_error(13);
        assert!(is_permission_denied(&denied));
        let wrapped = Wrapper(Box::new(Wrapper(Box::new(io::Error::from(
            io::ErrorKind::PermissionDenied,
        )))));
        assert!(is_permission_denied(&wrapped));
    }

    #[test]
    fn other_io_errors_are_not_permission_denied() {
        assert!(!is_permission_denied(&io::Error::from_raw_os_error(16)));
        let wrapped = Wrapper(Box::new(io::Error::from(io::ErrorKind::NotFound)));
        assert!(!is_permission_denied(&wrapped));
    }

    #[test]
    fn a_message_with_os_error_13_counts_when_the_source_is_lost() {
        let flat = io::Error::other("Page flip commit failed (Permission denied (os error 13))");
        assert!(is_permission_denied(&flat));
    }
}
