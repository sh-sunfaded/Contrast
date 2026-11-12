//! アプリケーション層: `Core`、入力（Command / Action）、出力（Effect）、読み取り口（StateView）。
//!
//! 状態を変えるのは `Core::apply` だけで、「State を複製して handler で変え、再配置し、
//! 差分から Effect を作り、成功時だけ差し替える」順で行う（I7・I8）。

mod arrange;
pub mod command;
mod diff;
pub mod effect;
pub mod engine;
mod handlers;
pub mod scrolling;
pub mod view;
