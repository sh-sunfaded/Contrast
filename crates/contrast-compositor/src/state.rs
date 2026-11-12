//! コンポジタ全体の状態と、Wayland ソケットの用意。

use std::{
    ffi::OsString,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicI32, Ordering},
    },
};

use smithay::{
    backend::{
        allocator::{Format, dmabuf::Dmabuf},
        renderer::element::solid::SolidColorBuffer,
    },
    desktop::{PopupManager, Space, Window},
    input::{Seat, SeatState, keyboard::Keycode},
    reexports::{
        calloop::{EventLoop, Interest, LoopSignal, Mode, PostAction, generic::Generic},
        wayland_server::{
            Display, DisplayHandle,
            backend::{ClientData, ClientId, DisconnectReason},
        },
    },
    wayland::{
        compositor::{CompositorClientState, CompositorState},
        dmabuf::{DmabufFeedback, DmabufGlobal, DmabufState},
        input_method::{InputMethodManagerState, PopupSurface},
        output::OutputManagerState,
        selection::{
            data_device::DataDeviceState, primary_selection::PrimarySelectionState,
            wlr_data_control::DataControlState,
        },
        shell::{wlr_layer::WlrLayerShellState, xdg::XdgShellState},
        shm::ShmState,
        socket::ListeningSocketSource,
        text_input::TextInputManagerState,
        virtual_keyboard::VirtualKeyboardManagerState,
    },
};

use contrast_policy::{ConfigLoader, ConfigStore, Policy, PortPlacement};

use crate::ConfigSource;

use crate::shell::{ContrastCore, Shell};

/// dmabuf をレンダラーにインポートできるか試す関数（できれば `true`）。
pub type DmabufImporter = Box<dyn FnMut(&Dmabuf) -> bool>;

/// コンポジタの状態。calloop のイベントループが `&mut` で持ち回る。
pub struct Contrast {
    /// 起動時刻（frame コールバックの時刻に使う）。
    pub start_time: std::time::Instant,
    /// クライアント向けに公開したソケット名。
    pub socket_name: OsString,
    /// niri 互換の IPC のソケットのパス（IPC が起動していなければ `None`）。`Startup` の `ipc="niri"` の子に渡す。
    pub ipc_socket: Option<std::path::PathBuf>,
    /// niri 互換の IPC のサーバー（起動していなければ `None`）。`Drop` でソケットを消す。
    pub(crate) ipc: Option<crate::ipc::IpcServer>,
    /// 窓がフォーカスを得た時刻（IPC の `focus_timestamp`。窓の id から）。
    pub(crate) ipc_focus_times: std::collections::BTreeMap<u64, crate::ipc::Timestamp>,
    /// IPC で最後に見たフォーカスの窓。
    pub(crate) ipc_last_focus: Option<u64>,
    /// Display のハンドル。
    pub display_handle: DisplayHandle,
    /// 窓と出力を置く 2 次元平面。
    pub space: Space<Window>,
    /// イベントループを止めるための信号。
    pub loop_signal: LoopSignal,
    /// 配置・キーバインドの問い合わせ先。
    /// core の配置判断（`PortPlacement`）とも共有する。
    pub policy: Rc<dyn Policy>,
    /// 現在の設定（`policy` の実体）。リロードで差し替える。
    pub store: Rc<ConfigStore>,
    /// リロードで設定を読み直す読み込み器。
    pub config_loader: Box<dyn ConfigLoader>,
    /// 設定ファイルのパス。
    pub config_path: Option<std::path::PathBuf>,
    /// 配置・フォーカスを決める状態遷移エンジン。
    pub core: ContrastCore,
    /// core の ExternalKey とシェル側の窓・出力の対応。
    pub shell: Shell,
    /// 枠の帯のバッファ（帯 1 本に 1 つ。フォーカス窓の 4 本、続けてフォーカス外の窓の分）。Id を保って描画の差分追跡を効かせる。
    pub border_buffers: Vec<SolidColorBuffer>,
    /// キーバインドとして握りつぶした押下中のキー（離したときも素通ししない）。
    pub suppressed_keys: Vec<Keycode>,
    /// ホイールの横方向の目盛りの足し込み（ホイールのバインド用）。
    pub(crate) wheel_horizontal: crate::input::wheel::ScrollTracker,
    /// ホイールの縦方向の目盛りの足し込み。
    pub(crate) wheel_vertical: crate::input::wheel::ScrollTracker,
    /// バインドの `cooldown`（最後に実行した時刻）。
    pub(crate) bind_cooldowns: crate::input::wheel::CooldownGate,

    pub xdg_shell_state: XdgShellState,
    pub compositor_state: CompositorState,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<Contrast>,
    pub data_device_state: DataDeviceState,
    pub primary_selection_state: PrimarySelectionState,
    /// wlr-data-control（ironbar の clipboard、wl-clipboard、cliphist）。許可リストのクライアントにだけ見せる。
    pub data_control_state: DataControlState,
    /// wp_viewporter（xwayland-satellite が要る。描画は smithay の面の状態が viewport を扱う）。
    /// wp_pointer_gestures（スワイプ・ピンチ・ホールドをクライアントに渡す）。
    pub pointer_gestures_state: smithay::wayland::pointer_gestures::PointerGesturesState,
    pub viewporter_state: smithay::wayland::viewporter::ViewporterState,
    /// wp_fractional_scale（出力の拡大率を、面ごとの好みの拡大率として伝える）。
    pub fractional_scale_state: smithay::wayland::fractional_scale::FractionalScaleManagerState,
    /// wlr-data-control を使ってよいクライアントのプログラム名（設定の `DataControlClient`）。
    pub data_control_clients: crate::data_control::AllowList,
    pub layer_shell_state: WlrLayerShellState,
    /// text-input-v3（アプリが入力メソッドに状態を伝える）。
    pub text_input_manager_state: TextInputManagerState,
    /// input-method-v2（fcitx5 などの入力メソッド）。
    pub input_method_manager_state: InputMethodManagerState,
    /// virtual-keyboard-v1（入力メソッドがキーを送り返す）。
    pub virtual_keyboard_manager_state: VirtualKeyboardManagerState,
    /// 表示中の入力メソッドのポップアップ（候補ウィンドウ）。
    pub ime_popups: Vec<PopupSurface>,
    /// キーボードフォーカスを持っているレイヤー面（無ければ core のフォーカス窓）。
    pub layer_focus: Option<smithay::reexports::wayland_server::protocol::wl_surface::WlSurface>,
    pub popups: PopupManager,
    pub seat: Seat<Self>,
    /// linux-dmabuf の状態。グローバルはレンダラーができてから `enable_dmabuf` で作る。
    pub dmabuf_state: DmabufState,
    /// 公開した linux-dmabuf のグローバル（`enable_dmabuf` の後）。
    pub dmabuf_global: Option<DmabufGlobal>,
    /// クライアントの dmabuf をレンダラーにインポートできるか試す関数（バックエンドが入れる）。
    pub dmabuf_importer: Option<DmabufImporter>,
    /// VT を切り替える関数（DRM バックエンドだけが入れる）。キーボードをコンポジタが持つので、これが無いと VT を移れない。
    pub vt_switch: Option<Box<dyn FnMut(i32)>>,
    /// `screenshot-screen` が押された。次に描くフレームを読み出して保存する。
    pub screenshot_pending: bool,
    /// `screenshot-window` が押された。次のフレームの前に、フォーカス窓の絵を保存する。
    pub screenshot_window_pending: bool,
    /// スクリーンショットの範囲を選ぶ UI（niri の `screenshot`）。
    pub screenshot_ui: crate::screenshot_ui_shell::ScreenshotUiState,
    /// 最後のタッチの位置（指が離れるときの位置に使う。全体の論理座標）。
    /// いまのスワイプをクライアントに渡しているか（3・4 本指以外）。
    pub swipe_forwarded: bool,
    /// 最後のタッチの位置（指が離れるときの位置に使う。全体の論理座標）。
    pub touch_last: smithay::utils::Point<f64, smithay::utils::Logical>,
    /// ホットキーのオーバーレイ（起動時に開く。niri の既定）。
    pub hotkey_overlay: crate::hotkey_overlay::HotkeyOverlay,
    /// タッチパッドのジェスチャーと DnD の端スクロール。
    pub gestures: crate::gestures::Gestures,
    /// オーバービュー。
    pub overview: crate::overview::Overview,
    /// ポインタが左上のホットコーナーに居る（入ったときに 1 回だけ切り替える）。
    pub pointer_in_hot_corner: bool,
    /// XWayland（xwayland-satellite を必要になったときに起動する）。使えなければ `None`。
    pub xwayland: Option<crate::xwayland::Xwayland>,
    /// 画面の電源（`power-off-monitors`。DRM バックエンドだけが関数を入れる）。
    pub monitors: crate::power::Monitors,
    /// クライアントが指定した（または既定の）カーソル。DRM バックエンドが描く。
    pub cursor_status: smithay::input::pointer::CursorImageStatus,
    /// 起動時に決まった窓の並べ方。リロードでは変わらない（core を作り直さないため）。
    pub layout_mode: contrast_core::LayoutMode,
    /// libinput のデバイス（設定のリロードで再適用するために覚える。DRM バックエンドだけが入れる）。
    pub input_devices: Vec<smithay::reexports::input::Device>,
    /// キー処理の中で見つかった VT 切り替えの要求（キー処理が終わってから実行する）。
    pub pending_vt: Option<i32>,
    /// 窓の矩形の補間（`Layout.animationMs`）。
    pub anim: crate::anim::Animator,
    /// 最後に入力（キー・ポインタ）があった時刻。`Cursor.hideAfterInactiveMs` でカーソルを隠すのに使う。
    pub last_input: std::time::Instant,
    /// winit のホストの窓でカーソルを隠している状態（変わったときだけホストに伝える）。
    pub host_cursor_hidden: bool,
    /// デバッグの `unplug` で外した出力と、その矩形（`replug` で戻す）。
    pub unplugged: Vec<(smithay::output::Output, contrast_core::Rect)>,
    /// 窓の開く・閉じるアニメーション（niri の window-open／window-close）。
    pub window_anims: crate::window_anim::WindowAnims,
}

impl Contrast {
    /// 各 Wayland グローバルを作り、ソケットをイベントループに登録する。
    pub fn new(
        event_loop: &mut EventLoop<Self>,
        display: Display<Self>,
        store: Rc<ConfigStore>,
        source: ConfigSource,
    ) -> Self {
        let policy: Rc<dyn Policy> = store.clone();
        let dh = display.handle();

        let compositor_state = CompositorState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let mut seat_state = SeatState::new();
        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let primary_selection_state = PrimarySelectionState::new::<Self>(&dh);
        let data_control_clients: crate::data_control::AllowList =
            std::sync::Arc::new(std::sync::Mutex::new(policy.data_control_clients()));
        let data_control_state = DataControlState::new::<Self, _>(
            &dh,
            Some(&primary_selection_state),
            crate::data_control::client_filter(data_control_clients.clone()),
        );
        let layer_shell_state = WlrLayerShellState::new::<Self>(&dh);
        let viewporter_state = smithay::wayland::viewporter::ViewporterState::new::<Self>(&dh);
        let pointer_gestures_state =
            smithay::wayland::pointer_gestures::PointerGesturesState::new::<Self>(&dh);
        let fractional_scale_state =
            smithay::wayland::fractional_scale::FractionalScaleManagerState::new::<Self>(&dh);
        let text_input_manager_state = TextInputManagerState::new::<Self>(&dh);
        let input_method_manager_state = InputMethodManagerState::new::<Self, _>(&dh, |_| true);
        let virtual_keyboard_manager_state =
            VirtualKeyboardManagerState::new::<Self, _>(&dh, |_| true);
        let mut seat: Seat<Self> = seat_state.new_wl_seat(&dh, "contrast");
        add_keyboard(&mut seat);
        seat.add_pointer();
        seat.add_touch();

        let socket_name = Self::init_wayland_listener(display, event_loop);
        let layout = policy.layout_settings();
        let skip_hotkey_overlay = policy.hotkey_overlay_settings().skip_at_startup;
        let mut core_config = contrast_core::CoreConfig::new(policy.workspace_count())
            .with_workspace_mode(policy.workspace_mode())
            .with_layout(layout.mode)
            .with_preset_column_widths(layout.preset_column_widths.clone())
            .with_preset_window_heights(layout.window_height_presets.clone());
        if let Some(width) = layout.default_column_width {
            core_config = core_config.with_default_column_width(width);
        }
        let layout_mode = layout.mode;
        let core = ContrastCore::new(
            core_config,
            PortPlacement::new(Rc::clone(&policy)),
            contrast_core::Columns::new(),
        );

        Self {
            start_time: std::time::Instant::now(),
            socket_name,
            ipc_socket: None,
            ipc: None,
            ipc_focus_times: Default::default(),
            ipc_last_focus: None,
            display_handle: dh,
            space: Space::default(),
            loop_signal: event_loop.get_signal(),
            policy,
            store,
            config_loader: source.loader,
            config_path: source.path,
            core,
            shell: Shell::default(),
            border_buffers: Default::default(),
            suppressed_keys: Vec::new(),
            wheel_horizontal: crate::input::wheel::ScrollTracker::new(
                crate::input::wheel::WHEEL_TICK_V120,
            ),
            wheel_vertical: crate::input::wheel::ScrollTracker::new(
                crate::input::wheel::WHEEL_TICK_V120,
            ),
            bind_cooldowns: Default::default(),
            data_control_state,
            viewporter_state,
            fractional_scale_state,
            data_control_clients,
            xdg_shell_state,
            compositor_state,
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            primary_selection_state,
            layer_shell_state,
            text_input_manager_state,
            input_method_manager_state,
            virtual_keyboard_manager_state,
            ime_popups: Vec::new(),
            layer_focus: None,
            popups: PopupManager::default(),
            seat,
            dmabuf_state: DmabufState::new(),
            dmabuf_global: None,
            dmabuf_importer: None,
            vt_switch: None,
            pending_vt: None,
            monitors: crate::power::Monitors::default(),
            screenshot_pending: false,
            anim: crate::anim::Animator::default(),
            last_input: std::time::Instant::now(),
            host_cursor_hidden: false,
            unplugged: Vec::new(),
            window_anims: Default::default(),
            screenshot_window_pending: false,
            screenshot_ui: Default::default(),
            touch_last: Default::default(),
            swipe_forwarded: false,
            pointer_gestures_state,
            hotkey_overlay: crate::hotkey_overlay::HotkeyOverlay::at_startup(skip_hotkey_overlay),
            gestures: Default::default(),
            overview: Default::default(),
            pointer_in_hot_corner: false,
            xwayland: None,
            input_devices: Vec::new(),
            layout_mode,
            cursor_status: smithay::input::pointer::CursorImageStatus::default_named(),
        }
    }

    /// zwp_linux_dmabuf_v1 を `formats`（レンダラーが受け付けるもの）で公開する。`importer` は、バッファがレンダラーに
    /// インポートできるかを試す。できなければクライアントにエラーを返す。
    pub fn enable_dmabuf(
        &mut self,
        formats: impl IntoIterator<Item = Format>,
        feedback: Option<DmabufFeedback>,
        importer: DmabufImporter,
    ) {
        let formats: Vec<Format> = formats.into_iter().collect();
        tracing::info!(
            formats = formats.len(),
            feedback = feedback.is_some(),
            "dmabuf: enabled"
        );
        self.dmabuf_importer = Some(importer);
        // v4（フィードバックつき）は、クライアントの Mesa が描画デバイスを知るのに要る。無ければ v3。
        let global = match &feedback {
            Some(f) => self
                .dmabuf_state
                .create_global_with_default_feedback::<Self>(&self.display_handle, f),
            None => self
                .dmabuf_state
                .create_global::<Self>(&self.display_handle, formats),
        };
        self.dmabuf_global = Some(global);
    }

    /// 空いている `wayland-N` でソケットを開き、Display と一緒にイベントループへ入れる。
    fn init_wayland_listener(display: Display<Self>, event_loop: &mut EventLoop<Self>) -> OsString {
        let listening_socket = ListeningSocketSource::new_auto().expect("failed to open socket");
        let socket_name = listening_socket.socket_name().to_os_string();
        let handle = event_loop.handle();

        handle
            .insert_source(listening_socket, |stream, _, state| {
                let data = Arc::new(ClientState::default());
                let client = state
                    .display_handle
                    .insert_client(stream, data.clone())
                    .expect("failed to insert client");
                // pid は、ディスパッチの外（ここ）で調べて残す。ディスパッチの中（グローバルのフィルタ）で
                // `get_credentials` を呼ぶと、バックエンドのロックと競合して止まる。
                if let Ok(credentials) = client.get_credentials(&state.display_handle) {
                    data.pid.store(credentials.pid, Ordering::Relaxed);
                }
                tracing::info!(client = ?client.id(), "client connected");
            })
            .expect("failed to init the wayland socket source");

        handle
            .insert_source(
                Generic::new(display, Interest::READ, Mode::Level),
                |_, display, state| {
                    // SAFETY: Display はこのソースが持ち続け、ここで drop しない。
                    unsafe {
                        display
                            .get_mut()
                            .dispatch_clients(state)
                            .expect("dispatch_clients failed");
                    }
                    Ok(PostAction::Continue)
                },
            )
            .expect("failed to insert display source");

        socket_name
    }
}

/// キーリピートの間隔（ミリ秒）。niri の既定（`niri-config` の `repeat-delay 600`）。
const KEY_REPEAT_DELAY_MS: i32 = 600;
/// キーリピートの速さ（1 秒あたりの回数）。niri の既定（`repeat-rate 25`）。
const KEY_REPEAT_RATE: i32 = 25;

/// xkb の設定（rules は使わない）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct XkbNames {
    /// 配列。
    pub layout: String,
    /// 変種。
    pub variant: String,
    /// 型。
    pub model: String,
    /// オプション（空なら None）。
    pub options: Option<String>,
}

/// `busctl get-property` の 1 行（`s "us"`）から文字列を取り出す。形が違えば `None`。
pub fn parse_busctl_string(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix("s ")?;
    let inner = rest.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.replace("\\\"", "\"").replace("\\\\", "\\"))
}

/// systemd-localed（`org.freedesktop.locale1`、システムの D-Bus）の X11 の配列を読む（niri の `freedesktop_locale1`）。
/// zbus は依存に無いので `busctl` を使う。読めなければ `None`。
pub(crate) fn xkb_from_locale1() -> Option<XkbNames> {
    let out = std::process::Command::new("busctl")
        .args([
            "--system",
            "get-property",
            "org.freedesktop.locale1",
            "/org/freedesktop/locale1",
            "org.freedesktop.locale1",
            "X11Layout",
            "X11Model",
            "X11Variant",
            "X11Options",
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let values: Vec<String> = text.lines().filter_map(parse_busctl_string).collect();
    let [layout, model, variant, options] = <[String; 4]>::try_from(values).ok()?;
    Some(XkbNames {
        layout,
        variant,
        model,
        options: (!options.is_empty()).then_some(options),
    })
}

/// キーボードを seat に足す。配列は `CONTRAST_XKB_LAYOUT` / `_VARIANT` / `_MODEL` / `_OPTIONS`。
/// どれも指定が無ければ、niri と同じく systemd-localed の X11 の配列（起動時に 1 回読む）、それも無ければ xkbcommon の既定。
/// 指定が不正でキーマップを作れなければ、警告して既定にする。
fn add_keyboard(seat: &mut Seat<Contrast>) {
    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    let from_env = XkbNames {
        layout: var("CONTRAST_XKB_LAYOUT").unwrap_or_default(),
        variant: var("CONTRAST_XKB_VARIANT").unwrap_or_default(),
        model: var("CONTRAST_XKB_MODEL").unwrap_or_default(),
        options: var("CONTRAST_XKB_OPTIONS"),
    };
    let names = if from_env == XkbNames::default() {
        match xkb_from_locale1() {
            Some(n) => {
                tracing::info!(?n, "xkb: using the settings of systemd-localed");
                n
            }
            None => from_env,
        }
    } else {
        from_env
    };
    let config = smithay::input::keyboard::XkbConfig {
        layout: &names.layout,
        variant: &names.variant,
        model: &names.model,
        options: names.options.clone(),
        ..Default::default()
    };
    if let Err(e) = seat.add_keyboard(config, KEY_REPEAT_DELAY_MS, KEY_REPEAT_RATE) {
        tracing::warn!(
            ?e,
            ?names,
            "xkb: invalid keymap settings, using the default"
        );
        if let Err(e) = seat.add_keyboard(Default::default(), KEY_REPEAT_DELAY_MS, KEY_REPEAT_RATE)
        {
            tracing::error!(?e, "xkb: failed to add a keyboard");
        }
    }
}

#[cfg(test)]
mod xkb_tests {
    use super::parse_busctl_string;

    #[test]
    fn busctl_strings_are_unquoted() {
        assert_eq!(parse_busctl_string("s \"us\""), Some("us".to_string()));
        assert_eq!(parse_busctl_string("s \"\""), Some(String::new()));
        assert_eq!(
            parse_busctl_string("s \"terminate:ctrl_alt_bksp\"\n"),
            Some("terminate:ctrl_alt_bksp".to_string())
        );
        assert_eq!(parse_busctl_string("u 3"), None);
    }
}

/// クライアントごとのデータ。
#[derive(Default)]
pub struct ClientState {
    /// wl_compositor のクライアント側状態。
    pub compositor_state: CompositorClientState,
    /// 接続したプロセスの pid。0 は不明。
    pub pid: AtomicI32,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, client_id: ClientId, reason: DisconnectReason) {
        tracing::info!(client = ?client_id, ?reason, "client disconnected");
    }
}
