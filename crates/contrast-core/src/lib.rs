//! Contrast コンポジタの中核ロジック。
//!
//! Wayland やレンダリングに依存しない、状態・レイアウト・フォーカスの純粋な実装。
//! 仕様は `docs/core-spec.md` を正とする。
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

/// エンティティと値オブジェクト。crate 内の他モジュールに依存しない。
mod domain {
    pub mod column;
    pub mod geometry;
    pub mod id;
    pub mod insets;
    pub mod output;
    pub mod state;
    pub mod window;
    pub mod workspace;
}

/// Command の解釈と Effect の生成（`Core`）。
mod application;
mod error;
/// 不変条件 I1〜I6・I9〜I11 の検査。
#[cfg(any(test, feature = "invariants"))]
mod invariant;
/// Tiled 窓の配置計算。
mod layout;
/// 新しい窓の配置の判断。
mod policy;

pub use application::command::{Action, Command, Direction};
pub use application::effect::{Applied, Effect};
pub use application::engine::{Core, CoreConfig};
pub use application::view::{StateView, WindowView};
pub use domain::column::{ColumnWidth, LayoutMode, Ratio, WindowHeight, WorkspaceMode};
pub use domain::geometry::{Point, Rect, Size};
pub use domain::id::{
    EntityId, ExternalKey, IdGen, IdKind, KeyMap, OutputId, WindowId, WorkspaceId, WorkspaceIndex,
};
pub use domain::insets::Insets;
pub use domain::output::OutputName;
pub use domain::window::{WindowMeta, WindowState};
pub use error::{CoreError, LayoutError};
#[cfg(any(test, feature = "invariants"))]
pub use invariant::{InvariantViolation, StateSnapshot};
pub use layout::{Columns, Layout};
pub use policy::{DefaultPolicy, Placement, PlacementContext, Policy};
