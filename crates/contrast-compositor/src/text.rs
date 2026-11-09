//! 文字の描画の基盤: フォントを探して読み、1 行の文字列を RGBA の画素に描く。
//!
//! niri は pango + cairo で描くが、開発シェルに pango が無いので、純 Rust の `fontdue` で描く。
//! フォントは fontconfig の `fc-match` で探す（sans・sans:bold・monospace。niri の `sans 14px` と `Monospace` に当たる）。
//! 1 行だけを扱い、カーニングとシェーピング（合字・複雑な文字）はしない（pango との差）。

use std::path::PathBuf;
use std::sync::OnceLock;

use fontdue::{Font, FontSettings};

/// 使うフォント。
pub struct Fonts {
    /// 本文（sans）。
    pub regular: Font,
    /// 見出し（sans の太字）。
    pub bold: Font,
    /// キーの名前（monospace）。
    pub mono: Font,
}

/// 色（RGBA、0.0〜1.0、乗算前）。
pub type Color = [f32; 4];

/// `fc-match` でフォントのファイルを探す。
fn find_font(pattern: &str) -> Option<PathBuf> {
    let out = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", pattern])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    let path = String::from_utf8(out.stdout).ok()?;
    let path = PathBuf::from(path.trim());
    path.is_file().then_some(path)
}

/// フォントのファイルを読む。
fn load(pattern: &str) -> Result<Font, String> {
    let path =
        find_font(pattern).ok_or_else(|| format!("fc-match found no font for `{pattern}`"))?;
    let data = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Font::from_bytes(data, FontSettings::default()).map_err(|e| format!("{}: {e}", path.display()))
}

/// フォント（最初の 1 回だけ探して読む）。見つからなければ `None`（文字を描く UI は出さず、ログに出す）。
pub fn fonts() -> Option<&'static Fonts> {
    static FONTS: OnceLock<Option<Fonts>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let loaded = (|| -> Result<Fonts, String> {
                Ok(Fonts {
                    regular: load("sans")?,
                    bold: load("sans:bold")?,
                    mono: load("monospace")?,
                })
            })();
            match loaded {
                Ok(f) => Some(f),
                Err(e) => {
                    tracing::warn!("text: cannot load fonts: {e}");
                    None
                }
            }
        })
        .as_ref()
}

/// 1 行の高さ（上の高さ + 下の深さ、切り上げ）。
pub fn line_height(font: &Font, px: f32) -> i32 {
    font.horizontal_line_metrics(px)
        .map_or(px.ceil() as i32, |m| (m.ascent - m.descent).ceil() as i32)
}

/// ベースラインまでの高さ（切り上げ）。
fn ascent(font: &Font, px: f32) -> f32 {
    font.horizontal_line_metrics(px).map_or(px, |m| m.ascent)
}

/// 1 行の幅（送り幅の合計、切り上げ）。
pub fn text_width(font: &Font, px: f32, text: &str) -> i32 {
    text.chars()
        .map(|c| font.metrics(c, px).advance_width)
        .sum::<f32>()
        .ceil() as i32
}

/// RGBA の画素（上の行から、乗算前のアルファ）。
pub struct Canvas {
    /// 幅。
    pub width: i32,
    /// 高さ。
    pub height: i32,
    /// 画素。
    pub rgba: Vec<u8>,
}

/// 0.0〜1.0 を 0〜255 にする。
fn byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

impl Canvas {
    /// 色 `background` で塗った `width × height` の画素。
    pub fn new(width: i32, height: i32, background: Color) -> Self {
        let (w, h) = (width.max(1), height.max(1));
        let pixel = background.map(byte);
        let count = usize::try_from(w).unwrap_or(1) * usize::try_from(h).unwrap_or(1);
        Self {
            width: w,
            height: h,
            rgba: pixel.iter().copied().cycle().take(count * 4).collect(),
        }
    }

    /// 点 `(x, y)` に色 `color` を被覆率 `coverage` で重ねる（範囲外は何もしない）。
    fn blend(&mut self, x: i32, y: i32, color: Color, coverage: f32) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return;
        }
        let i = (usize::try_from(y).unwrap_or(0) * usize::try_from(self.width).unwrap_or(0)
            + usize::try_from(x).unwrap_or(0))
            * 4;
        let Some(px) = self.rgba.get_mut(i..i + 4) else {
            return;
        };
        let a = (color[3] * coverage).clamp(0.0, 1.0);
        for (c, v) in px.iter_mut().take(3).zip(color) {
            let old = f32::from(*c) / 255.0;
            *c = byte(old * (1.0 - a) + v * a);
        }
        let old_a = f32::from(px[3]) / 255.0;
        px[3] = byte(old_a + a * (1.0 - old_a));
    }

    /// 矩形を塗る。
    pub fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.blend(xx, yy, color, 1.0);
            }
        }
    }

    /// 1 行の文字列を、左上 `(x, y)`（行の上端）から描く。描いた幅を返す。
    pub fn text(&mut self, font: &Font, px: f32, x: i32, y: i32, text: &str, color: Color) -> i32 {
        let baseline = y as f32 + ascent(font, px);
        let mut pen = x as f32;
        for c in text.chars() {
            let (m, bitmap) = font.rasterize(c, px);
            let gx = (pen + m.xmin as f32).round() as i32;
            let gy = (baseline - m.height as f32 - m.ymin as f32).round() as i32;
            for (row, line) in bitmap.chunks(m.width.max(1)).enumerate() {
                for (col, coverage) in line.iter().enumerate() {
                    if *coverage > 0 {
                        let (cx, cy) = (
                            i32::try_from(col).unwrap_or(0),
                            i32::try_from(row).unwrap_or(0),
                        );
                        self.blend(gx + cx, gy + cy, color, f32::from(*coverage) / 255.0);
                    }
                }
            }
            pen += m.advance_width;
        }
        (pen - x as f32).ceil() as i32
    }

    /// 中心 `(cx, cy)`・半径 `r` の円を塗る（縁は 1 px でなめらかに）。
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, color: Color) {
        let (x0, x1) = ((cx - r - 1.0).floor() as i32, (cx + r + 1.0).ceil() as i32);
        let (y0, y1) = ((cy - r - 1.0).floor() as i32, (cy + r + 1.0).ceil() as i32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                let coverage = (r - d + 0.5).clamp(0.0, 1.0);
                if coverage > 0.0 {
                    self.blend(x, y, color, coverage);
                }
            }
        }
    }

    /// 内側に太さ `width` の枠を描く。
    pub fn border(&mut self, width: i32, color: Color) {
        let (w, h) = (self.width, self.height);
        self.fill(0, 0, w, width, color);
        self.fill(0, h - width, w, width, color);
        self.fill(0, 0, width, h, color);
        self.fill(w - width, 0, width, h, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_canvas_is_filled_and_blended() {
        let mut c = Canvas::new(4, 2, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(c.rgba.len(), 32);
        c.fill(1, 0, 2, 1, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(&c.rgba[4..8], &[255, 255, 255, 255]);
        assert_eq!(&c.rgba[0..4], &[0, 0, 0, 255]);
        c.blend(0, 1, [1.0, 0.0, 0.0, 1.0], 0.5);
        assert_eq!(&c.rgba[16..20], &[128, 0, 0, 255]);
        // 範囲外は何もしない。
        c.blend(9, 9, [1.0, 1.0, 1.0, 1.0], 1.0);
    }

    #[test]
    fn the_border_is_drawn_inside() {
        let mut c = Canvas::new(6, 6, [0.0, 0.0, 0.0, 1.0]);
        c.border(2, [1.0, 1.0, 1.0, 1.0]);
        let at = |x: usize, y: usize| c.rgba[(y * 6 + x) * 4];
        assert_eq!(at(0, 0), 255);
        assert_eq!(at(1, 3), 255);
        assert_eq!(at(2, 2), 0);
        assert_eq!(at(5, 5), 255);
    }

    #[test]
    fn text_draws_something_when_fonts_are_available() {
        // フォントが無い環境では、何もしない（テストの前提を満たさない）。
        let Some(f) = fonts() else { return };
        let w = text_width(&f.regular, 14.0, "Hotkeys");
        assert!(w > 20 && w < 120, "{w}");
        let mut c = Canvas::new(
            w + 4,
            line_height(&f.regular, 14.0) + 4,
            [0.0, 0.0, 0.0, 1.0],
        );
        c.text(&f.regular, 14.0, 2, 2, "Hotkeys", [1.0, 1.0, 1.0, 1.0]);
        assert!(c.rgba.chunks(4).any(|p| p[0] > 200), "白い画素がある");
    }
}
