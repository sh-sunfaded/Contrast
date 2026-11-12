//! 仕様テストの入口。人間だけが編集する。フェーズごとに mod を足していく。
//! 実行: cargo test -p contrast-core --test spec [フィルタ]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::missing_docs_in_private_items,
    missing_docs,
    reason = "仕様テストは失敗＝パニックでよい。本体の lint は緩めない"
)]
#[cfg(feature = "invariants")]
mod spec_core_scrolling;
mod spec_geometry;
mod spec_id;

// ---- core の仕様テスト（P3）。check_invariants と snapshot が要るので invariants feature が前提 ----
#[cfg(feature = "invariants")]
mod common;
#[cfg(feature = "invariants")]
mod spec_core_actions;
#[cfg(feature = "invariants")]
mod spec_core_lifecycle;
#[cfg(feature = "invariants")]
mod spec_core_properties;

/// feature の付け忘れで、core の仕様テストが黙って 0 件になるのを防ぐ。
#[cfg(not(feature = "invariants"))]
#[test]
#[allow(
    clippy::assertions_on_constants,
    reason = "feature の付け忘れを、テストの失敗として知らせる"
)]
fn core_specs_need_the_invariants_feature() {
    assert!(
        cfg!(feature = "invariants"),
        "core の仕様テストは --all-features（または --features invariants）で実行する"
    );
}
#[cfg(feature = "invariants")]
mod spec_core_insets;
#[cfg(feature = "invariants")]
mod spec_core_scrolling_heights;
mod spec_props;
