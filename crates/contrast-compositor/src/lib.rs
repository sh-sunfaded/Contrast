//! Contrast の Smithay シェル（nested プロトタイプ）。
//!
//! ホストの Wayland 上に winit の窓を開き、その中でクライアントを受け付ける。
//! 配置・キーバインド・背景色は `contrast_policy::Policy` に問い合わせる。

mod anim;
mod backend;
mod cursor;
mod data_control;
mod geometry;
mod handler;
mod ime;
mod input;
mod ipc;
mod reload;
mod render;
mod shell;
mod spawn;
mod state;

use std::path::PathBuf;
use std::rc::Rc;

use calloop::signals::{Signal, Signals};
use smithay::reexports::{calloop::EventLoop, wayland_server::Display};

use contrast_policy::{ConfigLoader, ConfigStore};
pub use state::Contrast;

/// 環境変数から使うバックエンドを決める。
fn detect_backend() -> Result<backend::Kind, backend::SelectError> {
    let choice = std::env::var("CONTRAST_BACKEND").ok();
    let has_host_display = ["WAYLAND_DISPLAY", "DISPLAY"]
        .iter()
        .any(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty()));
    backend::select(choice.as_deref(), has_host_display)
}

/// 設定に `modKey` が無いときの `mod`。`CONTRAST_MOD_KEY`（`alt` / `super`）> バックエンドの既定
/// （DRM は super、winit は alt）。バックエンドを決められないときは winit の既定（`run` がエラーにする）。
/// 不正な環境変数は警告して無視する。CDL の `modKey` の明示は、これより優先される。
pub fn startup_mod_key() -> contrast_policy::ModKey {
    let kind = detect_backend().unwrap_or(backend::Kind::Winit);
    let env = std::env::var("CONTRAST_MOD_KEY").ok();
    let (mod_key, invalid) = backend::resolve_mod_key(env.as_deref(), kind);
    if let Some(value) = invalid {
        tracing::warn!("CONTRAST_MOD_KEY={value} は alt か super ではないので無視する");
    }
    mod_key
}

/// 設定の再読み込みに使う読み込み器と設定ファイルのパス。
pub struct ConfigSource {
    /// 設定ファイルを読む読み込み器。
    pub loader: Box<dyn ConfigLoader>,
    /// 設定ファイルのパス。決められなかったときは `None`（リロードは何もしない）。
    pub path: Option<PathBuf>,
}

/// コンポジタを起動し、終了まで戻らない。`store` が配置・キーバインド・見た目の問い合わせ先。
///
/// SIGUSR1 と `reload-config` アクションで、`source` から設定を読み直して `store` を差し替える。
///
/// バックエンドは環境変数 `CONTRAST_BACKEND=winit|drm` で選ぶ。未指定で `WAYLAND_DISPLAY` か `DISPLAY` が
/// あれば winit、無ければエラー。`drm` のときは実機の出力に描き、libinput で入力を受ける
/// （`CONTRAST_EXIT_AFTER` 秒、既定 30 で自動終了する。0 なら無効）。
pub fn run(store: Rc<ConfigStore>, source: ConfigSource) -> Result<(), Box<dyn std::error::Error>> {
    let kind = detect_backend()?;

    let mut event_loop: EventLoop<Contrast> = EventLoop::try_new()?;
    // SIGUSR1 はスレッドを作る前にブロックして calloop で受ける（既定の動作は終了）。
    let signals = Signals::new(&[Signal::SIGUSR1])?;
    event_loop
        .handle()
        .insert_source(signals, |_, _, state| state.reload_config())?;
    let display: Display<Contrast> = Display::new()?;
    let mut state = Contrast::new(&mut event_loop, display, store, source);

    // DRM の資源は `drm_guard` の Drop（と、ループの後の `finish`）で片付ける。パニックでも通る。
    let mut drm_guard = None;
    match kind {
        backend::Kind::Winit => backend::winit::init_winit(&mut event_loop, &mut state)?,
        backend::Kind::Drm => {
            drm_guard = Some(backend::drm::init(&mut event_loop, &mut state)?);
            tracing::info!("wayland: socket={}", state.socket_name.to_string_lossy());
        }
    }
    input::debug::init(&mut event_loop);

    tracing::info!(
        socket = ?state.socket_name,
        "listening: WAYLAND_DISPLAY={}",
        state.socket_name.to_string_lossy()
    );

    // `Startup` の `ipc="niri"` の子に渡すので、コマンドの起動より前に IPC のソケットを作る。
    state.start_ipc(&event_loop.handle());

    // Wayland のソケットを公開したので、`Startup` のコマンドを起動する（起動時に一度だけ。リロードでは再実行しない）。
    state.run_startup();

    let result = event_loop.run(None, &mut state, |_| {});
    tracing::info!("event loop stopped, exiting");
    if let Some(guard) = drm_guard.as_mut() {
        guard.finish()?;
    }
    result?;
    Ok(())
}
