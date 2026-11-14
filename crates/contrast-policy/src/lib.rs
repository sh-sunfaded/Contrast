//! Contrast のポリシー層。
//!
//! - `port`: コンポジタがポリシーに問い合わせる差し込み口（トレイト）。
//! - `settings`: 設定の値 `Settings`（見た目・キーバインド・ワークスペース数）と組み込みの既定値。
//! - `hook`: ポリシーのフック `windowOpen` の口（CDL を知らない）。
//! - `config`: 設定ファイルのパスの解決、`ConfigLoader`、設定の診断 `ConfigDiagnostic`。
//! - `store`: `Config` と、世代番号を振って成功した版だけを差し替える `ConfigStore`（`Policy` として使える）。
//! - `static_policy`: 組み込みの既定値だけで答える `StaticPolicy` と、既定の配置。
//! - `core_adapter`: `port::Policy` を contrast-core の `Policy`（窓の配置）として使うアダプタ。
//! - `tier`: 注意の段階（`Tiers`）と、その検査（INV-A1）・減光の乗数。
//! - `codes`: Contrast の診断コード（`CT…`）。
//!
//! CDL の読み込みは別のクレート（`contrast-policy-cdl`）にある。このクレートは CDL を知らない。

pub mod codes;
pub mod config;
pub mod core_adapter;
pub mod hook;
pub mod port;
pub mod settings;
pub mod static_policy;
pub mod store;
pub mod tier;

pub use config::{
    ConfigDiagnostic, ConfigLoader, DiagnosticSeverity, LoadOutcome, resolve_config_path,
};
pub use core_adapter::PortPlacement;
pub use hook::{HookPlacement, PlacementHook, WindowInfo};
pub use port::{Action, Mods, PlacementWidth, Policy, WindowPlacement};
pub use settings::{
    AnimationKind, Appearance, CursorSettings, DEFAULT_DATA_CONTROL_CLIENTS,
    DEFAULT_SCREENSHOT_PATH, HotkeyOverlaySettings, InputSettings, KeyBinding, KeySpec,
    LayoutSettings, ModKey, Settings, Shadow, StartupCommand,
};
pub use static_policy::{StaticPolicy, default_placement};
pub use store::{Config, ConfigStore, ReloadOutcome, StartupStatus, load_or_builtin};
pub use tier::{Tier, TierError, TierSlot, TierSpec, Tiers, dim_factor, validate_tiers};
