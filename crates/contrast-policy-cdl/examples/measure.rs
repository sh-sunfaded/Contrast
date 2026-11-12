//! 所要時間と資源の測定（assert はしない。数値を標準出力に出す）。
//!
//! 使い方: `cargo run --release -p contrast-policy-cdl --example measure -- <シナリオ> <作業ディレクトリ> [N]`
//!
//! シナリオ（1 プロセスにつき 1 つ。ピークメモリ `VmHWM` はプロセス全体の値なので混ぜない）:
//! - `example`: `examples/config.cdl` の load の時間、フック 1 回の時間（10,000 回の平均と最大）、同期リロードの停止時間
//! - `steps N`: 長さ N の配列の二重 `map`（評価ステップが N² に比例）
//! - `nodes N`: `Keybind` を N 個作る（要素数が N に比例）
//! - `string`: 64 KiB に近い文字列
//! - `nesting`: 入れ子の深さ 120 の配列
//!
//! 評価ステップ数は `cdl-host` が公開していないので測れない。代わりに N を変えて上限（E5xxx）に当たる境界を探す。

use std::error::Error;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use contrast_core::WindowMeta;
use contrast_policy::{Config, ConfigLoader, ConfigStore, LoadOutcome};
use contrast_policy_cdl::CdlLoader;

/// `/proc/self/status` の `VmHWM`（ピークの常駐メモリ、KiB）。
fn peak_rss_kib() -> String {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .map(|l| l.trim_start_matches("VmHWM:").trim().to_owned())
        })
        .unwrap_or_else(|| "?".to_owned())
}

/// ミリ秒（小数）。
fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// 読み込み結果の要約。
fn summary(outcome: &LoadOutcome) -> String {
    match outcome {
        LoadOutcome::Loaded(c) => format!(
            "Loaded keybinds={} workspaces={}",
            c.settings.keybinds.len(),
            c.settings.workspaces
        ),
        LoadOutcome::Absent => "Absent".to_owned(),
        LoadOutcome::Failed(d) => format!(
            "Failed diagnostics={} first={}",
            d.len(),
            d.first().map_or_else(String::new, |d| format!(
                "{}: {}",
                d.code,
                d.message.chars().take(100).collect::<String>()
            ))
        ),
    }
}

/// 設定ファイルの前書き。
const HEAD: &str = "\"use cdl 1\";\nimport { Contrast, Keybind } from \"host:contrast\";\n";

/// `body`（`<Contrast>` の中身）と追加の宣言から設定を組み立てる。
fn doc(decls: &str, body: &str) -> String {
    format!("{HEAD}{decls}\nexport default (\n  <Contrast>\n{body}\n  </Contrast>\n);\n")
}

/// シナリオの設定ファイルの本文。
fn scenario_text(name: &str, n: usize) -> Result<String, Box<dyn Error>> {
    let mut decls = String::new();
    match name {
        "steps" => {
            let items: Vec<String> = (0..n).map(|i| i.to_string()).collect();
            writeln!(decls, "const xs = [{}];", items.join(","))?;
            writeln!(
                decls,
                "const total = xs.map((a) => xs.map((b) => a + b).length).length;"
            )?;
            Ok(doc(
                &decls,
                "    <Keybind keys=\"mod+a\" action=\"workspace\" workspace={total > 0 ? 1 : 2} />",
            ))
        }
        "nodes" => {
            let items: Vec<String> = (0..n).map(|i| i.to_string()).collect();
            writeln!(decls, "const xs = [{}];", items.join(","))?;
            Ok(doc(
                &decls,
                "    {xs.map((i) => <Keybind keys={`mod+k${i}`} action=\"close\" />)}",
            ))
        }
        "string" => {
            let long = "a".repeat(60_000);
            Ok(doc(
                "",
                &format!("    <Keybind keys=\"mod+a\" action=\"spawn\" command=\"{long}\" />"),
            ))
        }
        "nesting" => {
            let depth = 120;
            let open = "[".repeat(depth);
            let close = "]".repeat(depth);
            Ok(doc(
                &format!("const deep = {open}1{close};"),
                "    <Keybind keys=\"mod+a\" action=\"close\" />",
            ))
        }
        other => Err(format!("unknown scenario {other}").into()),
    }
}

/// 設定を読んで時間と要約を出す。
fn timed_load(loader: &CdlLoader, path: &Path) -> Duration {
    let t = Instant::now();
    let outcome = loader.load(path);
    let d = t.elapsed();
    println!("load: {:.2} ms  {}", ms(d), summary(&outcome));
    d
}

/// `example` シナリオ。
fn example(dir: &Path) -> Result<(), Box<dyn Error>> {
    let _ = dir;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/config.cdl");
    let loader = CdlLoader::new();
    // 初回（ファイルのキャッシュなし）と 2 回目以降。
    for i in 0..5 {
        print!("run {i}: ");
        timed_load(&loader, &path);
    }
    let LoadOutcome::Loaded(config) = loader.load(&path) else {
        return Err("config.cdl did not load".into());
    };
    let mut meta = WindowMeta::default();
    meta.app_id = Some("firefox".to_owned());
    let (mut total, mut max) = (Duration::ZERO, Duration::ZERO);
    let runs: u32 = 10_000;
    for _ in 0..runs {
        let t = Instant::now();
        let _ = config.place(&meta, Some(0));
        let d = t.elapsed();
        total = total.saturating_add(d);
        max = max.max(d);
    }
    println!(
        "hook windowOpen x{runs}: mean {:.1} us  max {:.1} us",
        ms(total) * 1000.0 / f64::from(runs),
        ms(max) * 1000.0
    );
    // 同期リロード: ConfigStore::reload（読み込み + 変換 + 差し替え）の所要時間 = イベントループの停止時間。
    let store = ConfigStore::new(Config::builtin());
    let mut worst = Duration::ZERO;
    for _ in 0..20 {
        let t = Instant::now();
        let _ = store.reload(&loader, &path);
        worst = worst.max(t.elapsed());
    }
    println!("sync reload x20: worst {:.2} ms", ms(worst));
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let name = args.next().ok_or("scenario required")?;
    let dir = PathBuf::from(args.next().ok_or("dir required")?);
    let n: usize = args.next().map_or(Ok(0), |s| s.parse())?;
    if name == "example" {
        example(&dir)?;
    } else {
        let text = scenario_text(&name, n)?;
        let path = dir.join(format!("measure-{name}-{n}.cdl"));
        std::fs::write(&path, &text)?;
        println!("scenario {name} n={n} source={} bytes", text.len());
        timed_load(&CdlLoader::new(), &path);
    }
    println!("peak RSS (VmHWM): {}", peak_rss_kib());
    Ok(())
}
