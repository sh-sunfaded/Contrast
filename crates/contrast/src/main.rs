//! Contrast の実行ファイル（nested プロトタイプ）。
//!
//! 設定ファイル（CDL）のパスは `CONTRAST_CONFIG` → `$XDG_CONFIG_HOME/contrast/config.cdl` →
//! `~/.config/contrast/config.cdl`。ファイルが無い・壊れているときは組み込みの既定値で起動し、
//! 壊れているときは診断を error でログに出す。端末（既定値の Alt+Return）は `CONTRAST_TERMINAL`、無ければ `foot`。

use std::io::IsTerminal;
use std::process::ExitCode;
use std::rc::Rc;

use contrast_compositor::ConfigSource;
use contrast_policy::{
    Config, ConfigStore, Settings, StartupStatus, load_or_builtin, resolve_config_path,
};
use contrast_policy_cdl::CdlLoader;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("contrast: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,smithay=warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(std::io::stdout().is_terminal())
        .init();

    let terminal = std::env::var("CONTRAST_TERMINAL").unwrap_or_else(|_| "foot".to_string());
    let path = resolve_config_path(
        std::env::var_os(contrast_policy::config::CONFIG_ENV),
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    );
    let default_mod_key = contrast_compositor::startup_mod_key();
    tracing::info!("config: default mod key={default_mod_key:?} (modKey in the config overrides)");
    let loader = CdlLoader::new().with_default_mod_key(default_mod_key);
    let builtin = Config::new(Settings::builtin_with_terminal(terminal), None);
    let (config, status) = load_or_builtin(&loader, path.as_deref(), builtin);

    let store = Rc::new(ConfigStore::new(config));
    let loaded = match &status {
        StartupStatus::Loaded => "true",
        StartupStatus::NoPath | StartupStatus::Absent => "absent",
        StartupStatus::Failed(_) => "failed",
    };
    tracing::info!(
        "config: path={} loaded={} generation={} bindings={}",
        path.as_deref()
            .map_or_else(|| "<none>".to_string(), |p| p.display().to_string()),
        loaded,
        store.generation(),
        store.binding_count()
    );
    if let StartupStatus::Failed(diagnostics) = &status {
        tracing::error!(
            "config: using built-in defaults ({} errors)",
            diagnostics.len()
        );
        for d in diagnostics {
            tracing::error!("{d}");
        }
    }

    contrast_compositor::run(
        store,
        ConfigSource {
            loader: Box::new(loader),
            path,
        },
    )
}
