//! Contrast の設定を CDL で読むアダプタ。`cdl-host` に触れるのはこのクレートだけ。
//!
//! 外へ出る型は `contrast-policy` の CDL 非依存の型（`Config`・`ConfigDiagnostic` など）に限る。
//! 契約（スキーマ・キー書式・フック・失敗時の既定・診断コード）は `docs/cdl-contract.md`。
//!
//! - `schema`: 組み込みコンポーネントとポリシーの型の登録。
//! - `resolver`: 設定ディレクトリに閉じた `ModuleResolver`（`FsResolver`）。
//! - `diag`: CDL の診断を `ConfigDiagnostic` に写す。
//! - `keys`: キー書式のパーサ（パニックしない純関数）。
//! - `convert`: 評価後の値木を `Settings` に変換する（誤りは全部集める）。
//! - `hook`: `windowOpen` の呼び出しと戻り値の検査（`CdlPolicy`）。
//! - `loader`: `ConfigLoader` の実装 `CdlLoader`。

mod convert;
mod diag;
mod hook;
mod keys;
mod loader;
mod resolver;
mod schema;

pub use hook::CdlPolicy;
pub use keys::{KeyError, parse_key_spec};
pub use loader::CdlLoader;
