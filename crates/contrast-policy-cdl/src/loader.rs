//! `ConfigLoader` の実装 `CdlLoader`。
//!
//! 流れは、ファイルの有無 → リゾルバ → `Engine::load`（構文・名前解決・型検査・スキーマ適合・評価）→
//! `to_settings`（値木の変換）→ `Config`。どの段階の失敗も `ConfigDiagnostic` の一覧にして返し、落ちない。

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::Path;
use std::rc::Rc;

use cdl_host::{Engine, Limits, ModuleResolver, Node, Program};
use contrast_policy::{
    Config, ConfigDiagnostic, ConfigLoader, EnvRenderer, LoadOutcome, ModKey, PolicyEnv, Settings,
    codes,
};

use crate::convert::to_settings;
use crate::diag::{convert_all, internal};
use crate::env_value::env_value;
use crate::hook::CdlPolicy;
use crate::resolver::{FsResolver, Recording};
use crate::schema::contrast_schema;

/// CDL の設定ファイルを読む読み込み器。
#[derive(Debug, Clone, Default)]
pub struct CdlLoader {
    /// プロファイル名（`Env.profile`）。`None` なら読み込みのたびに環境変数 `CONTRAST_PROFILE` と `profile` ファイルから決める。
    profile: Option<String>,
    /// CDL の資源上限。
    limits: Limits,
    /// `modKey` を省略した設定の `mod`。呼び出し側が、環境変数・バックエンドの既定から決めて渡す。
    default_mod_key: ModKey,
}

impl CdlLoader {
    /// 既定の上限で作る。
    pub fn new() -> Self {
        Self::default()
    }

    /// 上限を指定して作る。
    pub fn with_limits(limits: Limits) -> Self {
        Self {
            limits,
            ..Self::default()
        }
    }

    /// プロファイル名を固定する（テスト用。通常は読み込みのたびに環境変数とファイルから決める）。
    #[must_use]
    pub fn with_profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = Some(profile.into());
        self
    }

    /// `modKey` を省略した設定の `mod` を指定する（既定は Alt）。設定に `modKey` があればそちらが優先。
    #[must_use]
    pub fn with_default_mod_key(mut self, mod_key: ModKey) -> Self {
        self.default_mod_key = mod_key;
        self
    }
}

impl ConfigLoader for CdlLoader {
    fn name(&self) -> &'static str {
        "cdl"
    }

    fn load(&self, path: &Path) -> LoadOutcome {
        match std::fs::metadata(path) {
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => return LoadOutcome::Absent,
            Err(e) => {
                return LoadOutcome::Failed(vec![internal(
                    codes::FILE_UNREADABLE,
                    format!("{}: {e}", path.display()),
                )]);
            }
        }
        let (resolver, entry) =
            match FsResolver::for_entry(path, u64::from(self.limits.max_source_bytes)) {
                Ok(v) => v,
                Err(message) => {
                    return LoadOutcome::Failed(vec![internal(codes::FILE_UNREADABLE, message)]);
                }
            };
        let profile = self.profile.clone().unwrap_or_else(current_profile);
        match load_config(
            &resolver,
            &entry,
            self.limits,
            self.default_mod_key,
            &profile,
        ) {
            Ok(config) => LoadOutcome::Loaded(Box::new(config)),
            Err(diags) => LoadOutcome::Failed(diags),
        }
    }
}

/// 現在のプロファイル名: 環境変数 `CONTRAST_PROFILE`、無ければ `profile` ファイルの最初の行、無ければ `"default"`。
fn current_profile() -> String {
    let env = std::env::var("CONTRAST_PROFILE").ok();
    let file = contrast_policy::profile_file_path(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    )
    .and_then(|path| std::fs::read_to_string(path).ok());
    contrast_policy::resolve_profile(env.as_deref(), file.as_deref())
}

/// `Env` で設定を作る（関数形ルートの設定。`render` + 変換）。
struct CdlEnvRenderer {
    /// 読み込んだプログラム。
    program: Rc<Program>,
    /// 診断の位置の計算に使うソース（id → 本文）。
    sources: BTreeMap<String, String>,
    /// `modKey` を省略した設定の `mod`。
    default_mod_key: ModKey,
}

impl EnvRenderer for CdlEnvRenderer {
    fn render(&self, env: &PolicyEnv) -> Result<Settings, Vec<ConfigDiagnostic>> {
        let tree = self
            .program
            .render(&env_value(env))
            .map_err(|diags| convert_all(&diags, &self.sources))?;
        to_settings(Node::new(&tree), &self.sources, self.default_mod_key)
    }
}

/// 代表の `Env` ごとの失敗を CT081 にする（元のコードはメッセージの先頭に残す。位置はそのまま）。
fn as_baseline_failure(diags: Vec<ConfigDiagnostic>, env: &PolicyEnv) -> Vec<ConfigDiagnostic> {
    let focus = env.focus.as_ref().map_or("フォーカスなし".to_owned(), |f| {
        format!("フォーカスあり（{}）", f.state.as_str())
    });
    diags
        .into_iter()
        .map(|mut d| {
            d.message = format!("[{}] {}（代表の Env: {focus}）", d.code, d.message);
            d.code = codes::ENV_BASELINE_FAILED.to_owned();
            d
        })
        .collect()
}

/// `resolver` からエントリ `entry` を読み、`Config` にする。`profile` は `Env.profile`。
pub(crate) fn load_config(
    resolver: &dyn ModuleResolver,
    entry: &str,
    limits: Limits,
    default_mod_key: ModKey,
    profile: &str,
) -> Result<Config, Vec<ConfigDiagnostic>> {
    let schema = contrast_schema().map_err(|errors| {
        errors
            .into_iter()
            .map(|e| internal(codes::TREE_MISMATCH, format!("スキーマの登録に失敗: {e}")))
            .collect::<Vec<_>>()
    })?;
    let recording = Recording::new(resolver);
    let engine = Engine::new().with_schema(schema).with_limits(limits);
    let program = match engine.load(&recording, entry) {
        Ok(program) => program,
        Err(diags) => return Err(convert_all(&diags, &recording.into_sources())),
    };
    let sources = recording.into_sources();
    if program.is_function_root() {
        return load_function_root(program, sources, default_mod_key, profile);
    }
    let Some(root) = program.root() else {
        return Err(vec![internal(
            codes::TREE_MISMATCH,
            "ルートの要素を読めない".to_owned(),
        )]);
    };
    let settings = to_settings(root, &sources, default_mod_key)?;
    let hook = CdlPolicy::new(Rc::new(program), sources);
    Ok(Config::new(settings, Some(Box::new(hook))))
}

/// 関数形ルート（`function App(env: Env)`）の設定。`check` は本体を評価しないので、`load` の後に代表の `Env`
/// （フォーカスなし、フォーカスあり ×3 状態）で `render` と変換を通し、失敗は CT081。基準（最初）の `Env` の設定を
/// `Config.settings` にし、`Env` ごとの設定は `Config.env_renderer` で作る。
fn load_function_root(
    program: Program,
    sources: BTreeMap<String, String>,
    default_mod_key: ModKey,
    profile: &str,
) -> Result<Config, Vec<ConfigDiagnostic>> {
    let program = Rc::new(program);
    let renderer = CdlEnvRenderer {
        program: Rc::clone(&program),
        sources: sources.clone(),
        default_mod_key,
    };
    let mut base: Option<Settings> = None;
    let mut errors: Vec<ConfigDiagnostic> = Vec::new();
    for env in PolicyEnv::representatives(profile) {
        match renderer.render(&env) {
            Ok(settings) => {
                if base.is_none() {
                    base = Some(settings);
                }
            }
            Err(diags) => {
                for d in as_baseline_failure(diags, &env) {
                    // 同じ位置・同じ元の誤りは 1 件にする（どの `Env` でも出る誤りを重ねない）。
                    let same = errors
                        .iter()
                        .any(|e| e.file == d.file && e.line == d.line && e.column == d.column);
                    if !same {
                        errors.push(d);
                    }
                }
            }
        }
    }
    let Some(settings) = base.filter(|_| errors.is_empty()) else {
        return Err(errors);
    };
    let hook = CdlPolicy::new(program, sources);
    Ok(Config::new(settings, Some(Box::new(hook))).with_env(Rc::new(renderer), profile.to_owned()))
}
