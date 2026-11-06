//! DRM バックエンド（フェーズ3・4）: 実機の出力に Contrast の描画（窓・枠・レイヤー面・カーソル）を載せ、
//! libinput でキーボードとポインタを扱う。Wayland サーバーや core・policy は `Contrast` がそのまま持ち、
//! 同じ calloop のループで動く。
//!
//! 安全策:
//! - `CONTRAST_EXIT_AFTER`（秒、既定 30）で必ず自動終了する。`0` と明示したときだけ、タイマーを無効にする。
//! - SIGINT / SIGTERM はイベントループで受けて、ループを抜けて通常の終了処理に入る。Alt+Shift+Q も同じ道を通る。
//! - 終了処理は `Drop`（`DrmGuard` と `Gpu`）に置く。正常終了・シグナル・パニック（unwind）のどれでも通る。
//!   出力を止め（surface の Drop が CRTC を無効化）、DRM master を手放し、fd を閉じるとカーネルが fbcon を戻す。
//! - 最初の page flip が 2 秒以内に終わらなければ、エラーで終了する。
//! - session の inactive（VT 切り替え）は必ずイベントループで処理する（描画と入力を止めて master を手放す）。
//!   active に戻れば再開する。libseat への応答（disable の ack）は `LibSeatSessionNotifier` が行う。
//! - VT 切り替えを要求した時点で `vt_switching` の印を立て、新しいフレームを出さない（pause か resume か 2 秒で解除）。
//!   master が外れたあとの描画の依頼は `EACCES` で失敗するが、inactive か `vt_switching` のあいだなら致命的にしない
//!   （`vt_guard::failure_action`）。active に戻って最初のフレームが 2 秒以内に終わらなければ、エラーで終了する。
//! - modeset の失敗は原因つきのエラー（終了コード 1）にする。
//!
//! dmabuf はクライアントのバッファを描画してから flip する（direct scanout はしない）。
//! 未対応: ホットプラグ、複数出力、スケール、クライアントのカーソル画像。

use std::cell::RefCell;
use std::fmt;
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use smithay::backend::{
    allocator::{
        Fourcc,
        gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
    },
    drm::{
        DrmDevice, DrmDeviceFd, DrmDeviceNotifier, DrmEvent,
        compositor::FrameFlags,
        exporter::gbm::GbmFramebufferExporter,
        output::{DrmOutput, DrmOutputManager, DrmOutputRenderElements},
    },
    egl::{EGLContext, EGLDisplay},
    input::InputEvent,
    libinput::{LibinputInputBackend, LibinputSessionInterface},
    renderer::{ImportDma, element::solid::SolidColorBuffer, gles::GlesRenderer},
    session::{Event as SessionEvent, Session, libseat::LibSeatSession},
    udev::{UdevBackend, primary_gpu},
};
use smithay::desktop::utils::send_frames_surface_tree;
use smithay::input::pointer::{CursorImageStatus, MotionEvent};
use smithay::output::{Mode as OutputMode, Output, PhysicalProperties, Subpixel};
use smithay::reexports::{
    calloop::{
        EventLoop, LoopHandle,
        signals::{Signal, Signals},
        timer::{TimeoutAction, Timer},
    },
    drm::{
        self,
        control::{self, Device as ControlDevice, connector::State, crtc},
    },
    input::Libinput,
    rustix::fs::OFlags,
};
use smithay::utils::{DeviceFd, Logical, Point, SERIAL_COUNTER, Transform};

use contrast_core::Rect;
use contrast_policy::CursorSettings;

use super::vt_guard::{FailureAction, failure_action, is_permission_denied};
use crate::cursor::{CursorTheme, cursor_elements};
use crate::render::{SceneElement, finish_frame, scene_elements};
use crate::state::Contrast;

/// 最初の page flip を待つ上限。
const FIRST_FLIP_TIMEOUT: Duration = Duration::from_secs(2);
/// VT 切り替えを要求してから、`vt_switching` の印を（pause も resume も来なくても）解除するまでの時間。
const VT_SWITCH_GRACE: Duration = Duration::from_secs(2);
/// session が active に戻ってから、最初のフレームが終わるまでの上限。
const RESUME_FLIP_TIMEOUT: Duration = Duration::from_secs(2);
/// `CONTRAST_EXIT_AFTER` の既定値（秒）。
const DEFAULT_EXIT_AFTER_SECS: u64 = 30;
/// 空のフレームが返ったときにやり直すまでの間隔（約 1 リフレッシュ）。
const RETRY_INTERVAL: Duration = Duration::from_millis(16);

/// DRM バックエンドの起動・列挙・表示の失敗。
#[derive(Debug)]
pub enum DrmError {
    /// libseat の session を開けなかった（seat が使えない、logind / seatd に繋がらない、など）。
    Session(String),
    /// udev で seat のデバイスを列挙できなかった。
    Udev(String),
    /// イベントループの作成・登録に失敗した。
    EventLoop(String),
    /// seat に DRM デバイスはあったが、どれも開いて読めなかった（session が active でない、など）。
    NoReadableDevice {
        /// session が active だったか。
        active: bool,
    },
    /// 環境変数の値が不正。
    Config(String),
    /// 接続済みで使える出力（connector と CRTC）が無い。
    NoOutput,
    /// GBM / EGL / renderer の初期化に失敗した。
    Gpu(String),
    /// modeset（最初の commit）に失敗した。画面は変えていない。
    Modeset(String),
    /// 描画または page flip に失敗した。
    Frame(String),
    /// 最初の page flip が時間内に終わらなかった。
    FlipTimeout,
    /// libinput の初期化に失敗した。
    Input(String),
}

impl fmt::Display for DrmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DrmError::Session(e) => write!(
                f,
                "DRM バックエンド: seat を開けません（{e}）。\
                 グラフィカルなセッションの外の TTY（Ctrl+Alt+F3 など）で、ログインしたユーザーとして実行してください。\
                 別のコンポジタが動いている VT や ssh からは seat を取れません"
            ),
            DrmError::Udev(e) => write!(
                f,
                "DRM バックエンド: udev でデバイスを列挙できません（{e}）"
            ),
            DrmError::NoReadableDevice { active } => write!(
                f,
                "DRM バックエンド: DRM デバイスを開けませんでした（session active={active}）。\
                 このセッションが seat の有効なセッションではない可能性があります。\
                 グラフィカルなセッションの外の TTY（Ctrl+Alt+F3 など）にログインして実行してください"
            ),
            DrmError::EventLoop(e) => {
                write!(
                    f,
                    "DRM バックエンド: イベントループの初期化に失敗しました（{e}）"
                )
            }
            DrmError::Config(e) => write!(f, "DRM バックエンド: 設定が不正です（{e}）"),
            DrmError::NoOutput => write!(
                f,
                "DRM バックエンド: 接続済みの出力（connector と CRTC）が見つかりません"
            ),
            DrmError::Gpu(e) => write!(
                f,
                "DRM バックエンド: GBM / EGL の初期化に失敗しました（{e}）"
            ),
            DrmError::Modeset(e) => write!(
                f,
                "DRM バックエンド: modeset に失敗しました（{e}）。画面は変更していません"
            ),
            DrmError::Frame(e) => write!(
                f,
                "DRM バックエンド: 描画または page flip に失敗しました（{e}）"
            ),
            DrmError::FlipTimeout => write!(
                f,
                "DRM バックエンド: 最初の page flip が {} 秒以内に完了しませんでした",
                FIRST_FLIP_TIMEOUT.as_secs()
            ),
            DrmError::Input(e) => write!(
                f,
                "DRM バックエンド: libinput の初期化に失敗しました（{e}）"
            ),
        }
    }
}

impl std::error::Error for DrmError {}

/// session 経由で開いた DRM デバイス（列挙用）。master は取らず、読み取り専用の ioctl だけに使う。
struct Card(OwnedFd);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl drm::Device for Card {}
impl ControlDevice for Card {}

/// modeset の対象: デバイス、connector、CRTC、モード。
struct Target {
    path: PathBuf,
    connector: control::connector::Handle,
    name: String,
    crtc: crtc::Handle,
    mode: control::Mode,
    size_mm: (i32, i32),
}

type Allocator = GbmAllocator<DrmDeviceFd>;
type Exporter = GbmFramebufferExporter<DrmDeviceFd>;
type Manager = DrmOutputManager<Allocator, Exporter, (), DrmDeviceFd>;
type OutputHandle = DrmOutput<Allocator, Exporter, (), DrmDeviceFd>;

/// GPU 側の資源。`Drop` で、出力を止める → DRM master を手放す → renderer と fd を閉じる、の順に片付ける。
struct Gpu {
    output: Option<OutputHandle>,
    manager: Option<Manager>,
    renderer: Option<GlesRenderer>,
}

impl Drop for Gpu {
    fn drop(&mut self) {
        // 出力（surface の Drop が CRTC を無効化する。master が要る）→ master を手放す → renderer・fd を閉じる。
        self.output = None;
        if let Some(manager) = self.manager.as_ref()
            && let Err(e) = drm::Device::release_master_lock(manager.device())
        {
            tracing::debug!("drm: release master: {e}");
        }
        self.manager = None;
        self.renderer = None;
        tracing::info!("drm: released master");
    }
}

/// DRM バックエンドの状態。イベントループのコールバックが `Rc<RefCell<_>>` 越しに触る。
struct Drm {
    // `gpu` を `_session` より先に落とす（フィールドは宣言順に Drop される）。
    gpu: Option<Gpu>,
    _session: LibSeatSession,
    output: Output,
    /// テーマを作ったときの設定（リロードで変わったら作り直す）。
    cursor_cfg: CursorSettings,
    /// xcursor のテーマ。
    theme: CursorTheme,
    /// テーマが使えないときの四角。
    fallback_cursor: SolidColorBuffer,
    libinput: Libinput,
    handle: LoopHandle<'static, Contrast>,
    active: bool,
    /// VT 切り替えを要求した時刻（`VT_SWITCH_GRACE` のあいだ、新しいフレームを出さない）。
    vt_switching_since: Option<Instant>,
    /// 描画の依頼を見送った（`vt_switching` か、`EACCES` を待った）。再開のときに描き直す。
    stalled: bool,
    frames: u64,
    last_log: Instant,
    error: Option<DrmError>,
}

type Shared = Rc<RefCell<Drm>>;

/// 共有状態を借りて `f` を呼ぶ。すでに借りていれば（再入）何もしない。
fn with(drm: &Shared, f: impl FnOnce(&mut Drm)) {
    if let Ok(mut d) = drm.try_borrow_mut() {
        f(&mut d);
    }
}

impl Drm {
    /// ループを止めて、`error` を持ち帰る。
    fn fail(&mut self, state: &mut Contrast, error: DrmError) {
        tracing::error!("{error}");
        if self.error.is_none() {
            self.error = Some(error);
        }
        state.loop_signal.stop();
    }

    /// VT 切り替えの要求から `VT_SWITCH_GRACE` 以内か。
    fn vt_switching(&self) -> bool {
        self.vt_switching_since
            .is_some_and(|since| since.elapsed() < VT_SWITCH_GRACE)
    }

    /// 描画の依頼の失敗を、致命的にするか、待つか決める（`vt_guard::failure_action`）。
    fn handle_failure(&mut self, state: &mut Contrast, error: DrmError, permission_denied: bool) {
        match failure_action(permission_denied, self.active, self.vt_switching()) {
            FailureAction::Fatal => self.fail(state, error),
            FailureAction::Wait => {
                tracing::debug!(
                    active = self.active,
                    vt_switching = self.vt_switching(),
                    "drm: waiting after a failure during a vt switch: {error}"
                );
                self.stalled = true;
            }
        }
    }

    /// 要求した VT 切り替えの猶予が切れた（pause も resume も来なかった）: 印を外し、見送った描画を再開する。
    fn vt_switch_expired(&mut self, state: &mut Contrast, shared: &Shared) {
        if self.vt_switching_since.is_none() || self.vt_switching() {
            return;
        }
        tracing::warn!("drm: no session event after the vt switch request, resuming rendering");
        self.vt_switching_since = None;
        if self.stalled && self.active {
            self.stalled = false;
            self.render(state, shared);
        }
    }

    /// 次のフレームを描いて queue し、成功したら frame callback を送る。
    /// クライアントの commit を拾うため、毎回バッファの age を捨てて全面を描き直す（プロトタイプ）。
    fn render(&mut self, state: &mut Contrast, shared: &Shared) {
        if !self.active {
            return;
        }
        if self.vt_switching() {
            self.stalled = true;
            return;
        }
        state.tick_animations();
        let first = self.frames == 0;
        let background = state.policy.background();
        let pointer_pos = state
            .seat
            .get_pointer()
            .map(|p| p.current_location())
            .unwrap_or_default();
        let cursor_status = state.cursor_status.clone();
        let cursor_cfg = state.policy.cursor_settings();
        if cursor_cfg != self.cursor_cfg {
            self.theme = CursorTheme::resolve(&cursor_cfg);
            self.cursor_cfg = cursor_cfg;
        }
        let output_name = self.output.clone();
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        let (Some(output), Some(renderer)) = (gpu.output.as_mut(), gpu.renderer.as_mut()) else {
            return;
        };
        let cursor = if state.cursor_hidden() {
            Vec::new()
        } else {
            cursor_elements(
                renderer,
                &mut self.theme,
                &mut self.fallback_cursor,
                &cursor_status,
                pointer_pos,
            )
        };
        let elements = match scene_elements(state, renderer, &output_name, cursor) {
            Ok(e) => e,
            Err(e) => {
                self.fail(state, DrmError::Frame(e.to_string()));
                return;
            }
        };
        output.with_compositor(|c| c.reset_buffer_ages());
        let rendered = output
            .render_frame::<_, SceneElement>(renderer, &elements, background, FrameFlags::DEFAULT)
            .map(|result| result.is_empty)
            .map_err(|e| (DrmError::Frame(e.to_string()), is_permission_denied(&e)));
        let result = match rendered {
            Ok(true) => Err(None),
            Ok(false) => output.queue_frame(()).map_err(|e| {
                // 最初の queue_frame が atomic commit（modeset）を行う。
                let error = if first {
                    DrmError::Modeset(e.to_string())
                } else {
                    DrmError::Frame(e.to_string())
                };
                Some((error, is_permission_denied(&e)))
            }),
            Err(e) => Err(Some(e)),
        };
        match result {
            Ok(()) => {
                finish_frame(state, &output_name);
                if let CursorImageStatus::Surface(surface) = &cursor_status {
                    send_frames_surface_tree(
                        surface,
                        &output_name,
                        state.start_time.elapsed(),
                        Some(Duration::ZERO),
                        |_, _| Some(output_name.clone()),
                    );
                }
            }
            Err(Some((error, denied))) => self.handle_failure(state, error, denied),
            Err(None) => {
                // 空のフレーム: 少し待ってやり直す。
                let shared = shared.clone();
                let scheduled = self.handle.insert_source(
                    Timer::from_duration(RETRY_INTERVAL),
                    move |_, _, state| {
                        with(&shared.clone(), |d| d.render(state, &shared));
                        TimeoutAction::Drop
                    },
                );
                if let Err(e) = scheduled {
                    self.fail(state, DrmError::EventLoop(e.to_string()));
                }
            }
        }
    }

    /// vblank: 提出済みのフレームを確定して、次を出す。
    fn on_vblank(&mut self, state: &mut Contrast, shared: &Shared) {
        let Some(output) = self.gpu.as_mut().and_then(|g| g.output.as_mut()) else {
            return;
        };
        match output.frame_submitted() {
            Ok(Some(())) => {}
            Ok(None) => return,
            Err(e) => {
                self.handle_failure(
                    state,
                    DrmError::Frame(e.to_string()),
                    is_permission_denied(&e),
                );
                return;
            }
        }
        self.frames = self.frames.saturating_add(1);
        if self.last_log.elapsed() >= Duration::from_secs(1) {
            self.last_log = Instant::now();
            tracing::info!("drm: frame={}", self.frames);
        }
        self.render(state, shared);
    }

    /// session が inactive になった（VT 切り替え）: 描画と入力を止めて master を手放す。
    fn pause(&mut self, state: &mut Contrast) {
        tracing::info!("drm: session inactive");
        self.active = false;
        self.vt_switching_since = None;
        self.stalled = false;
        self.libinput.suspend();
        // デバイスが外れると、離したキーのイベントが来ない（押しっぱなしの修飾キーが残る）ので、ここで離す。
        state.release_all_keys();
        if let Some(manager) = self.gpu.as_mut().and_then(|g| g.manager.as_mut()) {
            manager.pause();
        }
    }

    /// session が active に戻った: master を取り直して入力と描画を再開する。
    fn resume(&mut self, state: &mut Contrast, shared: &Shared) {
        tracing::info!("drm: session active");
        self.active = true;
        self.vt_switching_since = None;
        self.stalled = false;
        if self.libinput.resume().is_err() {
            tracing::warn!("drm: libinput resume failed");
        }
        if let Some(manager) = self.gpu.as_mut().and_then(|g| g.manager.as_mut())
            && let Err(e) = manager.activate(false)
        {
            self.fail(state, DrmError::Frame(format!("activate failed: {e}")));
            return;
        }
        // 戻ってから最初のフレームが 2 秒以内に終わらなければ諦める（最初の page flip の見張りと同じ考え方）。
        let frames = self.frames;
        let watched = shared.clone();
        let scheduled = self.handle.insert_source(
            Timer::from_duration(RESUME_FLIP_TIMEOUT),
            move |_, _, state| {
                with(&watched.clone(), |d| {
                    if d.active && !d.vt_switching() && d.error.is_none() && d.frames == frames {
                        d.fail(state, DrmError::FlipTimeout);
                    }
                });
                TimeoutAction::Drop
            },
        );
        if let Err(e) = scheduled {
            self.fail(state, DrmError::EventLoop(e.to_string()));
            return;
        }
        self.render(state, shared);
    }
}

/// `init` が返す、終了処理の持ち主。Drop（パニックを含む）か `finish` で GPU 資源を片付ける。
pub struct DrmGuard {
    drm: Shared,
}

impl DrmGuard {
    /// イベントループのあとの後始末（出力を止めて master を手放す）。実行中に起きたエラーがあれば返す。
    pub fn finish(&mut self) -> Result<(), DrmError> {
        let mut drm = self
            .drm
            .try_borrow_mut()
            .map_err(|_| DrmError::Frame("DRM state is still borrowed".to_string()))?;
        drm.gpu = None;
        match drm.error.take() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

impl Drop for DrmGuard {
    fn drop(&mut self) {
        if let Ok(mut drm) = self.drm.try_borrow_mut() {
            drm.gpu = None;
        }
    }
}

/// `CONTRAST_EXIT_AFTER`（秒）を読む。未指定は既定の 30 秒。`0` と明示したときだけ `None`（タイマー無し）。
fn exit_after() -> Result<Option<Duration>, DrmError> {
    match std::env::var("CONTRAST_EXIT_AFTER") {
        Err(_) => Ok(Some(Duration::from_secs(DEFAULT_EXIT_AFTER_SECS))),
        Ok(v) => match v.trim().parse::<u64>() {
            Ok(0) => Ok(None),
            Ok(secs) => Ok(Some(Duration::from_secs(secs))),
            Err(_) => Err(DrmError::Config(format!(
                "CONTRAST_EXIT_AFTER={v} は秒数（整数）ではありません"
            ))),
        },
    }
}

/// DRM の出力・描画・入力をイベントループに登録する。最初のフレームを出した状態で戻る。
pub fn init(
    event_loop: &mut EventLoop<'static, Contrast>,
    state: &mut Contrast,
) -> Result<DrmGuard, DrmError> {
    let exit_after = exit_after()?;

    let (mut session, session_notifier) =
        LibSeatSession::new().map_err(|e| DrmError::Session(e.to_string()))?;
    let seat = session.seat();
    tracing::info!(seat = %seat, active = session.is_active(), "drm: session opened");

    let target = enumerate(&mut session, &seat)?;
    tracing::info!(
        "drm: modeset connector={} mode={}",
        target.name,
        format_mode(&target.mode)
    );
    let output = create_output(state, &target)?;
    let (gpu, drm_notifier) = open_gpu(&mut session, &target, &output)?;

    // libinput: session 経由でデバイスを開き、この seat に割り当てる。
    let mut libinput =
        Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(session.clone().into());
    libinput
        .udev_assign_seat(&seat)
        .map_err(|()| DrmError::Input(format!("udev_assign_seat({seat}) failed")))?;
    let input_backend = LibinputInputBackend::new(libinput.clone());

    let handle = event_loop.handle();
    let drm: Shared = Rc::new(RefCell::new(Drm {
        gpu: Some(gpu),
        _session: session.clone(),
        output: output.clone(),
        cursor_cfg: state.policy.cursor_settings(),
        theme: CursorTheme::resolve(&state.policy.cursor_settings()),
        fallback_cursor: SolidColorBuffer::default(),
        libinput,
        handle: handle.clone(),
        active: session.is_active(),
        vt_switching_since: None,
        stalled: false,
        frames: 0,
        last_log: Instant::now(),
        error: None,
    }));
    // ここから先の失敗は、guard の Drop が GPU 資源を片付ける。
    let guard = DrmGuard { drm: drm.clone() };

    // linux-dmabuf: レンダラーが受け付けるフォーマットで公開し、インポートはレンダラーで試す。
    let (formats, feedback): (Vec<_>, _) = drm
        .try_borrow()
        .ok()
        .and_then(|d| {
            let r = d.gpu.as_ref()?.renderer.as_ref()?;
            Some((
                r.dmabuf_formats().iter().copied().collect(),
                crate::render::dmabuf_feedback(r),
            ))
        })
        .unwrap_or_default();
    let importer = drm.clone();
    state.enable_dmabuf(
        formats,
        feedback,
        Box::new(move |dmabuf| {
            let Ok(mut d) = importer.try_borrow_mut() else {
                return false;
            };
            match d.gpu.as_mut().and_then(|g| g.renderer.as_mut()) {
                Some(r) => r.import_dmabuf(dmabuf, None).is_ok(),
                None => false,
            }
        }),
    );

    // VT 切り替え: キー処理が `pending_vt` を立てると、この関数が呼ばれる。
    // 要求した時点で印を立てて新しいフレームを止める（master が外れたあとの commit は EACCES になる）。
    // pause も resume も来なければ、`VT_SWITCH_GRACE` で印を外して描画を再開する。
    let mut vt_session = session;
    let vt_shared = drm.clone();
    let vt_handle = handle.clone();
    state.vt_switch = Some(Box::new(move |vt| {
        tracing::info!(vt, "drm: switching vt");
        with(&vt_shared.clone(), |d| {
            d.vt_switching_since = Some(Instant::now());
        });
        let expired = vt_shared.clone();
        let scheduled =
            vt_handle.insert_source(Timer::from_duration(VT_SWITCH_GRACE), move |_, _, state| {
                with(&expired.clone(), |d| d.vt_switch_expired(state, &expired));
                TimeoutAction::Drop
            });
        if let Err(e) = scheduled {
            tracing::warn!("drm: cannot schedule the vt switch timeout: {e}");
        }
        if let Err(e) = vt_session.change_vt(vt) {
            tracing::warn!(vt, "drm: change_vt failed: {e}");
        }
    }));

    {
        let shared = drm.clone();
        handle
            .insert_source(session_notifier, move |event, _, state| match event {
                SessionEvent::ActivateSession => {
                    with(&shared.clone(), |d| d.resume(state, &shared))
                }
                SessionEvent::PauseSession => with(&shared.clone(), |d| d.pause(state)),
            })
            .map_err(|e| DrmError::EventLoop(e.to_string()))?;
    }
    {
        let shared = drm.clone();
        handle
            .insert_source(drm_notifier, move |event, _, state| match event {
                DrmEvent::VBlank(_) => with(&shared.clone(), |d| d.on_vblank(state, &shared)),
                DrmEvent::Error(e) => with(&shared.clone(), |d| {
                    d.handle_failure(
                        state,
                        DrmError::Frame(e.to_string()),
                        is_permission_denied(&e),
                    )
                }),
            })
            .map_err(|e| DrmError::EventLoop(e.to_string()))?;
    }
    handle
        .insert_source(input_backend, |event, _, state| match event {
            InputEvent::DeviceAdded { device } => state.input_device_added(device),
            InputEvent::DeviceRemoved { device } => state.input_device_removed(&device),
            event => state.process_input_event(event),
        })
        .map_err(|e| DrmError::EventLoop(e.to_string()))?;
    let signals = Signals::new(&[Signal::SIGINT, Signal::SIGTERM])
        .map_err(|e| DrmError::EventLoop(e.to_string()))?;
    handle
        .insert_source(signals, |event, _, state| {
            tracing::info!("drm: signal {:?}, exiting", event.signal());
            state.loop_signal.stop();
        })
        .map_err(|e| DrmError::EventLoop(e.to_string()))?;

    // 終了タイマー（`0` の明示で無効）と、最初の page flip の見張り。
    match exit_after {
        Some(after) => {
            handle
                .insert_source(Timer::from_duration(after), move |_, _, state| {
                    tracing::info!("drm: exit after {}", after.as_secs());
                    state.loop_signal.stop();
                    TimeoutAction::Drop
                })
                .map_err(|e| DrmError::EventLoop(e.to_string()))?;
        }
        None => tracing::warn!("drm: CONTRAST_EXIT_AFTER=0, the exit timer is disabled"),
    }
    {
        let shared = drm.clone();
        handle
            .insert_source(
                Timer::from_duration(FIRST_FLIP_TIMEOUT),
                move |_, _, state| {
                    with(&shared.clone(), |d| {
                        if d.frames == 0 && d.active && d.error.is_none() {
                            d.fail(state, DrmError::FlipTimeout);
                        }
                    });
                    TimeoutAction::Drop
                },
            )
            .map_err(|e| DrmError::EventLoop(e.to_string()))?;
    }

    center_pointer(state, &output);
    {
        let shared = drm.clone();
        with(&drm, |d| d.render(state, &shared));
    }
    if let Ok(mut d) = drm.try_borrow_mut()
        && let Some(e) = d.error.take()
    {
        return Err(e);
    }
    Ok(guard)
}

/// 出力を作り、Wayland のグローバル・Space・core に登録する（スケール 1、論理矩形 = モードサイズ）。
fn create_output(state: &mut Contrast, target: &Target) -> Result<Output, DrmError> {
    let mode = output_mode(&target.mode);
    let output = Output::new(
        target.name.clone(),
        PhysicalProperties {
            size: target.size_mm.into(),
            subpixel: Subpixel::Unknown,
            make: "Contrast".into(),
            model: "DRM".into(),
        },
    );
    let _global = output.create_global::<Contrast>(&state.display_handle);
    output.change_current_state(
        Some(mode),
        Some(Transform::Normal),
        None,
        Some((0, 0).into()),
    );
    output.set_preferred(mode);
    state.space.map_output(&output, (0, 0));
    let rect = Rect::new(0, 0, mode.size.w, mode.size.h)
        .ok_or_else(|| DrmError::Config(format!("mode size {:?} is not positive", mode.size)))?;
    state.output_added(&target.name, rect);
    Ok(output)
}

/// ポインタを出力の中央に置く。
fn center_pointer(state: &mut Contrast, output: &Output) {
    let Some(geo) = state.space.output_geometry(output) else {
        return;
    };
    let Some(pointer) = state.seat.get_pointer() else {
        return;
    };
    let center: Point<f64, Logical> = (
        f64::from(geo.loc.x) + f64::from(geo.size.w) / 2.0,
        f64::from(geo.loc.y) + f64::from(geo.size.h) / 2.0,
    )
        .into();
    pointer.motion(
        state,
        None,
        &MotionEvent {
            location: center,
            serial: SERIAL_COUNTER.next_serial(),
            time: 0,
        },
    );
    pointer.frame(state);
}

/// seat の DRM デバイスを列挙してログに出し、modeset の対象を決める。
fn enumerate(session: &mut LibSeatSession, seat: &str) -> Result<Target, DrmError> {
    let primary = primary_gpu(seat).map_err(|e| DrmError::Udev(e.to_string()))?;
    let udev = UdevBackend::new(seat).map_err(|e| DrmError::Udev(e.to_string()))?;
    let mut paths: Vec<PathBuf> = udev.device_list().map(|(_, p)| p.to_path_buf()).collect();
    paths.sort();
    if paths.is_empty() {
        tracing::warn!(seat = %seat, "drm: no DRM device on this seat");
    }

    let mut readable = 0usize;
    let mut chosen: Option<Target> = None;
    let mut fallback: Option<Target> = None;
    for path in &paths {
        let is_primary = primary.as_deref() == Some(path.as_path());
        tracing::info!("drm: device={} primary={is_primary}", path.display());
        match list_device(session, path) {
            Ok(target) => {
                readable = readable.saturating_add(1);
                if let Some(target) = target {
                    if is_primary && chosen.is_none() {
                        chosen = Some(target);
                    } else if fallback.is_none() {
                        fallback = Some(target);
                    }
                }
            }
            Err(e) => tracing::warn!("drm: device={} skipped: {e}", path.display()),
        }
    }
    if !paths.is_empty() && readable == 0 {
        return Err(DrmError::NoReadableDevice {
            active: session.is_active(),
        });
    }
    chosen.or(fallback).ok_or(DrmError::NoOutput)
}

/// 1 つの DRM デバイスを session 経由で開き、connector と CRTC をログに出して閉じる。
/// 最初の接続済み connector があれば、modeset の対象として返す。
fn list_device(session: &mut LibSeatSession, path: &Path) -> Result<Option<Target>, String> {
    let flags = OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK;
    let fd = session.open(path, flags).map_err(|e| e.to_string())?;
    let card = Card(fd);
    let result = log_resources(&card, path).map_err(|e| e.to_string());
    if let Err(e) = session.close(card.0) {
        tracing::warn!("drm: close {} failed: {e}", path.display());
    }
    result
}

/// connector（名前・状態・モード・物理サイズ）と CRTC をログに出す。最初の接続済み出力を返す。
fn log_resources(card: &Card, path: &Path) -> io::Result<Option<Target>> {
    let res = card.resource_handles()?;
    let mut target: Option<Target> = None;
    for &handle in res.connectors() {
        let info = card.get_connector(handle, false)?;
        let name = format!("{}-{}", info.interface().as_str(), info.interface_id());
        let status = match info.state() {
            State::Connected => "connected",
            State::Disconnected => "disconnected",
            State::Unknown => "unknown",
        };
        let preferred = info
            .modes()
            .iter()
            .find(|m| m.mode_type().contains(control::ModeTypeFlags::PREFERRED))
            .copied();
        let size = info
            .size()
            .map_or_else(|| "unknown".to_string(), |(w, h)| format!("{w}x{h}"));
        tracing::info!(
            "drm: connector={name} status={status} modes={} preferred={} size_mm={size}",
            info.modes().len(),
            preferred
                .as_ref()
                .map_or_else(|| "none".to_string(), format_mode)
        );
        for mode in info.modes() {
            tracing::info!("drm: mode connector={name} {}", format_mode(mode));
        }
        if target.is_none() && info.state() == State::Connected {
            let mode = preferred.or_else(|| info.modes().first().copied());
            let crtc = pick_crtc(card, &res, &info);
            if let (Some(mode), Some(crtc)) = (mode, crtc) {
                let (w, h) = info.size().unwrap_or((0, 0));
                target = Some(Target {
                    path: path.to_path_buf(),
                    connector: handle,
                    name,
                    crtc,
                    mode,
                    size_mm: (i32::try_from(w).unwrap_or(0), i32::try_from(h).unwrap_or(0)),
                });
            }
        }
    }
    for &crtc in res.crtcs() {
        tracing::info!("drm: crtc={}", u32::from(crtc));
    }
    Ok(target)
}

/// connector を駆動する CRTC: いま繋がっている encoder の CRTC、無ければ encoder が使える最初の CRTC。
fn pick_crtc(
    card: &Card,
    res: &control::ResourceHandles,
    info: &control::connector::Info,
) -> Option<crtc::Handle> {
    if let Some(encoder) = info.current_encoder()
        && let Ok(encoder) = card.get_encoder(encoder)
        && let Some(crtc) = encoder.crtc()
    {
        return Some(crtc);
    }
    info.encoders().iter().find_map(|&e| {
        let encoder = card.get_encoder(e).ok()?;
        res.filter_crtcs(encoder.possible_crtcs()).first().copied()
    })
}

/// 対象のデバイスを開き、GBM + EGL + GLES の renderer と、出力を用意する。最初の commit（modeset）はまだしない。
fn open_gpu(
    session: &mut LibSeatSession,
    target: &Target,
    output: &Output,
) -> Result<(Gpu, DrmDeviceNotifier), DrmError> {
    let flags = OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK;
    let fd = session
        .open(&target.path, flags)
        .map_err(|e| DrmError::Gpu(format!("open {}: {e}", target.path.display())))?;
    // DrmDeviceFd::new が master を取る。以降、Drop で手放す。
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));
    // `disable_connectors = false`: ここでは画面に触らない。最初の commit が modeset になる。
    let (drm, notifier) =
        DrmDevice::new(fd.clone(), false).map_err(|e| DrmError::Gpu(format!("DrmDevice: {e}")))?;
    let gbm = GbmDevice::new(fd).map_err(|e| DrmError::Gpu(format!("GbmDevice: {e}")))?;

    // SAFETY: `gbm` は有効な GBM デバイスで、EGLDisplay は `GbmDevice` の複製を保持するので、
    // ネイティブのデバイスは EGLDisplay より長く生きる。
    let display = unsafe { EGLDisplay::new(gbm.clone()) }
        .map_err(|e| DrmError::Gpu(format!("EGLDisplay: {e}")))?;
    let context =
        EGLContext::new(&display).map_err(|e| DrmError::Gpu(format!("EGLContext: {e}")))?;
    // SAFETY: `context` はこのスレッドでだけ使う新しい EGL コンテキストで、他で current になっていない。
    let mut renderer = unsafe { GlesRenderer::new(context) }
        .map_err(|e| DrmError::Gpu(format!("GlesRenderer: {e}")))?;

    let render_formats: Vec<_> = renderer
        .egl_context()
        .dmabuf_render_formats()
        .iter()
        .copied()
        .collect();
    let allocator = GbmAllocator::new(
        gbm.clone(),
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );
    let exporter = GbmFramebufferExporter::new(gbm.clone(), None);
    let color_formats = [
        Fourcc::Abgr8888,
        Fourcc::Argb8888,
        Fourcc::Xbgr8888,
        Fourcc::Xrgb8888,
    ];
    let mut manager: Manager = DrmOutputManager::new(
        drm,
        allocator,
        exporter,
        Some(gbm),
        color_formats,
        render_formats,
    );

    let handle = manager
        .initialize_output::<_, SceneElement>(
            target.crtc,
            target.mode,
            &[target.connector],
            output,
            None,
            &mut renderer,
            &DrmOutputRenderElements::default(),
        )
        .map_err(|e| DrmError::Modeset(e.to_string()))?;

    Ok((
        Gpu {
            output: Some(handle),
            manager: Some(manager),
            renderer: Some(renderer),
        },
        notifier,
    ))
}

/// DRM のモードを、出力のモード（サイズと mHz）にする。
fn output_mode(mode: &control::Mode) -> OutputMode {
    let (w, h) = mode.size();
    OutputMode {
        size: (i32::from(w), i32::from(h)).into(),
        refresh: i32::try_from(mode.vrefresh())
            .unwrap_or(0)
            .saturating_mul(1000),
    }
}

/// モードを `WxH@Hz` の形にする。
fn format_mode(mode: &control::Mode) -> String {
    let (w, h) = mode.size();
    format!("{w}x{h}@{}", mode.vrefresh())
}
