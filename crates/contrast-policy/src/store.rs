//! 設定 `Config`（設定値とフック）と、それを持つ `ConfigStore`。
//!
//! `ConfigStore` はコンポジタが共有する `Policy` の実体。読み込みに成功した版だけを丸ごと差し替え、
//! そのたびに世代番号を進める。失敗した版は採用しない（旧版を保持）。
//! フックの失敗は、その呼び出しだけ既定の配置に戻し、同じ診断コードは世代ごとに 1 回だけログに出す。

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::path::Path;
use std::rc::Rc;

use contrast_core::{Rect, WindowMeta};

use crate::codes;
use crate::config::{ConfigDiagnostic, ConfigLoader, DiagnosticSeverity, LoadOutcome};
use crate::hook::{HookPlacement, PlacementHook, WindowInfo};
use crate::port::{Action, Mods, Policy, WindowPlacement};
use crate::settings::Settings;
use crate::static_policy::default_placement;

/// 設定の 1 つの版。
pub struct Config {
    /// 設定値。
    pub settings: Settings,
    /// `windowOpen` フック。無ければ既定の配置。
    pub hook: Option<Box<dyn PlacementHook>>,
    /// 読み込みで出た警告（成功したが知らせたいこと。例: `<Motion>` があるので旧 `Layout.animation*` を無視する）。
    pub warnings: Vec<ConfigDiagnostic>,
    /// 設定ファイルが関数形ルートのとき、`Env` で設定を作る口（`settings` はその基準の `Env` で作った値）。
    pub env_renderer: Option<Rc<dyn crate::env::EnvRenderer>>,
    /// 設定を読んだときのプロファイル名（`Env.profile`）。
    pub profile: String,
}

impl Config {
    /// 設定値とフックから作る。
    pub fn new(settings: Settings, hook: Option<Box<dyn PlacementHook>>) -> Self {
        Self {
            settings,
            hook,
            warnings: Vec::new(),
            env_renderer: None,
            profile: crate::env::DEFAULT_PROFILE.to_owned(),
        }
    }

    /// `Env` で設定を作る口とプロファイル名を付ける（関数形ルートの設定）。
    #[must_use]
    pub fn with_env(mut self, renderer: Rc<dyn crate::env::EnvRenderer>, profile: String) -> Self {
        self.env_renderer = Some(renderer);
        self.profile = profile;
        self
    }

    /// 読み込みの警告を付ける。
    #[must_use]
    pub fn with_warnings(mut self, warnings: Vec<ConfigDiagnostic>) -> Self {
        self.warnings = warnings;
        self
    }

    /// 組み込みの既定値（フックなし）。
    pub fn builtin() -> Self {
        Self::new(Settings::builtin(), None)
    }

    /// 新しい窓の配置を決める。フックが失敗したときは既定の配置と診断を返す。
    pub fn place(
        &self,
        meta: &WindowMeta,
        focused_workspace: Option<u8>,
    ) -> (WindowPlacement, Option<ConfigDiagnostic>) {
        let default = default_placement(meta);
        let Some(hook) = &self.hook else {
            return (default, None);
        };
        let info = WindowInfo {
            app_id: meta.app_id.clone().unwrap_or_default(),
            title: meta.title.clone(),
            is_dialog: meta.is_dialog,
            workspace: focused_workspace.unwrap_or(0),
        };
        match hook.window_open(&info) {
            Ok(None) => (default, None),
            Err(diagnostic) => (default, Some(diagnostic)),
            Ok(Some(p)) => match self.validate(&p) {
                Ok(placement) => (placement, None),
                Err(diagnostic) => (default, Some(diagnostic)),
            },
        }
    }

    /// フックの結果を検査して `WindowPlacement` にする。
    fn validate(&self, p: &HookPlacement) -> Result<WindowPlacement, ConfigDiagnostic> {
        if let Some(n) = p.workspace
            && n >= self.settings.workspaces
        {
            return Err(ConfigDiagnostic {
                severity: DiagnosticSeverity::Warning,
                code: codes::HOOK_WORKSPACE_RANGE.to_owned(),
                file: String::new(),
                line: None,
                column: None,
                message: format!(
                    "windowOpen の workspace {} はワークスペース数 {} を超える",
                    u16::from(n).saturating_add(1),
                    self.settings.workspaces
                ),
            });
        }
        Ok(WindowPlacement {
            workspace: p.workspace,
            state: if p.floating {
                contrast_core::WindowState::Floating
            } else {
                contrast_core::WindowState::Tiled
            },
            focus: p.focus,
            fullscreen: p.fullscreen,
            column_width: p.column_width,
        })
    }
}

/// `ConfigStore::reload` の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadOutcome {
    /// 新しい版に差し替えた。`diagnostics` は反映できなかった変更などの警告。
    Applied {
        /// 差し替え後の世代番号。
        generation: u64,
        /// 警告。
        diagnostics: Vec<ConfigDiagnostic>,
    },
    /// 設定ファイルが無い。旧版を保持した。
    Absent,
    /// 失敗した。旧版を保持した。
    Failed(Vec<ConfigDiagnostic>),
}

/// 現在の `Config` を持ち、成功した読み込みだけで丸ごと差し替える。`Policy` として使える。
pub struct ConfigStore {
    /// 現在の版。
    current: RefCell<Rc<Config>>,
    /// 世代番号（最初の版が 1）。
    generation: Cell<u64>,
    /// この世代でログに出した診断コード。
    logged: RefCell<BTreeSet<String>>,
    /// 実際に使う設定。`Env` で変わってよい部分（`Appearance`・`Motion`・`Keybind`）だけが、基準の設定（`current`）と
    /// 違うことがある。`ConfigStore` の世代・`logged`・CT050 とは別の層で、`replace` で基準に戻る。
    effective: RefCell<Rc<Settings>>,
    /// 最後に渡された `Env`（等しければ再 `render` しない）。
    last_env: RefCell<Option<crate::env::PolicyEnv>>,
}

impl ConfigStore {
    /// 最初の版で作る。世代は 1。
    pub fn new(config: Config) -> Self {
        let effective = Rc::new(config.settings.clone());
        Self {
            current: RefCell::new(Rc::new(config)),
            generation: Cell::new(1),
            logged: RefCell::new(BTreeSet::new()),
            effective: RefCell::new(effective),
            last_env: RefCell::new(None),
        }
    }

    /// 実際に使う設定（`Env` を反映したもの。`Env` を渡していなければ基準の設定と同じ）。
    pub fn settings(&self) -> Rc<Settings> {
        Rc::clone(&self.effective.borrow())
    }

    /// `Env` が変わった。関数形ルートの設定なら `Env` で設定を作り直し、`Appearance`・`Motion`・`Keybind` だけを反映する。
    /// `Env` が前回と等しければ何もしない。失敗は直前の設定を保持し、同じ `Env` では 1 回だけ警告（CT080）を返す。
    /// 世代・`logged`・CT050 には触れない（`replace` とは別の層）。
    pub fn update_env(&self, env: crate::env::PolicyEnv) -> crate::env::EnvUpdate {
        use crate::env::EnvUpdate;
        let config = self.config();
        let Some(renderer) = config.env_renderer.clone() else {
            return EnvUpdate::Unchanged;
        };
        if self.last_env.borrow().as_ref() == Some(&env) {
            return EnvUpdate::Unchanged;
        }
        *self.last_env.borrow_mut() = Some(env.clone());
        let rendered = match renderer.render(&env) {
            Ok(rendered) => rendered,
            Err(diagnostics) => {
                let detail = diagnostics
                    .first()
                    .map(|d| format!("{}: {}", d.code, d.message))
                    .unwrap_or_default();
                return EnvUpdate::Failed(ConfigDiagnostic {
                    severity: DiagnosticSeverity::Warning,
                    code: codes::ENV_RENDER_FAILED.to_owned(),
                    file: String::new(),
                    line: None,
                    column: None,
                    message: format!("Env での設定の作成に失敗した（直前の設定を保つ）: {detail}"),
                });
            }
        };
        let (merged, ignored) = config.settings.apply_env_result(&rendered);
        // `Env` で変わってはいけない項目（`Layout`・`Workspaces` など）が違っていたら、無視して警告（CT050）。
        let notes: Vec<ConfigDiagnostic> = ignored
            .iter()
            .map(|name| ConfigDiagnostic {
                severity: DiagnosticSeverity::Warning,
                code: codes::NOT_APPLIED.to_owned(),
                file: String::new(),
                line: None,
                column: None,
                message: format!(
                    "`{name}` は Env では変わらない（起動時に決まる）。Env で作った値は無視する"
                ),
            })
            .collect();
        let changed = *self.effective.borrow().as_ref() != merged;
        if changed {
            *self.effective.borrow_mut() = Rc::new(merged);
        }
        if changed || !notes.is_empty() {
            EnvUpdate::Applied(notes)
        } else {
            EnvUpdate::Unchanged
        }
    }

    /// 現在の世代番号。
    pub fn generation(&self) -> u64 {
        self.generation.get()
    }

    /// 現在のキーバインドの数。
    pub fn binding_count(&self) -> usize {
        self.settings().keybinds.len()
    }

    /// 現在の版。
    pub fn config(&self) -> Rc<Config> {
        Rc::clone(&self.current.borrow())
    }

    /// `config` に差し替える。世代を進め、ログ済みの診断コードを忘れる。
    ///
    /// ワークスペース数は起動後に変えられない（core が固定で持つ）ので、現在の数のまま採用し、
    /// 変わっていれば警告（`CT050`）を返す。
    pub fn replace(&self, mut config: Config) -> ReloadOutcome {
        let mut diagnostics = std::mem::take(&mut config.warnings);
        let current = self.workspace_count();
        if config.settings.workspaces != current {
            diagnostics.push(ConfigDiagnostic {
                severity: DiagnosticSeverity::Warning,
                code: codes::NOT_APPLIED.to_owned(),
                file: String::new(),
                line: None,
                column: None,
                message: format!(
                    "ワークスペース数 {} は再起動するまで反映されない（現在 {}）",
                    config.settings.workspaces, current
                ),
            });
            config.settings.workspaces = current;
        }
        let mode = self.workspace_mode();
        if config.settings.workspace_mode != mode {
            diagnostics.push(ConfigDiagnostic {
                severity: DiagnosticSeverity::Warning,
                code: codes::NOT_APPLIED.to_owned(),
                file: String::new(),
                line: None,
                column: None,
                message: format!(
                    "ワークスペースの方式 {:?} は再起動するまで反映されない（現在 {:?}）",
                    config.settings.workspace_mode, mode
                ),
            });
            config.settings.workspace_mode = mode;
        }
        *self.effective.borrow_mut() = Rc::new(config.settings.clone());
        *self.last_env.borrow_mut() = None;
        *self.current.borrow_mut() = Rc::new(config);
        self.logged.borrow_mut().clear();
        let generation = self.generation.get().saturating_add(1);
        self.generation.set(generation);
        ReloadOutcome::Applied {
            generation,
            diagnostics,
        }
    }

    /// `loader` で `path` を読み直し、成功したときだけ差し替える。
    pub fn reload(&self, loader: &dyn ConfigLoader, path: &Path) -> ReloadOutcome {
        match loader.load(path) {
            LoadOutcome::Loaded(config) => self.replace(*config),
            LoadOutcome::Absent => ReloadOutcome::Absent,
            LoadOutcome::Failed(diagnostics) => ReloadOutcome::Failed(diagnostics),
        }
    }

    /// 診断を、この世代でまだ出していないコードならログに出す。
    fn log_once(&self, diagnostic: &ConfigDiagnostic) {
        if self.logged.borrow_mut().insert(diagnostic.code.clone()) {
            tracing::warn!("{diagnostic}（この呼び出しだけ既定の配置にした）");
        }
    }
}

/// 起動時の読み込みの結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupStatus {
    /// 設定ファイルのパスを決められなかった（`HOME` も無いなど）。既定値で起動する。
    NoPath,
    /// 設定ファイルが無い。既定値で起動する（エラーではない）。
    Absent,
    /// 読み込めた。
    Loaded,
    /// 失敗した。既定値で起動する。診断は 1 件以上。
    Failed(Vec<ConfigDiagnostic>),
}

/// 起動時の設定を決める。読めなければ（無い・壊れている）`builtin` を使う。どの場合も落ちない。
pub fn load_or_builtin(
    loader: &dyn ConfigLoader,
    path: Option<&Path>,
    builtin: Config,
) -> (Config, StartupStatus) {
    let Some(path) = path else {
        return (builtin, StartupStatus::NoPath);
    };
    match loader.load(path) {
        LoadOutcome::Loaded(config) => (*config, StartupStatus::Loaded),
        LoadOutcome::Absent => (builtin, StartupStatus::Absent),
        LoadOutcome::Failed(diagnostics) => (builtin, StartupStatus::Failed(diagnostics)),
    }
}

impl Policy for ConfigStore {
    fn appearance(&self) -> crate::Appearance {
        self.settings().appearance.clone()
    }

    fn layout_settings(&self) -> crate::LayoutSettings {
        self.settings().layout.clone()
    }

    fn cursor_settings(&self) -> crate::CursorSettings {
        self.settings().cursor.clone()
    }

    fn screenshot_path(&self) -> Option<String> {
        self.settings().screenshot_path.clone()
    }

    fn hotkey_overlay_settings(&self) -> crate::HotkeyOverlaySettings {
        self.settings().hotkey_overlay
    }

    fn input_settings(&self) -> crate::InputSettings {
        self.settings().input
    }

    fn startup_commands(&self) -> Vec<crate::StartupCommand> {
        self.settings().startup.clone()
    }

    fn data_control_clients(&self) -> Vec<String> {
        self.settings().data_control_clients.clone()
    }

    fn environment(&self) -> Vec<(String, String)> {
        self.config()
            .settings
            .environment
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    fn background(&self) -> [f32; 4] {
        self.settings().appearance.background
    }

    fn border_width(&self) -> i32 {
        self.settings().appearance.border_width
    }

    fn focus_border_color(&self) -> Option<[f32; 4]> {
        self.settings().appearance.tiers.focus_ring()
    }

    fn unfocused_border_color(&self) -> Option<[f32; 4]> {
        self.settings().appearance.tiers.unfocused_ring()
    }

    fn tiers(&self) -> crate::tier::Tiers {
        self.settings().appearance.tiers.clone()
    }

    fn motion(&self) -> Option<crate::motion::MotionSettings> {
        self.settings().motion
    }

    fn workspace_count(&self) -> u8 {
        self.settings().workspaces
    }

    fn workspace_mode(&self) -> contrast_core::WorkspaceMode {
        self.settings().workspace_mode
    }

    fn mod_key(&self) -> crate::settings::ModKey {
        self.settings().mod_key
    }

    fn keybindings(&self) -> Vec<crate::settings::KeyBinding> {
        self.settings().keybinds.clone()
    }

    fn layout(&self, area: Rect, count: usize) -> Vec<Rect> {
        if count == 0 {
            return Vec::new();
        }
        // 幅が窓数に足りないときは全窓を作業領域いっぱいに重ねる（プロトタイプの割り切り）。
        area.split_columns(count)
            .unwrap_or_else(|_| vec![area; count])
    }

    fn place_window(&self, meta: &WindowMeta, focused_workspace: Option<u8>) -> WindowPlacement {
        let config = self.config();
        let (placement, diagnostic) = config.place(meta, focused_workspace);
        if let Some(d) = diagnostic {
            self.log_once(&d);
        }
        placement
    }

    fn resolve_key(&self, mods: Mods, keysym: &str) -> Option<Action> {
        self.settings().resolve_key(mods, keysym)
    }

    fn resolve_key_in_mode(&self, mode: &str, mods: Mods, keysym: &str) -> Option<Action> {
        self.settings().resolve_key_in(mode, mods, keysym)
    }

    fn mode_fallthrough(&self, mode: &str) -> bool {
        self.settings().mode_fallthrough(mode)
    }

    fn named_workspaces(&self) -> Vec<crate::settings::NamedWorkspace> {
        self.settings().named_workspaces.clone()
    }

    fn profile(&self) -> String {
        self.config().profile.clone()
    }

    fn update_env(&self, env: crate::env::PolicyEnv) -> crate::env::EnvUpdate {
        ConfigStore::update_env(self, env)
    }

    fn key_cooldown_ms(&self, mods: Mods, keysym: &str) -> Option<u32> {
        self.settings().resolve_cooldown_ms(mods, keysym)
    }
}

#[cfg(test)]
mod tests {
    use contrast_core::WindowState;

    use super::*;

    /// 決まった結果を返すフック。
    struct Fixed(Result<Option<HookPlacement>, ConfigDiagnostic>);

    impl PlacementHook for Fixed {
        fn window_open(&self, _w: &WindowInfo) -> Result<Option<HookPlacement>, ConfigDiagnostic> {
            self.0.clone()
        }
    }

    /// 診断（位置なし）。
    fn diag(code: &str) -> ConfigDiagnostic {
        ConfigDiagnostic {
            severity: DiagnosticSeverity::Warning,
            code: code.to_owned(),
            file: String::new(),
            line: None,
            column: None,
            message: "test".to_owned(),
        }
    }

    /// フック付きの設定。
    fn with_hook(r: Result<Option<HookPlacement>, ConfigDiagnostic>) -> Config {
        Config::new(Settings::builtin(), Some(Box::new(Fixed(r))))
    }

    #[test]
    fn hook_result_is_used() {
        let c = with_hook(Ok(Some(HookPlacement {
            workspace: Some(2),
            floating: true,
            focus: false,
            fullscreen: true,
            column_width: None,
        })));
        let (p, d) = c.place(&WindowMeta::default(), Some(0));
        assert_eq!(d, None);
        assert_eq!(p.workspace, Some(2));
        assert_eq!(p.state, WindowState::Floating);
        assert!(!p.focus);
        assert!(p.fullscreen);
    }

    #[test]
    fn default_placement_is_not_fullscreen() {
        assert!(!default_placement(&WindowMeta::default()).fullscreen);
        let (p, _) = Config::builtin().place(&WindowMeta::default(), None);
        assert!(!p.fullscreen);
    }

    #[test]
    fn a_changed_workspace_mode_is_kept_at_the_running_value_with_a_warning() {
        use contrast_core::WorkspaceMode;
        let store = ConfigStore::new(Config::builtin());
        assert_eq!(store.workspace_mode(), WorkspaceMode::Fixed);
        let mut s = Settings::builtin();
        s.workspace_mode = WorkspaceMode::Dynamic;
        let outcome = store.replace(Config::new(s, None));
        let ReloadOutcome::Applied { diagnostics, .. } = outcome else {
            panic!("applied");
        };
        assert!(diagnostics.iter().any(|d| d.code == codes::NOT_APPLIED));
        // 起動時の方式のまま。
        assert_eq!(store.workspace_mode(), WorkspaceMode::Fixed);
    }

    #[test]
    fn the_store_exposes_the_motion_of_the_current_config() {
        use crate::motion::{MotionKind, MotionSettings, MotionSpec};
        let store = ConfigStore::new(Config::builtin());
        assert_eq!(Policy::motion(&store), None);
        let mut m = MotionSettings::default();
        m.set(
            MotionKind::Focus,
            MotionSpec::default_for(MotionKind::Focus),
        );
        let mut s = Settings::builtin();
        s.motion = Some(m);
        store.replace(Config::new(s, None));
        assert_eq!(Policy::motion(&store), Some(m));
    }

    /// 呼ばれた回数を数え、`Env` の `profile` が "dim" なら角丸 9、"bad" なら失敗、"fixed" なら `Layout` を変えて返す。
    struct Renderer(Cell<u32>);

    impl crate::env::EnvRenderer for Renderer {
        fn render(&self, env: &crate::env::PolicyEnv) -> Result<Settings, Vec<ConfigDiagnostic>> {
            self.0.set(self.0.get().saturating_add(1));
            let mut s = Settings::builtin();
            match env.profile.as_str() {
                "dim" => s.appearance.radius = 9,
                "bad" => return Err(vec![diag(codes::ENV_BASELINE_FAILED)]),
                "fixed" => s.layout.gap = 7,
                _ => {}
            }
            Ok(s)
        }
    }

    fn env_store() -> (ConfigStore, Rc<Renderer>) {
        let renderer = Rc::new(Renderer(Cell::new(0)));
        let config = Config::builtin().with_env(renderer.clone(), "default".to_owned());
        (ConfigStore::new(config), renderer)
    }

    #[test]
    fn an_equal_env_is_not_rendered_again_and_a_changed_one_applies_only_the_switchable_parts() {
        use crate::env::{EnvUpdate, PolicyEnv};
        let (store, renderer) = env_store();
        let generation = store.generation();
        assert_eq!(
            store.update_env(PolicyEnv::baseline("default")),
            EnvUpdate::Unchanged
        );
        let calls = renderer.0.get();
        assert_eq!(
            store.update_env(PolicyEnv::baseline("default")),
            EnvUpdate::Unchanged
        );
        assert_eq!(renderer.0.get(), calls, "等しい Env では再 render しない");
        assert!(matches!(
            store.update_env(PolicyEnv::baseline("dim")),
            EnvUpdate::Applied(notes) if notes.is_empty()
        ));
        assert_eq!(Policy::appearance(&store).radius, 9);
        // 世代・基準の設定は動かない（`replace` とは別の層）。
        assert_eq!(store.generation(), generation);
        assert_eq!(store.config().settings.appearance.radius, 0);
        // `Layout` は Env で変わらない。違っていたことは警告（CT050）で知らせる。
        let EnvUpdate::Applied(notes) = store.update_env(PolicyEnv::baseline("fixed")) else {
            panic!("applied");
        };
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].code, codes::NOT_APPLIED);
        assert_eq!(Policy::layout_settings(&store).gap, 0);
        // 角丸は "fixed" の Env の値（0）に戻る。
        assert_eq!(Policy::appearance(&store).radius, 0);
    }

    #[test]
    fn a_failing_env_keeps_the_previous_settings_and_warns_once() {
        use crate::env::{EnvUpdate, PolicyEnv};
        let (store, _) = env_store();
        store.update_env(PolicyEnv::baseline("dim"));
        let failed = store.update_env(PolicyEnv::baseline("bad"));
        let EnvUpdate::Failed(d) = failed else {
            panic!("failed");
        };
        assert_eq!(d.code, codes::ENV_RENDER_FAILED);
        assert_eq!(d.severity, DiagnosticSeverity::Warning);
        // 直前の設定（"dim"）を保持する。
        assert_eq!(Policy::appearance(&store).radius, 9);
        // 同じ Env では二度目の警告を出さない。
        assert_eq!(
            store.update_env(PolicyEnv::baseline("bad")),
            EnvUpdate::Unchanged
        );
    }

    #[test]
    fn a_reload_returns_to_the_base_and_forgets_the_last_env() {
        use crate::env::{EnvUpdate, PolicyEnv};
        let (store, _) = env_store();
        store.update_env(PolicyEnv::baseline("dim"));
        assert_eq!(Policy::appearance(&store).radius, 9);
        store.replace(Config::builtin());
        assert_eq!(Policy::appearance(&store).radius, 0);
        // 静的な設定（`env_renderer` なし）では何もしない。
        assert_eq!(
            store.update_env(PolicyEnv::baseline("dim")),
            EnvUpdate::Unchanged
        );
    }

    #[test]
    fn load_warnings_are_reported_when_a_config_is_applied() {
        let store = ConfigStore::new(Config::builtin());
        let config = Config::builtin().with_warnings(vec![diag(codes::NOT_APPLIED)]);
        let ReloadOutcome::Applied { diagnostics, .. } = store.replace(config) else {
            panic!("applied");
        };
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, codes::NOT_APPLIED);
    }

    #[test]
    fn startup_and_environment_come_from_the_settings() {
        let mut s = Settings::builtin();
        s.startup = vec![
            crate::StartupCommand::new("mako"),
            crate::StartupCommand::new("swaybg -i x.jpg").with_niri_ipc(),
        ];
        s.environment.insert("B".to_owned(), "2".to_owned());
        s.environment.insert("A".to_owned(), "1".to_owned());
        let store = ConfigStore::new(Config::new(s, None));
        let startup = store.startup_commands();
        let commands: Vec<&str> = startup.iter().map(|s| s.command.as_str()).collect();
        assert_eq!(commands, ["mako", "swaybg -i x.jpg"]);
        // `NIRI_SOCKET` を渡す指定は、指定したコマンドだけ。
        let ipc: Vec<bool> = startup.iter().map(|s| s.niri_ipc).collect();
        assert_eq!(ipc, [false, true]);
        // 名前順。
        assert_eq!(
            store.environment(),
            [
                ("A".to_owned(), "1".to_owned()),
                ("B".to_owned(), "2".to_owned())
            ]
        );
        assert!(
            ConfigStore::new(Config::builtin())
                .startup_commands()
                .is_empty()
        );
    }

    #[test]
    fn failed_or_out_of_range_hook_falls_back_with_diagnostic() {
        let default = default_placement(&WindowMeta::default());
        let (p, d) = with_hook(Err(diag(codes::HOOK_FAILED))).place(&WindowMeta::default(), None);
        assert_eq!(p, default);
        assert_eq!(d.map(|d| d.code), Some(codes::HOOK_FAILED.to_owned()));

        let out = HookPlacement {
            workspace: Some(4),
            floating: true,
            focus: true,
            fullscreen: false,
            column_width: None,
        };
        let (p, d) = with_hook(Ok(Some(out))).place(&WindowMeta::default(), None);
        assert_eq!(p, default);
        assert_eq!(
            d.map(|d| d.code),
            Some(codes::HOOK_WORKSPACE_RANGE.to_owned())
        );
    }

    #[test]
    fn replace_bumps_generation_and_keeps_workspace_count() {
        let store = ConfigStore::new(Config::builtin());
        assert_eq!(store.generation(), 1);
        let mut s = Settings::builtin();
        s.workspaces = 6;
        s.appearance.border_width = 8;
        let outcome = store.replace(Config::new(s, None));
        let (generation, diagnostics) = match outcome {
            ReloadOutcome::Applied {
                generation,
                diagnostics,
            } => (generation, diagnostics),
            _ => (0, Vec::new()),
        };
        assert_eq!(generation, 2);
        assert_eq!(store.generation(), 2);
        assert_eq!(store.border_width(), 8);
        assert_eq!(store.workspace_count(), 4);
        assert_eq!(
            diagnostics
                .iter()
                .map(|d| d.code.as_str())
                .collect::<Vec<_>>(),
            [codes::NOT_APPLIED]
        );
    }

    #[test]
    fn startup_falls_back_to_builtin() {
        /// 結果を決めて返す読み込み器。
        struct Fixed(fn() -> LoadOutcome);
        impl ConfigLoader for Fixed {
            fn name(&self) -> &'static str {
                "fixed"
            }
            fn load(&self, _path: &Path) -> LoadOutcome {
                (self.0)()
            }
        }
        let path = Path::new("/x");
        let (c, st) = load_or_builtin(&Fixed(|| LoadOutcome::Absent), None, Config::builtin());
        assert_eq!(
            (c.settings, st),
            (Settings::builtin(), StartupStatus::NoPath)
        );
        let (_, st) = load_or_builtin(
            &Fixed(|| LoadOutcome::Absent),
            Some(path),
            Config::builtin(),
        );
        assert_eq!(st, StartupStatus::Absent);
        let (c, st) = load_or_builtin(
            &Fixed(|| LoadOutcome::Failed(vec![diag(codes::BAD_PROP)])),
            Some(path),
            Config::builtin(),
        );
        assert_eq!(c.settings, Settings::builtin());
        assert!(matches!(st, StartupStatus::Failed(ref d) if d.len() == 1));
        let (c, st) = load_or_builtin(
            &Fixed(|| {
                let mut s = Settings::builtin();
                s.workspaces = 7;
                LoadOutcome::Loaded(Box::new(Config::new(s, None)))
            }),
            Some(path),
            Config::builtin(),
        );
        assert_eq!((c.settings.workspaces, st), (7, StartupStatus::Loaded));
    }

    #[test]
    fn failed_reload_keeps_old_version() {
        /// 常に失敗する読み込み器。
        struct Failing;
        impl ConfigLoader for Failing {
            fn name(&self) -> &'static str {
                "failing"
            }
            fn load(&self, _path: &Path) -> LoadOutcome {
                LoadOutcome::Failed(vec![diag(codes::BAD_PROP)])
            }
        }
        let store = ConfigStore::new(Config::builtin());
        let out = store.reload(&Failing, Path::new("/x"));
        assert!(matches!(out, ReloadOutcome::Failed(ref d) if d.len() == 1));
        assert_eq!(store.generation(), 1);
    }
}
