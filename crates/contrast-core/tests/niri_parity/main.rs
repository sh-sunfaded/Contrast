//! niri との動作を合わせるための core の新しい仕様テスト（`docs/core-spec.md` の 15 節以降）。
//!
//! `tests/spec/`（人間が書く仕様。編集しない）の外に置く。実装より先に書いたもので、実装のために変更しない。
//! 公開 API だけを使う。`check_invariants` を使うので、`invariants` feature（`--all-features`）が前提。

mod common;
mod consume_or_expel;
