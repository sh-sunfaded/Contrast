//! Contrast の診断コード（名前空間 `CT`。CDL の `E` とは別）。意味は `docs/cdl-contract.md` の表と同じ。

/// 設定ファイルを読めない。
pub const FILE_UNREADABLE: &str = "CT001";
/// 値木が期待と食い違う。
pub const TREE_MISMATCH: &str = "CT002";
/// prop の値が不正。
pub const BAD_PROP: &str = "CT003";
/// `Workspaces.count` が範囲外。
pub const BAD_WORKSPACE_COUNT: &str = "CT010";
/// 未知の action。
pub const UNKNOWN_ACTION: &str = "CT020";
/// `command` が要る action に無い。
pub const MISSING_COMMAND: &str = "CT021";
/// `workspace` が要る action に無い・不正。
pub const BAD_ACTION_WORKSPACE: &str = "CT022";
/// キー書式の誤り。
pub const BAD_KEY: &str = "CT030";
/// 同じキーの重複。
pub const DUPLICATE_KEY: &str = "CT031";
/// フックの評価エラー・上限超過。
pub const HOOK_FAILED: &str = "CT040";
/// フックの戻り値の形が不正。
pub const HOOK_BAD_SHAPE: &str = "CT041";
/// フックの戻り値の `workspace` が不正。
pub const HOOK_BAD_WORKSPACE: &str = "CT042";
/// フックの戻り値の `workspace` が数を超える。
pub const HOOK_WORKSPACE_RANGE: &str = "CT043";
/// `FocusTier` / `Tier` の `l` が有限の数でない、または 0 以下か 1 超。
pub const TIER_BAD_L: &str = "CT060";
/// `Tiers` に `FocusTier` が無い。
pub const TIER_MISSING_FOCUS: &str = "CT061";
/// 段階が単調でない（`F > n_1 > …` を満たさない）。
pub const TIER_NOT_MONOTONIC: &str = "CT062";
/// フォーカス段階との差が、フォーカス外の段階全体の幅以下（DU-12）。
pub const TIER_SPREAD: &str = "CT063";
/// `Tier` の個数が多すぎる（この版では 0 個または 1 個。DU-20）。
pub const TIER_COUNT: &str = "CT064";
/// `Tier` の `name` が空か、重複している。
pub const TIER_BAD_NAME: &str = "CT065";
/// リロードで反映できない変更。
pub const NOT_APPLIED: &str = "CT050";
