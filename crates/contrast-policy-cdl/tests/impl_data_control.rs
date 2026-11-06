//! `DataControlClient`（wlr-data-control を使ってよいクライアントの一覧）の変換のテスト: 既定、置き換え、誤りの診断。

use std::fs;
use std::path::{Path, PathBuf};

use contrast_policy::{Config, ConfigLoader, DEFAULT_DATA_CONTROL_CLIENTS, LoadOutcome};
use contrast_policy_cdl::CdlLoader;

/// 一時ファイルを書いて、そのパスを返す。`body` は `<Contrast>` の中身。
/// テストは並列に走るので、テストの名前でディレクトリを分ける。
fn write_temp(dir: &str, body: &str) -> Result<PathBuf, String> {
    let test = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .to_owned();
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("impl_data_control")
        .join(test)
        .join(dir);
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let path = base.join("config.cdl");
    let text = format!(
        "\"use cdl 1\";\n\
         import {{ Contrast, DataControlClient }} from \"host:contrast\";\n\
         export default (\n  <Contrast>\n{body}\n  </Contrast>\n);\n"
    );
    fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(path)
}

/// 読み込んで、設定か診断コードの並びを返す。
fn load(dir: &str, body: &str) -> Result<Result<Config, Vec<String>>, String> {
    let path = write_temp(dir, body)?;
    Ok(match CdlLoader::new().load(&path) {
        LoadOutcome::Loaded(c) => Ok(*c),
        LoadOutcome::Failed(d) => Err(d.iter().map(|d| d.code.clone()).collect()),
        LoadOutcome::Absent => Err(vec!["absent".to_owned()]),
    })
}

/// 失敗する設定の診断コード。成功したらエラー。
fn codes_of(dir: &str, body: &str) -> Result<Vec<String>, String> {
    match load(dir, body)? {
        Ok(_) => Err(format!("誤りにならない: {body}")),
        Err(codes) => Ok(codes),
    }
}

#[test]
fn the_default_list_is_used_when_none_is_written() -> Result<(), String> {
    let c = load("none", "")?.map_err(|d| format!("{d:?}"))?;
    assert_eq!(
        c.settings.data_control_clients,
        DEFAULT_DATA_CONTROL_CLIENTS
    );
    assert_eq!(
        c.settings.data_control_clients,
        [
            "ironbar",
            "wl-copy",
            "wl-paste",
            "cliphist",
            "wl-clip-persist"
        ]
    );
    Ok(())
}

#[test]
fn written_clients_replace_the_default_list_in_order() -> Result<(), String> {
    let body = r#"
    <DataControlClient name="wl-copy" />
    <DataControlClient name="  my-clipper  " />"#;
    let c = load("replace", body)?.map_err(|d| format!("{d:?}"))?;
    assert_eq!(c.settings.data_control_clients, ["wl-copy", "my-clipper"]);
    Ok(())
}

#[test]
fn bad_names_are_ct003() -> Result<(), String> {
    for (dir, name) in [
        ("empty", ""),
        ("blank", "   "),
        ("slash", "/usr/bin/wl-copy"),
        ("space", "wl copy"),
    ] {
        let body = format!(r#"<DataControlClient name="{name}" />"#);
        assert_eq!(codes_of(dir, &body)?, ["CT003"], "{name:?}");
    }
    let dup = r#"<DataControlClient name="a" /><DataControlClient name="a" />"#;
    assert_eq!(codes_of("dup", dup)?, ["CT003"]);
    // `name` が無いのは CDL のスキーマ適合（E4002）。
    assert_eq!(codes_of("missing", "<DataControlClient />")?, ["E4002"]);
    Ok(())
}
