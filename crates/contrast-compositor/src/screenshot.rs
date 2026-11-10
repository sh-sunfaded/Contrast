//! スクリーンショットの保存: RGBA の画素を PNG にし、保存先（`~` と strftime の `%Y` など）を決めて書く。
//!
//! PNG は自前の符号化（フィルタ Sub／Up の選択、LZ77、固定ハフマンの deflate）。依存を増やさないため。
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

/// 下位のビットから詰めるビット出力（deflate）。
struct Bits {
    out: Vec<u8>,
    acc: u32,
    used: u32,
}

impl Bits {
    fn new(capacity: usize) -> Self {
        Self {
            out: Vec::with_capacity(capacity),
            acc: 0,
            used: 0,
        }
    }

    /// 下位 `count` ビット（13 以下）を、下位から順に足す。
    fn put(&mut self, value: u32, count: u32) {
        self.acc |= value << self.used;
        self.used += count;
        while self.used >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.used -= 8;
        }
    }

    /// ハフマン符号 `code`（`len` ビット）を、上位のビットから足す。
    fn huffman(&mut self, code: u32, len: u32) {
        self.put(code.reverse_bits() >> (32 - len), len);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.used > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

/// 固定ハフマンのリテラル・長さの符号（RFC 1951 の 3.2.6）。
fn put_symbol(bits: &mut Bits, symbol: u32) {
    match symbol {
        0..=143 => bits.huffman(0x30 + symbol, 8),
        144..=255 => bits.huffman(0x190 + symbol - 144, 9),
        256..=279 => bits.huffman(symbol - 256, 7),
        _ => bits.huffman(0xC0 + symbol - 280, 8),
    }
}

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// 一致（長さ 3〜258、距離 1〜32768）を足す。
fn put_match(bits: &mut Bits, length: usize, distance: usize) {
    let li = LEN_BASE.partition_point(|b| usize::from(*b) <= length) - 1;
    put_symbol(bits, 257 + li as u32);
    bits.put((length - usize::from(LEN_BASE[li])) as u32, LEN_EXTRA[li]);
    let di = DIST_BASE.partition_point(|b| usize::from(*b) <= distance) - 1;
    bits.huffman(di as u32, 5);
    bits.put(
        (distance - usize::from(DIST_BASE[di])) as u32,
        DIST_EXTRA[di],
    );
}

/// `data` を、固定ハフマンの 1 ブロックの deflate にする（貪欲な LZ77。4 バイトのハッシュ、窓は 32 KiB）。
fn deflate(data: &[u8]) -> Vec<u8> {
    const HASH_BITS: u32 = 15;
    const WINDOW: usize = 32768;
    const MAX_MATCH: usize = 258;
    let hash = |i: usize| -> usize {
        let v = u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
        (v.wrapping_mul(2_654_435_761) >> (32 - HASH_BITS)) as usize
    };
    let mut bits = Bits::new(data.len() / 4 + 16);
    bits.put(1, 1); // BFINAL
    bits.put(1, 2); // BTYPE = 01（固定ハフマン）
    let mut table = vec![0_usize; 1 << HASH_BITS]; // 位置 + 1（0 は無し）
    let mut i = 0;
    while i < data.len() {
        let mut best = 0;
        let mut distance = 0;
        if i + 4 <= data.len() {
            let h = hash(i);
            let candidate = table[h];
            table[h] = i + 1;
            if candidate > 0 {
                let j = candidate - 1;
                let d = i - j;
                if d <= WINDOW {
                    let limit = MAX_MATCH.min(data.len() - i);
                    let mut l = 0;
                    while l < limit && data[j + l] == data[i + l] {
                        l += 1;
                    }
                    if l >= 4 {
                        best = l;
                        distance = d;
                    }
                }
            }
        }
        if best == 0 {
            put_symbol(&mut bits, u32::from(data[i]));
            i += 1;
        } else {
            put_match(&mut bits, best, distance);
            // 飛ばした位置も（次の一致の候補のために）覚える。
            for k in i + 1..(i + best).min(data.len().saturating_sub(3)) {
                table[hash(k)] = k + 1;
            }
            i += best;
        }
    }
    put_symbol(&mut bits, 256);
    bits.finish()
}

/// 1 行を PNG のフィルタ（0 なし・1 Sub・2 Up）にして、絶対値の和（小さいほど圧縮しやすい）と一緒に返す。
fn filter_row(kind: u8, line: &[u8], above: Option<&[u8]>) -> (u64, Vec<u8>) {
    let out: Vec<u8> = line
        .iter()
        .enumerate()
        .map(|(x, byte)| match kind {
            1 if x >= 4 => byte.wrapping_sub(line[x - 4]),
            2 => byte.wrapping_sub(above.map_or(0, |a| a[x])),
            _ => *byte,
        })
        .collect();
    let cost = out
        .iter()
        .map(|b| u64::from((*b as i8).unsigned_abs()))
        .sum();
    (cost, out)
}

/// 8 ビット RGBA（上の行から）の画素を、PNG にする。行ごとに、フィルタ（なし・Sub・Up）のうち
/// 絶対値の和が最も小さいものを選び、全体を 1 つの deflate にする。
///
/// # Errors
/// `rgba` の長さが `width × height × 4` でない、または 0 サイズ。
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, SaveError> {
    let row = (width as usize).checked_mul(4).ok_or(SaveError::BadSize)?;
    let total = row.checked_mul(height as usize).ok_or(SaveError::BadSize)?;
    if width == 0 || height == 0 || rgba.len() != total {
        return Err(SaveError::BadSize);
    }
    let mut raw = Vec::with_capacity(total + height as usize);
    let mut above: Option<&[u8]> = None;
    for line in rgba.chunks_exact(row) {
        let (kind, filtered) = (0..=2)
            .map(|k| (k, filter_row(k, line, above)))
            .min_by_key(|(_, (cost, _))| *cost)
            .map_or((0, line.to_vec()), |(k, (_, f))| (k, f));
        raw.push(kind);
        raw.extend_from_slice(&filtered);
        above = Some(line);
    }
    // zlib: ヘッダ + deflate + Adler-32。
    let mut z = vec![0x78, 0x01];
    z.extend_from_slice(&deflate(&raw));
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

    /// 参照の復号: PNG の署名・チャンク・zlib（格納・固定ハフマン）・フィルタを読み戻し、元の画素に戻ることを確かめる。
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
        let (raw, p) = inflate(&idat, 2)?;
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
        let mut pixels: Vec<u8> = Vec::new();
        for (y, line) in raw.chunks(row).enumerate() {
            let kind = *line.first().ok_or("filter")?;
            let body = line.get(1..).ok_or("line")?;
            let above_start = y.saturating_sub(1) * (row - 1);
            for (x, byte) in body.iter().enumerate() {
                let left = if x >= 4 { pixels[pixels.len() - 4] } else { 0 };
                let up = if y > 0 { pixels[above_start + x] } else { 0 };
                let value = match kind {
                    0 => *byte,
                    1 => byte.wrapping_add(left),
                    2 => byte.wrapping_add(up),
                    _ => return Err("filter".into()),
                };
                pixels.push(value);
            }
        }
        Ok((size.0, size.1, pixels))
    }

    /// 参照の inflate（格納ブロックと固定ハフマンだけ。実装と別の書き方）。`start` は zlib ヘッダの次。
    /// 展開した列と、deflate の終わりのバイト位置を返す。
    fn inflate(data: &[u8], start: usize) -> Result<(Vec<u8>, usize), String> {
        let (mut bit, mut out) = (start * 8, Vec::new());
        let take = |bit: &mut usize, n: usize| -> Result<u32, String> {
            let mut v = 0_u32;
            for i in 0..n {
                let byte = *data.get(*bit / 8).ok_or("eof")?;
                v |= u32::from((byte >> (*bit % 8)) & 1) << i;
                *bit += 1;
            }
            Ok(v)
        };
        loop {
            let last = take(&mut bit, 1)?;
            match take(&mut bit, 2)? {
                0 => {
                    bit = bit.div_ceil(8) * 8;
                    let p = bit / 8;
                    let len = usize::from(u16::from_le_bytes([
                        *data.get(p).ok_or("l")?,
                        *data.get(p + 1).ok_or("l")?,
                    ]));
                    out.extend_from_slice(data.get(p + 4..p + 4 + len).ok_or("body")?);
                    bit = (p + 4 + len) * 8;
                }
                1 => loop {
                    let mut code = 0_u32;
                    let mut symbol = None;
                    for len in 1..=9 {
                        code = (code << 1) | take(&mut bit, 1)?;
                        symbol = match (len, code) {
                            (7, 0..=23) => Some(256 + code),
                            (8, 48..=191) => Some(code - 48),
                            (8, 192..=199) => Some(280 + code - 192),
                            (9, 400..=511) => Some(144 + code - 400),
                            _ => None,
                        };
                        if symbol.is_some() {
                            break;
                        }
                    }
                    let symbol = symbol.ok_or("code")?;
                    match symbol {
                        0..=255 => out.push(symbol as u8),
                        256 => break,
                        _ => {
                            const BASE: [u32; 29] = [
                                3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43,
                                51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258,
                            ];
                            const EXTRA: [usize; 29] = [
                                0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4,
                                4, 4, 5, 5, 5, 5, 0,
                            ];
                            const DBASE: [u32; 30] = [
                                1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257,
                                385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289,
                                16385, 24577,
                            ];
                            const DEXTRA: [usize; 30] = [
                                0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9,
                                10, 10, 11, 11, 12, 12, 13, 13,
                            ];
                            let i = (symbol - 257) as usize;
                            let length = BASE.get(i).ok_or("len")?
                                + take(&mut bit, *EXTRA.get(i).ok_or("len")?)?;
                            let mut dcode = 0_u32;
                            for _ in 0..5 {
                                dcode = (dcode << 1) | take(&mut bit, 1)?;
                            }
                            let d = dcode as usize;
                            let distance = (DBASE.get(d).ok_or("dist")?
                                + take(&mut bit, *DEXTRA.get(d).ok_or("dist")?)?)
                                as usize;
                            if distance > out.len() {
                                return Err("distance".into());
                            }
                            for _ in 0..length {
                                out.push(out[out.len() - distance]);
                            }
                        }
                    }
                },
                _ => return Err("block type".into()),
            }
            if last == 1 {
                break;
            }
        }
        Ok((out, bit.div_ceil(8)))
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

    /// 画素の並びの種類: 一色・横の階調・縦の階調・窓のような矩形と細かい模様・乱数に近いもの。
    fn picture(kind: u8, w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let px = match kind {
                    0 => [38, 38, 38, 255],
                    1 => [(x % 256) as u8, 0, 0, 255],
                    2 => [0, (y % 256) as u8, 7, 255],
                    3 if (x / 40 + y / 40) % 5 == 0 => {
                        [(x * 7 % 256) as u8, 200, (y % 256) as u8, 255]
                    }
                    3 => [30, 30, 46, 255],
                    _ => {
                        let v = x
                            .wrapping_mul(2_654_435_761)
                            .wrapping_add(y.wrapping_mul(40_503));
                        [(v >> 3) as u8, (v >> 11) as u8, (v >> 19) as u8, 255]
                    }
                };
                out.extend_from_slice(&px);
            }
        }
        out
    }

    #[test]
    fn compressed_png_decodes_back_for_every_kind_of_picture() -> Result<(), String> {
        for kind in 0..5 {
            for (w, h) in [(1, 1), (2, 3), (37, 11), (320, 200)] {
                let pixels = picture(kind, w, h);
                let png = encode_png(w, h, &pixels).map_err(|e| e.to_string())?;
                let (dw, dh, back) = decode(&png)?;
                assert_eq!((dw, dh), (w, h));
                assert!(back == pixels, "kind {kind} {w}x{h}");
            }
        }
        Ok(())
    }

    #[test]
    fn a_screen_sized_flat_or_window_like_picture_becomes_small() -> Result<(), String> {
        let raw = 1920 * 1200 * 4;
        for (kind, limit) in [(0_u8, 80_000_usize), (1, 150_000), (3, 400_000)] {
            let pixels = picture(kind, 1920, 1200);
            let png = encode_png(1920, 1200, &pixels).map_err(|e| e.to_string())?;
            assert!(
                png.len() < limit,
                "kind {kind}: {} bytes (raw {raw})",
                png.len()
            );
            assert!(decode(&png)?.2 == pixels, "kind {kind}");
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
