//! `ModuleResolver` の実装。
//!
//! - `FsResolver`: 設定ファイルのあるディレクトリの内側だけを読む（CD6）。
//!   `canonicalize` の後で判定するので、`..` や symlink で外へ出る import は断る。
//!   読む前にサイズを確認し、上限を超えるファイルは読まない。
//! - `Recording`: 任意の `ModuleResolver` を包み、読んだソースを覚える（診断の行・列の計算用）。

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use cdl_host::{ModuleResolver, Source};

/// 設定ディレクトリに閉じたファイルシステムのリゾルバ。`id` は正準な絶対パス。
#[derive(Debug)]
pub(crate) struct FsResolver {
    /// 正準化した設定ディレクトリ。この内側のファイルだけを読む。
    base: PathBuf,
    /// 1 ファイルの最大バイト数。
    max_bytes: u64,
}

impl FsResolver {
    /// エントリファイル `entry` から作る。戻り値の文字列は、`Engine::load` に渡すエントリの id。
    pub(crate) fn for_entry(entry: &Path, max_bytes: u64) -> Result<(Self, String), String> {
        let entry = entry
            .canonicalize()
            .map_err(|e| format!("{}: {e}", entry.display()))?;
        let base = entry
            .parent()
            .ok_or_else(|| format!("{}: 親ディレクトリがない", entry.display()))?
            .to_path_buf();
        let id = path_id(&entry)?;
        Ok((Self { base, max_bytes }, id))
    }

    /// `path` を正準化し、設定ディレクトリの内側で、通常ファイルで、サイズ上限以下なら読む。
    fn read(&self, path: &Path) -> Result<Source, String> {
        let path = path
            .canonicalize()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if !path.starts_with(&self.base) {
            return Err(format!(
                "{}: 設定ディレクトリ {} の外は読めない",
                path.display(),
                self.base.display()
            ));
        }
        let meta = fs::metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if !meta.is_file() {
            return Err(format!("{}: 通常のファイルではない", path.display()));
        }
        if meta.len() > self.max_bytes {
            return Err(format!(
                "{}: {} バイトは上限 {} バイトを超える",
                path.display(),
                meta.len(),
                self.max_bytes
            ));
        }
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Source {
            id: path_id(&path)?,
            text,
        })
    }
}

/// パスを id の文字列にする。UTF-8 でないパスは断る（id を決定的に保つため）。
fn path_id(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{}: UTF-8 でないパスは扱えない", path.display()))
}

impl ModuleResolver for FsResolver {
    fn resolve(&self, importer: Option<&str>, specifier: &str) -> Result<Source, String> {
        let path = match importer {
            None => PathBuf::from(specifier),
            Some(from) => Path::new(from)
                .parent()
                .unwrap_or(Path::new(""))
                .join(specifier),
        };
        self.read(&path)
    }
}

/// 読んだソースを覚えるリゾルバ。
pub(crate) struct Recording<'a> {
    /// 実際に読むリゾルバ。
    inner: &'a dyn ModuleResolver,
    /// 読めたソースの本文（id → 本文）。
    seen: RefCell<BTreeMap<String, String>>,
}

impl<'a> Recording<'a> {
    /// `inner` を包む。
    pub(crate) fn new(inner: &'a dyn ModuleResolver) -> Self {
        Self {
            inner,
            seen: RefCell::new(BTreeMap::new()),
        }
    }

    /// ここまでに読めたソースの本文（id → 本文）を取り出す。
    pub(crate) fn into_sources(self) -> BTreeMap<String, String> {
        self.seen.into_inner()
    }
}

impl ModuleResolver for Recording<'_> {
    fn resolve(&self, importer: Option<&str>, specifier: &str) -> Result<Source, String> {
        let source = self.inner.resolve(importer, specifier)?;
        if let Ok(mut seen) = self.seen.try_borrow_mut() {
            seen.insert(source.id.clone(), source.text.clone());
        }
        Ok(source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
    }

    #[test]
    fn reads_inside_base() -> Result<(), String> {
        let (resolver, id) = FsResolver::for_entry(&manifest(), 1 << 20)?;
        let entry = resolver.resolve(None, &id)?;
        assert_eq!(entry.id, id);
        let same = resolver.resolve(Some(&id), "./Cargo.toml")?;
        assert_eq!(same.id, id);
        Ok(())
    }

    #[test]
    fn rejects_escape_from_base() -> Result<(), String> {
        let (resolver, id) = FsResolver::for_entry(&manifest(), 1 << 20)?;
        let result = resolver.resolve(Some(&id), "../contrast-core/Cargo.toml");
        assert!(
            matches!(&result, Err(e) if e.contains("外は読めない")),
            "{result:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_oversize_before_reading() -> Result<(), String> {
        let (resolver, id) = FsResolver::for_entry(&manifest(), 4)?;
        let result = resolver.resolve(None, &id);
        assert!(
            matches!(&result, Err(e) if e.contains("上限")),
            "{result:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_directory() -> Result<(), String> {
        let (resolver, id) = FsResolver::for_entry(&manifest(), 1 << 20)?;
        let result = resolver.resolve(Some(&id), "./src");
        assert!(
            matches!(&result, Err(e) if e.contains("通常のファイルではない")),
            "{result:?}"
        );
        Ok(())
    }

    #[test]
    fn missing_file_is_an_error_value() -> Result<(), String> {
        let (resolver, id) = FsResolver::for_entry(&manifest(), 1 << 20)?;
        assert!(resolver.resolve(Some(&id), "./no-such.cdl").is_err());
        Ok(())
    }
}
