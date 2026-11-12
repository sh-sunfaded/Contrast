//! 失敗経路のテスト: 壊れた・異常な設定ファイルでも、パニックもハングもせず、診断つきで `Failed` になる。
//!
//! 読み込みは別スレッドで行い、30 秒で終わらなければハングとして失敗にする。
//! 結果は標準エラーに出す（`--nocapture` で見える）。

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use contrast_policy::{ConfigLoader, LoadOutcome};
use contrast_policy_cdl::CdlLoader;

/// 読み込み結果（スレッドをまたげる形）。
#[derive(Debug)]
enum Result3 {
    /// 読み込めた。
    Loaded,
    /// 設定ファイルが無い。
    Absent,
    /// 失敗。診断の（コード, ファイル, メッセージ）。
    Failed(Vec<(String, String, String)>),
}

/// ハングとパニックを検出しながら読み込む。
fn load(path: &Path) -> Result<Result3, String> {
    let path = path.to_path_buf();
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let r = match CdlLoader::new().load(&path) {
            LoadOutcome::Loaded(_) => Result3::Loaded,
            LoadOutcome::Absent => Result3::Absent,
            LoadOutcome::Failed(d) => {
                Result3::Failed(d.into_iter().map(|d| (d.code, d.file, d.message)).collect())
            }
        };
        let _ = tx.send(r);
    });
    match rx.recv_timeout(Duration::from_secs(30)) {
        Ok(r) => {
            let _ = handle.join();
            Ok(r)
        }
        Err(mpsc::RecvTimeoutError::Timeout) => Err("hang (30 s)".to_owned()),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err("panic in loader".to_owned()),
    }
}

/// このテスト用のディレクトリ。
fn dir(name: &str) -> Result<PathBuf, String> {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("impl_failure_paths")
        .join(name);
    fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d)
}

/// `Failed` で、診断が 1 件以上あり、全部コードを持つことを確かめ、要約を標準エラーに出す。
fn expect_failed(label: &str, r: Result3) -> Result<Vec<(String, String, String)>, String> {
    eprintln!("[{label}] {r:?}");
    match r {
        Result3::Failed(d) if !d.is_empty() && d.iter().all(|(c, _, _)| !c.is_empty()) => Ok(d),
        other => Err(format!(
            "{label}: expected Failed with diagnostics, got {other:?}"
        )),
    }
}

/// 最小の有効な設定。
const VALID: &str = "\"use cdl 1\";\nimport { Contrast } from \"host:contrast\";\nexport default (<Contrast></Contrast>);\n";

#[test]
fn huge_file_is_rejected_before_parsing() -> Result<(), String> {
    let d = dir("huge")?;
    let path = d.join("config.cdl");
    let mut text = String::from("\"use cdl 1\";\n// ");
    text.push_str(&"x".repeat(1_100_000));
    fs::write(&path, text).map_err(|e| e.to_string())?;
    let diags = expect_failed("huge", load(&path)?)?;
    assert!(
        diags.iter().any(|(_, _, m)| m.contains("上限")),
        "{diags:?}"
    );
    Ok(())
}

#[test]
fn unreadable_file_fails_with_diagnostic() -> Result<(), String> {
    let d = dir("unreadable")?;
    let path = d.join("config.cdl");
    // 前回の実行で権限を落としたままのファイルに書けるよう、先に戻す（無ければ無視）。
    let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o644));
    fs::write(&path, VALID).map_err(|e| e.to_string())?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).map_err(|e| e.to_string())?;
    if fs::read(&path).is_ok() {
        eprintln!("[unreadable] skipped: permissions are not enforced (root?)");
        return Ok(());
    }
    expect_failed("unreadable", load(&path)?)?;
    Ok(())
}

#[test]
fn non_utf8_file_fails_with_diagnostic() -> Result<(), String> {
    let d = dir("non_utf8")?;
    let path = d.join("config.cdl");
    fs::write(&path, [0xff_u8, 0xfe, 0x00, 0x41]).map_err(|e| e.to_string())?;
    expect_failed("non-utf8", load(&path)?)?;
    Ok(())
}

#[test]
fn empty_file_fails_with_diagnostic() -> Result<(), String> {
    let d = dir("empty")?;
    let path = d.join("config.cdl");
    fs::write(&path, "").map_err(|e| e.to_string())?;
    expect_failed("empty", load(&path)?)?;
    Ok(())
}

#[test]
fn symlink_import_pointing_outside_is_rejected() -> Result<(), String> {
    let outside = dir("symlink_outside")?;
    fs::write(
        outside.join("secret.cdl"),
        "\"use cdl 1\";\nexport const X = 1;\n",
    )
    .map_err(|e| e.to_string())?;
    let inner = dir("symlink_outside/inner")?;
    let link = inner.join("link.cdl");
    if !link.exists() {
        symlink(outside.join("secret.cdl"), &link).map_err(|e| e.to_string())?;
    }
    let path = inner.join("config.cdl");
    let text = "\"use cdl 1\";\nimport { Contrast } from \"host:contrast\";\nimport { X } from \"./link.cdl\";\nexport default (<Contrast></Contrast>);\n";
    fs::write(&path, text).map_err(|e| e.to_string())?;
    let diags = expect_failed("symlink", load(&path)?)?;
    assert!(
        diags.iter().any(|(_, _, m)| m.contains("外は読めない")),
        "{diags:?}"
    );
    Ok(())
}

#[test]
fn deep_import_chain_terminates() -> Result<(), String> {
    for depth in [50_usize, 300] {
        let d = dir(&format!("deep{depth}"))?;
        for i in 0..depth {
            let next = i.saturating_add(1);
            let text = if i == depth.saturating_sub(1) {
                format!("\"use cdl 1\";\nexport const V{i} = 1;\n")
            } else {
                format!(
                    "\"use cdl 1\";\nimport {{ V{next} }} from \"./f{next}.cdl\";\nexport const V{i} = V{next};\n"
                )
            };
            fs::write(d.join(format!("f{i}.cdl")), text).map_err(|e| e.to_string())?;
        }
        let entry = "\"use cdl 1\";\nimport { Contrast } from \"host:contrast\";\nimport { V0 } from \"./f0.cdl\";\nexport default (<Contrast></Contrast>);\n";
        let path = d.join("config.cdl");
        fs::write(&path, entry).map_err(|e| e.to_string())?;
        let r = load(&path)?;
        eprintln!("[deep import depth={depth}] {r:?}");
        // 読めても失敗しても構わない。パニックとハングでないことだけを確かめる（`load` が Err にする）。
    }
    Ok(())
}
