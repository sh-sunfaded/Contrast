//! niri 互換の IPC（ironbar の workspaces 用の最小実装）。計画は `docs/ipc-plan.md`。
//!
//! - `protocol`: JSON（純粋）。
//! - `state`: `Contrast` の状態から作るワークスペースの一覧と、その差分（純粋）。
//! - `server`: calloop の Unix ソケットのサーバー。

mod protocol;
mod state;
