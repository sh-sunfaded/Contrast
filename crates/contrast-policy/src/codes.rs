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
/// リロードで反映できない変更。
pub const NOT_APPLIED: &str = "CT050";
