//! niri との動作を合わせるための core の新しい仕様テスト（`docs/core-spec.md` の 15 節以降）。
//!
//! `tests/spec/`（人間が書く仕様。編集しない）の外に置く。実装より先に書いたもので、実装のために変更しない。
//! 公開 API だけを使う。`check_invariants` と `snapshot` を使うので、`invariants` feature（`--all-features`）が前提。
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::missing_docs_in_private_items,
    missing_docs,
    reason = "仕様テストは失敗＝パニックでよい。本体の lint は緩めない"
)]

/// feature の付け忘れで、テストが黙って 0 件になるのを防ぐ。
#[cfg(not(feature = "invariants"))]
#[test]
#[allow(
    clippy::assertions_on_constants,
    reason = "feature の付け忘れを、テストの失敗として知らせる"
)]
fn niri_parity_tests_need_the_invariants_feature() {
    assert!(
        cfg!(feature = "invariants"),
        "core の niri_parity テストは --all-features（または --features invariants）で実行する"
    );
}

#[cfg(feature = "invariants")]
mod common;
#[cfg(feature = "invariants")]
mod consume_or_expel;
#[cfg(feature = "invariants")]
mod dynamic_workspaces;
#[cfg(feature = "invariants")]
mod multi_monitor;
#[cfg(feature = "invariants")]
mod view_gesture;
#[cfg(feature = "invariants")]
mod move_column_no_focus;
