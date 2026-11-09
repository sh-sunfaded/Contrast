//! ポリシーの差し込み口。
//!
//! コンポジタ（Smithay 側）はこのトレイトだけを見る。配置・キーバインド・見た目の
//! 決定はすべてここを経由させ、実装を `StaticPolicy` から CDL 由来の実装へ
//! 差し替えられるようにしておく。

use contrast_core::{Rect, WindowMeta, WindowState};

/// キー入力時の修飾キーの状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    /// Alt。
    pub alt: bool,
    /// Shift。
    pub shift: bool,
    /// Ctrl。
    pub ctrl: bool,
    /// Super / Logo。
    pub logo: bool,
}

/// キーバインド解決後の意味操作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// コマンドを起動する。空白で区切った先頭がプログラム、残りが引数。
    Spawn(String),
    /// フォーカス窓を閉じる。
    CloseFocused,
    /// 並びの前の窓へフォーカスを移す。
    FocusPrev,
    /// 並びの次の窓へフォーカスを移す。
    FocusNext,
    /// フォーカス窓を並びの左の窓と入れ替える。
    MoveLeft,
    /// フォーカス窓を並びの右の窓と入れ替える。
    MoveRight,
    /// ワークスペースを切り替える（0 始まり）。
    SwitchWorkspace(u8),
    /// フォーカス窓を別のワークスペースへ移す（0 始まり）。
    MoveToWorkspace(u8),
    /// 表示中の次のワークスペースへ切り替える。最後の次は無い（循環しない）。
    WorkspaceNext,
    /// 表示中の前のワークスペースへ切り替える。最初の前は無い（循環しない）。
    WorkspacePrev,
    /// フォーカス窓を次のワークスペースへ移す。最後の次は無い（循環しない）。
    MoveToWorkspaceNext,
    /// フォーカス窓を前のワークスペースへ移す。最初の前は無い（循環しない）。
    MoveToWorkspacePrev,
    /// フォーカス窓の列ごと、別のワークスペースへ移して切り替える（0 始まり。スクロール列レイアウトのみ）。
    MoveColumnToWorkspace(u8),
    /// 列ごと、次のワークスペースへ移して切り替える。最後の次は無い（循環しない）。
    MoveColumnToWorkspaceNext,
    /// 列ごと、前のワークスペースへ移して切り替える。最初の前は無い（循環しない）。
    MoveColumnToWorkspacePrev,
    /// 表示中のワークスペースを 1 つ下と入れ替える（動的なワークスペースのみ）。
    MoveWorkspaceDown,
    /// 表示中のワークスペースを 1 つ上と入れ替える（動的なワークスペースのみ）。
    MoveWorkspaceUp,
    /// フォーカスを上の窓へ移す（スクロール列レイアウトでは列の中の上の窓）。
    FocusUp,
    /// フォーカスを下の窓へ移す（スクロール列レイアウトでは列の中の下の窓）。
    FocusDown,
    /// フォーカス窓を上へ動かす（スクロール列レイアウトでは列の中で入れ替える）。
    MoveUp,
    /// フォーカス窓を下へ動かす（スクロール列レイアウトでは列の中で入れ替える）。
    MoveDown,
    /// 最初の列へフォーカスを移す（スクロール列レイアウトのみ）。
    FocusColumnFirst,
    /// 最後の列へフォーカスを移す（スクロール列レイアウトのみ）。
    FocusColumnLast,
    /// フォーカス列を最初の位置へ移す（スクロール列レイアウトのみ）。
    MoveColumnFirst,
    /// フォーカス列を最後の位置へ移す（スクロール列レイアウトのみ）。
    MoveColumnLast,
    /// 右隣の列の先頭の窓を、フォーカス列の最下段へ取り込む（スクロール列レイアウトのみ）。
    ConsumeWindowIntoColumn,
    /// フォーカス窓を、右隣の新しい列へ出す（スクロール列レイアウトのみ）。
    ExpelWindowFromColumn,
    /// 列に窓が 1 枚なら左隣の列の最下段へ、2 枚以上なら左隣に新しい列として出す（スクロール列レイアウトのみ）。
    ConsumeOrExpelWindowLeft,
    /// 列に窓が 1 枚なら右隣の列の最下段へ、2 枚以上なら右隣に新しい列として出す（スクロール列レイアウトのみ）。
    ConsumeOrExpelWindowRight,
    /// フォーカス列の幅を、次のプリセットへ変える（スクロール列レイアウトのみ）。
    SwitchPresetColumnWidth,
    /// フォーカス列を最大化する。最大化中なら元の幅に戻す（スクロール列レイアウトのみ）。
    MaximizeColumn,
    /// フォーカス列の幅を、作業領域の幅の割合にする（スクロール列レイアウトのみ）。
    SetColumnWidth(contrast_core::Ratio),
    /// フォーカス列の幅を、作業領域の幅の `n` パーセントぶん増減する（スクロール列レイアウトのみ）。
    AdjustColumnWidth(i8),
    /// その方向の出力をアクティブにする（niri の `focus-monitor-*`。動的なワークスペースのみ）。
    FocusMonitor(contrast_core::Direction),
    /// フォーカス窓の列ごと、その方向の出力へ移す（niri の `move-column-to-monitor-*`。スクロール列・動的なワークスペースのみ）。
    MoveColumnToMonitor(contrast_core::Direction),
    /// フォーカス列の中心を作業領域の中心に合わせる（スクロール列レイアウトのみ）。
    CenterColumn,
    /// フォーカス窓の高さを、次のプリセットへ変える（スクロール列レイアウトのみ）。
    SwitchPresetWindowHeight,
    /// フォーカス窓の高さを自動（`Auto`）に戻す（スクロール列レイアウトのみ）。
    ResetWindowHeight,
    /// フォーカス窓の高さを指定する（スクロール列レイアウトのみ）。
    SetWindowHeight(contrast_core::WindowHeight),
    /// フォーカス窓の高さを、作業領域の高さの `n` パーセントぶん増減する（スクロール列レイアウトのみ）。
    AdjustWindowHeight(i8),
    /// フォーカス窓の Tiled / Floating を切り替える。
    ToggleFloating,
    /// フォーカス窓の全画面を切り替える。
    ToggleFullscreen,
    /// 設定を再読み込みする。
    ReloadConfig,
    /// コンポジタを終了する。
    Quit,
    /// 出力の絵をスクリーンショットにして保存する（niri の `screenshot-screen`）。
    ScreenshotScreen,
    /// フォーカス窓だけの絵を PNG で保存する（niri の `screenshot-window`。ポインタは入れない）。
    ScreenshotWindow,
    /// スクリーンショットの範囲を選ぶ UI を開く（niri の `screenshot`）。
    Screenshot,
    /// ホットキーのオーバーレイを出す（niri の `show-hotkey-overlay`）。
    ShowHotkeyOverlay,
    /// 画面を消す（DPMS。niri の `power-off-monitors`）。どんな入力でも戻る。
    PowerOffMonitors,
}

/// 新しい窓の配置の判断（`Policy::place_window` の結果）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowPlacement {
    /// 配置先のワークスペース（0 始まり）。`None` ならフォーカス出力の表示ワークスペース。
    pub workspace: Option<u8>,
    /// 初期状態。
    pub state: WindowState,
    /// 配置先が表示中ならフォーカスを与えるか。
    pub focus: bool,
    /// 開いた直後に全画面にするか。core の配置は Tiled / Floating までなので、コンポジタが配置のあとに
    /// `ToggleFullscreen` を送る（窓がフォーカスを得たときだけ効く）。
    pub fullscreen: bool,
}

/// コンポジタがポリシーに問い合わせる口。
pub trait Policy {
    /// 背景色（RGBA、0.0〜1.0）。
    fn background(&self) -> [f32; 4];

    /// フォーカス枠の太さ（論理ピクセル、0 で枠なし）。窓はタイルからこの幅だけ内側に縮めて置く。
    fn border_width(&self) -> i32;

    /// フォーカス枠の色（RGBA、0.0〜1.0）。
    fn focus_border_color(&self) -> [f32; 4];

    /// フォーカス外の窓の枠の色（RGBA、0.0〜1.0）。`None` なら描かない。
    fn unfocused_border_color(&self) -> Option<[f32; 4]> {
        None
    }
    /// ワークスペースの数（1 以上）。
    fn workspace_count(&self) -> u8;

    /// キーバインドの一覧（設定の順）。ホットキーのオーバーレイに使う。既定は空。
    fn keybindings(&self) -> Vec<crate::settings::KeyBinding> {
        Vec::new()
    }

    /// キーバインドの `mod` が指す修飾キー（スクリーンショットの UI の Mod+クリックなどに使う）。既定は Alt。
    fn mod_key(&self) -> crate::settings::ModKey {
        crate::settings::ModKey::Alt
    }

    /// ワークスペースの方式（固定か動的か）。起動時に決まり、リロードでは変わらない。既定は固定。
    fn workspace_mode(&self) -> contrast_core::WorkspaceMode {
        contrast_core::WorkspaceMode::Fixed
    }

    /// 見た目の設定（角丸・影・枠など）。
    fn appearance(&self) -> crate::Appearance {
        crate::Settings::builtin().appearance
    }

    /// 窓の隙間の設定。
    fn layout_settings(&self) -> crate::LayoutSettings {
        crate::LayoutSettings::default()
    }

    /// カーソルの設定。
    fn cursor_settings(&self) -> crate::CursorSettings {
        crate::CursorSettings::default()
    }

    /// スクリーンショットの保存先（`~` と strftime の `%Y` などを展開する）。`None` なら保存しない。
    fn screenshot_path(&self) -> Option<String> {
        Some(crate::DEFAULT_SCREENSHOT_PATH.to_owned())
    }

    /// 入力デバイス（タッチパッドなど）の設定。
    fn input_settings(&self) -> crate::InputSettings {
        crate::InputSettings::default()
    }

    /// 起動時に一度だけ実行するコマンド。
    fn startup_commands(&self) -> Vec<crate::StartupCommand> {
        Vec::new()
    }

    /// wlr-data-control を使ってよいクライアントのプログラム名。既定は空（だれにも見せない）。
    fn data_control_clients(&self) -> Vec<String> {
        Vec::new()
    }

    /// spawn する子プロセスに足す環境変数（名前順）。
    fn environment(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    /// 作業領域 `area` に `count` 個の窓を並べた矩形を、並び順に返す。
    ///
    /// 戻り値の長さは `count` と等しくなければならない。
    fn layout(&self, area: Rect, count: usize) -> Vec<Rect>;

    /// 新しい窓（app_id・title・is_dialog など）をどこにどう置くか決める。
    ///
    /// contrast-core の `Policy::place` には `core_adapter::PortPlacement` 経由で渡る。
    ///
    /// `focused_workspace` はフォーカス出力の表示ワークスペース（0 始まり。出力が無ければ `None`）。
    fn place_window(&self, meta: &WindowMeta, focused_workspace: Option<u8>) -> WindowPlacement;

    /// 修飾キーとキーシンボル名（xkb の名前、例 `"Return"`, `"q"`, `"Left"`）から
    /// 意味操作を引く。該当しなければ `None`（クライアントへ素通しする）。
    fn resolve_key(&self, mods: Mods, keysym: &str) -> Option<Action>;

    /// `resolve_key` で当たるバインドの間引き（ミリ秒）。最初の実行から、この時間のあいだは同じバインドを実行しない。
    /// 既定は間引かない。
    fn key_cooldown_ms(&self, _mods: Mods, _keysym: &str) -> Option<u32> {
        None
    }
}
