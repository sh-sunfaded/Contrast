//! xdg-decoration は公開しない（niri の `prefer-no-csd` が無効のときと同じ）。
//!
//! niri は `prefer-no-csd` が無効（作者の config.kdl はコメントアウト）のとき、xdg-decoration のグローバルを
//! クライアントに見せず（`niri.rs` の `can_view_decoration_globals`）、窓に Tiled 状態も送らない
//! （`utils/mod.rs` の `update_tiled_state`）。クライアントは自分で装飾（CSD）を描く。Contrast も同じにする。
//! このモジュールには、ログ用の補助だけが残る。

/// ログ用に窓の通し番号を書く（窓が無い・未管理なら `none`）。
pub fn fmt_id(id: Option<u64>) -> String {
    id.map_or_else(|| "none".to_string(), |id| id.to_string())
}
