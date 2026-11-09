//! スクリーンショットの保存: RGBA の画素を PNG にし、保存先（`~` と strftime の `%Y` など）を決めて書く。
//!
//! PNG は圧縮しない（deflate の「格納」ブロック）自前の符号化。依存を増やさないため。
//! 画素の読み出し（GL）は `render` にある。この module は時計も GL も触らない
//! （保存先の日時の整形だけ、`date` コマンドを呼ぶ）。

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 保存の失敗。
#[derive(Debug)]
pub enum SaveError {
    /// 画素の数が `幅 × 高さ × 4` と合わない。
    BadSize,
    /// 保存先の日時の整形（`date`）に失敗した。
    Date(String),
    /// ディレクトリを作る・ファイルを書くのに失敗した。
    Io(String),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadSize => write!(f, "pixel data does not match the size"),
            Self::Date(e) => write!(f, "cannot format the screenshot path: {e}"),
            Self::Io(e) => write!(f, "cannot write the screenshot: {e}"),
        }
    }
}

impl std::error::Error for SaveError {}

/// CRC-32（PNG のチャンクの検査値）。
fn crc32(chunks: &[&[u8]]) -> u32 {
    let mut crc = u32::MAX;
    for chunk in chunks {
        for byte in *chunk {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
    }
    !crc
}

/// Adler-32（zlib の検査値）。
fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1_u32, 0_u32);
    for byte in data {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// PNG のチャンクを `out` に足す。
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(&[kind, data]).to_be_bytes());
}

/// 8 ビット RGBA（上の行から）の画素を、圧縮しない PNG にする。
///
/// # Errors
/// `rgba` の長さが `width × height × 4` でない、または 0 サイズ。
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, SaveError> {
    let row = (width as usize).checked_mul(4).ok_or(SaveError::BadSize)?;
    let total = row.checked_mul(height as usize).ok_or(SaveError::BadSize)?;
    if width == 0 || height == 0 || rgba.len() != total {
        return Err(SaveError::BadSize);
    }
    // 行ごとにフィルタの種類 0（なし）の 1 バイトを前に付ける。
    let mut raw = Vec::with_capacity(total + height as usize);
    for line in rgba.chunks_exact(row) {
        raw.push(0);
        raw.extend_from_slice(line);
    }
    // zlib: ヘッダ + 格納ブロック（1 つ 65535 バイトまで）+ Adler-32。
    let mut z = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65535).peekable();
    while let Some(block) = blocks.next() {
        z.push(u8::from(blocks.peek().is_none()));
        let len = block.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    // 8 ビット、色の種類 6（RGBA）、圧縮 0、フィルタ 0、インターレースなし。
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

/// 先頭の `~` を `home` にする（`~/…` と `~` だけ）。
fn expand_tilde(template: &str, home: Option<&str>) -> String {
    match (template.strip_prefix('~'), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
            format!("{home}{rest}")
        }
        _ => template.to_owned(),
    }
}

/// 保存先を決める。`~` を展開し、`%` を含むなら `date` で strftime と同じに整形する。
///
/// # Errors
/// `date` を起動できない、または失敗した。
pub fn resolve_path(template: &str, home: Option<&str>) -> Result<PathBuf, SaveError> {
    let expanded = expand_tilde(template, home);
    if !expanded.contains('%') {
        return Ok(PathBuf::from(expanded));
    }
    let output = Command::new("date")
        .arg(format!("+{expanded}"))
        .output()
        .map_err(|e| SaveError::Date(e.to_string()))?;
    if !output.status.success() {
        return Err(SaveError::Date(format!(
            "date exited with {}",
            output.status
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(PathBuf::from(text.trim_end_matches('\n')))
}

/// `png` を `path` に書く。親のディレクトリを作る。同じ名前のファイルがあれば上書きしない。
///
/// # Errors
/// ディレクトリやファイルを作れない、既にある。
pub fn write_new(path: &Path, png: &[u8]) -> Result<(), SaveError> {
    use std::io::Write;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)
            .map_err(|e| SaveError::Io(format!("{}: {e}", dir.display())))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| SaveError::Io(format!("{}: {e}", path.display())))?;
    file.write_all(png)
        .map_err(|e| SaveError::Io(format!("{}: {e}", path.display())))
}

/// 画素を PNG にして保存先に書く。書いた場所を返す。
///
/// # Errors
/// 符号化・保存先の整形・書き込みの失敗。
/// 撮った絵を、niri の `save_screenshot` と同じく扱う: PNG にしてクリップボード（`image/png`）に入れ、
/// `write_to_disk` で保存先が有効なら別スレッドでファイルに書き、通知（`org.freedesktop.Notifications`）を出す。
pub fn deliver(
    state: &mut crate::state::Contrast,
    width: u32,
    height: u32,
    rgba: &[u8],
    write_to_disk: bool,
) {
    let png = match encode_png(width, height, rgba) {
        Ok(png) => png,
        Err(e) => {
            tracing::warn!("screenshot: {e}");
            return;
        }
    };
    let png: std::sync::Arc<[u8]> = std::sync::Arc::from(png.into_boxed_slice());
    smithay::wayland::selection::data_device::set_data_device_selection(
        &state.display_handle,
        &state.seat,
        vec![String::from("image/png")],
        png.clone(),
    );
    tracing::info!(
        width,
        height,
        "screenshot: copied to the clipboard as image/png"
    );
    let template = write_to_disk
        .then(|| state.policy.screenshot_path())
        .flatten();
    if write_to_disk && template.is_none() {
        tracing::info!("screenshot: screenshot-path is disabled, not saving");
    }
    let home = std::env::var("HOME").ok();
    let spawned = std::thread::Builder::new()
        .name("screenshot-save".into())
        .spawn(move || {
            let mut saved = None;
            if let Some(template) = template {
                match resolve_path(&template, home.as_deref())
                    .and_then(|p| write_new(&p, &png).map(|()| p))
                {
                    Ok(path) => {
                        tracing::info!(path = %path.display(), "screenshot: saved");
                        saved = Some(path);
                    }
                    Err(e) => tracing::warn!("screenshot: {e}"),
                }
            }
            notify(saved.as_deref());
        });
    if let Err(e) = spawned {
        tracing::warn!("screenshot: cannot start the saving thread: {e}");
    }
}

/// niri と同じ通知（アプリ名 niri、要約 "Screenshot captured"、本文 "You can paste the image from the clipboard."、
/// 画像は保存したファイル、transient・urgency 1）を、`busctl --user` で出す。失敗はログだけ。
fn notify(path: Option<&Path>) {
    let icon = path
        .and_then(|p| p.canonicalize().ok())
        .map(|p| format!("file://{}", p.display()))
        .unwrap_or_default();
    let result = std::process::Command::new("busctl")
        .args([
            "--user",
            "call",
            // 引数の `-1`（有効期限）を、オプションと読ませない。
            "--",
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "Notify",
            "susssasa{sv}i",
            "niri",
            "0",
            &icon,
            "Screenshot captured",
            "You can paste the image from the clipboard.",
            "0",
            "2",
            "transient",
            "b",
            "true",
            "urgency",
            "y",
            "1",
            "-1",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output();
    match result {
        Ok(out) if out.status.success() => tracing::info!("screenshot: notification sent"),
        Ok(out) => tracing::warn!(
            "screenshot: notification failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ),
        Err(e) => tracing::warn!("screenshot: cannot run busctl for the notification: {e}"),
    }
}

/// `template` の保存先に、PNG を書く（既存のファイルは上書きしない）。テストで保存の手順を確かめるのに使う。
#[cfg(test)]
pub fn save(
    template: &str,
    home: Option<&str>,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<PathBuf, SaveError> {
    let png = encode_png(width, height, rgba)?;
    let path = resolve_path(template, home)?;
    write_new(&path, &png)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 参照: 標準的な CRC-32 の既知の値（"123456789" → 0xCBF43926）と Adler-32（"Wikipedia" → 0x11E60398）。
    #[test]
    fn checksums_match_the_known_values() {
        assert_eq!(crc32(&[b"123456789"]), 0xCBF4_3926);
        assert_eq!(crc32(&[b"1234", b"56789"]), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        assert_eq!(adler32(b""), 1);
    }

    /// 参照の復号: PNG の署名・チャンク・zlib の格納ブロックを読み戻し、元の画素に戻ることを確かめる。
    fn decode(png: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
        let sig = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        if png.get(..8) != Some(&sig[..]) {
            return Err("signature".into());
        }
        let (mut pos, mut size, mut idat) = (8, (0, 0), Vec::new());
        let mut ended = false;
        while pos < png.len() {
            let len = u32::from_be_bytes(
                png.get(pos..pos + 4)
                    .ok_or("len")?
                    .try_into()
                    .map_err(|_| "len")?,
            ) as usize;
            let kind = png.get(pos + 4..pos + 8).ok_or("kind")?;
            let data = png.get(pos + 8..pos + 8 + len).ok_or("data")?;
            let crc = u32::from_be_bytes(
                png.get(pos + 8 + len..pos + 12 + len)
                    .ok_or("crc")?
                    .try_into()
                    .map_err(|_| "crc")?,
            );
            if crc != crc32(&[kind, data]) {
                return Err(format!("crc of {kind:?}"));
            }
            match kind {
                b"IHDR" => {
                    let w =
                        u32::from_be_bytes(data.get(0..4).ok_or("w")?.try_into().map_err(|_| "w")?);
                    let h =
                        u32::from_be_bytes(data.get(4..8).ok_or("h")?.try_into().map_err(|_| "h")?);
                    if data.get(8..13) != Some(&[8, 6, 0, 0, 0][..]) {
                        return Err("ihdr".into());
                    }
                    size = (w, h);
                }
                b"IDAT" => idat.extend_from_slice(data),
                b"IEND" => ended = true,
                _ => {}
            }
            pos += 12 + len;
        }
        if !ended || idat.get(..2) != Some(&[0x78, 0x01][..]) {
            return Err("structure".into());
        }
        let (mut raw, mut p) = (Vec::new(), 2);
        loop {
            let head = *idat.get(p).ok_or("block")?;
            let len = u16::from_le_bytes(
                idat.get(p + 1..p + 3)
                    .ok_or("l")?
                    .try_into()
                    .map_err(|_| "l")?,
            );
            let nlen = u16::from_le_bytes(
                idat.get(p + 3..p + 5)
                    .ok_or("n")?
                    .try_into()
                    .map_err(|_| "n")?,
            );
            if nlen != !len {
                return Err("nlen".into());
            }
            raw.extend_from_slice(idat.get(p + 5..p + 5 + usize::from(len)).ok_or("body")?);
            p += 5 + usize::from(len);
            if head & 1 == 1 {
                break;
            }
        }
        let adler = u32::from_be_bytes(
            idat.get(p..p + 4)
                .ok_or("adler")?
                .try_into()
                .map_err(|_| "adler")?,
        );
        if adler != adler32(&raw) || p + 4 != idat.len() {
            return Err("adler".into());
        }
        let row = size.0 as usize * 4 + 1;
        let mut pixels = Vec::new();
        for line in raw.chunks(row) {
            if line.first() != Some(&0) {
                return Err("filter".into());
            }
            pixels.extend_from_slice(line.get(1..).ok_or("line")?);
        }
        Ok((size.0, size.1, pixels))
    }

    fn pattern(w: u32, h: u32) -> Vec<u8> {
        (0..w * h * 4)
            .map(|i| (i.wrapping_mul(31) % 251) as u8)
            .collect()
    }

    #[test]
    fn encoded_png_decodes_back_to_the_same_pixels() -> Result<(), String> {
        // 1 画素、小さい画像、格納ブロックが 65535 バイトを超える画像（複数ブロック）。
        for (w, h) in [(1, 1), (3, 2), (17, 5), (300, 120), (1920, 3)] {
            let pixels = pattern(w, h);
            let png = encode_png(w, h, &pixels).map_err(|e| e.to_string())?;
            let (dw, dh, back) = decode(&png)?;
            assert_eq!((dw, dh), (w, h));
            assert_eq!(back, pixels, "{w}x{h}");
        }
        Ok(())
    }

    #[test]
    fn a_wrong_pixel_count_or_an_empty_image_is_refused() {
        assert!(matches!(
            encode_png(2, 2, &[0; 15]),
            Err(SaveError::BadSize)
        ));
        assert!(matches!(encode_png(0, 2, &[]), Err(SaveError::BadSize)));
        assert!(matches!(encode_png(2, 0, &[]), Err(SaveError::BadSize)));
    }

    #[test]
    fn tilde_is_expanded_only_at_the_start() {
        assert_eq!(
            expand_tilde("~/a/b.png", Some("/home/u")),
            "/home/u/a/b.png"
        );
        assert_eq!(expand_tilde("~", Some("/home/u")), "/home/u");
        assert_eq!(expand_tilde("~other/x", Some("/home/u")), "~other/x");
        assert_eq!(expand_tilde("/x/~/y", Some("/home/u")), "/x/~/y");
        assert_eq!(expand_tilde("~/a", None), "~/a");
    }

    #[test]
    fn a_path_without_percent_is_used_as_is_and_percent_is_formatted() -> Result<(), String> {
        let plain = resolve_path("~/shots/a.png", Some("/h")).map_err(|e| e.to_string())?;
        assert_eq!(plain, PathBuf::from("/h/shots/a.png"));
        let dated = resolve_path("/s/%Y-%m-%d %H-%M-%S.png", None).map_err(|e| e.to_string())?;
        let text = dated.to_string_lossy().into_owned();
        // 例: /s/2026-11-08 01-02-03.png
        let digits: Vec<bool> = text.bytes().map(|b| b.is_ascii_digit()).collect();
        assert_eq!(text.len(), "/s/2026-11-08 01-02-03.png".len(), "{text}");
        assert!(
            digits.get(3..7).is_some_and(|d| d.iter().all(|x| *x)),
            "{text}"
        );
        assert!(text.ends_with(".png") && !text.contains('%'), "{text}");
        Ok(())
    }

    #[test]
    fn save_creates_the_directories_and_never_overwrites() -> Result<(), String> {
        let dir = std::env::temp_dir().join(format!("contrast-shot-{}", std::process::id()));
        let template = format!("{}/a/b/shot.png", dir.display());
        let pixels = pattern(4, 3);
        let path = save(&template, None, 4, 3, &pixels).map_err(|e| e.to_string())?;
        let written = std::fs::read(&path).map_err(|e| e.to_string())?;
        assert_eq!(decode(&written)?.2, pixels);
        // 同じ名前があれば失敗して、中身はそのまま。
        assert!(matches!(
            save(
                &template,
                None,
                4,
                3,
                &pattern(4, 3).iter().map(|b| b ^ 1).collect::<Vec<_>>()
            ),
            Err(SaveError::Io(_))
        ));
        assert_eq!(std::fs::read(&path).map_err(|e| e.to_string())?, written);
        Ok(())
    }
}
