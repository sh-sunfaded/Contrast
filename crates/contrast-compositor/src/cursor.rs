//! カーソルの絵: xcursor のテーマ（`XCURSOR_THEME` / `XCURSOR_SIZE`）、クライアントが `wl_pointer.set_cursor` で
//! 指定した面、テーマが無いときの色つきの四角。DRM バックエンドだけが使う（winit ではホストがカーソルを描く）。
//!
//! テーマの読み込みの失敗は落ちない（そのカーソルは「無い」ことにして、四角に戻る）。アニメーションは最初のコマだけ。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{
                Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                solid::{SolidColorBuffer, SolidColorRenderElement},
                surface::render_elements_from_surface_tree,
            },
            gles::GlesRenderer,
        },
    },
    input::pointer::{CursorImageAttributes, CursorImageStatus},
    utils::{Logical, Physical, Point, Transform},
    wayland::compositor::with_states,
};

use contrast_policy::CursorSettings;

use crate::render::SceneElement;
use crate::state::Contrast;

/// 入力が `limit_ms` ミリ秒以上なかったら、カーソルを隠すか（`Cursor.hideAfterInactiveMs`）。`None` なら隠さない。
pub fn is_hidden(idle: std::time::Duration, limit_ms: Option<u32>) -> bool {
    limit_ms.is_some_and(|ms| idle >= std::time::Duration::from_millis(u64::from(ms)))
}

impl Contrast {
    /// 今、カーソルを隠すべきか。動かす・キーを押すと（`last_input` が新しくなり）また出る。
    pub fn cursor_hidden(&self) -> bool {
        is_hidden(
            self.last_input.elapsed(),
            self.policy.cursor_settings().hide_after_inactive_ms,
        )
    }
}

/// カーソルの既定の大きさ（`XCURSOR_SIZE` が無いとき）。
const DEFAULT_SIZE: u32 = 24;
/// 簡易カーソル（四角）の一辺と色。
const FALLBACK_SIZE: i32 = 12;
const FALLBACK_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// テーマ名と大きさの優先順位（設定 > 環境変数 > 既定）。環境変数の大きさが範囲外・非数なら無視する。
fn resolve_theme(
    settings: &CursorSettings,
    env_theme: Option<&str>,
    env_size: Option<&str>,
) -> (String, u32) {
    let name = settings
        .theme
        .clone()
        .or_else(|| env_theme.filter(|n| !n.is_empty()).map(str::to_owned))
        .unwrap_or_else(|| "default".to_owned());
    let size = settings
        .size
        .or_else(|| {
            env_size
                .and_then(|s| s.trim().parse::<u32>().ok())
                .filter(|s| (8..=256).contains(s))
        })
        .unwrap_or(DEFAULT_SIZE);
    (name, size)
}

/// テーマから読んだカーソル 1 つ。
pub struct ThemeCursor {
    /// 画素（Argb8888）。
    buffer: MemoryRenderBuffer,
    /// ホットスポット（画像の左上から）。
    hotspot: (i32, i32),
}

/// xcursor のテーマ。名前ごとに 1 度だけ読む（読めなかった名前も覚える）。
pub struct CursorTheme {
    name: String,
    size: u32,
    /// 名前と拡大率（1/120 刻みの整数）ごとの絵。
    cache: BTreeMap<(String, u32), Option<ThemeCursor>>,
}

/// 拡大率 `scale` の出力に使う画像の大きさ（物理 px）。niri と同じく、テーマの大きさに拡大率を掛ける。
pub fn scaled_size(size: u32, scale: f64) -> u32 {
    (f64::from(size) * scale.max(1.0)).round() as u32
}

/// 拡大率を、キャッシュの鍵（1/120 刻み）にする。
fn scale_key(scale: f64) -> u32 {
    (scale.max(1.0) * 120.0).round() as u32
}

impl CursorTheme {
    /// 設定（CDL の `Cursor`）> 環境変数 `XCURSOR_THEME` / `XCURSOR_SIZE` > 既定（`default` / 24）で作る。
    /// まだ何も読まない。
    pub fn resolve(settings: &CursorSettings) -> Self {
        let env_theme = std::env::var("XCURSOR_THEME").ok();
        let env_size = std::env::var("XCURSOR_SIZE").ok();
        let (name, size) = resolve_theme(settings, env_theme.as_deref(), env_size.as_deref());
        Self::new(name, size)
    }

    /// テーマ名と大きさを指定して作る。
    pub fn new(name: String, size: u32) -> Self {
        Self {
            name,
            size,
            cache: BTreeMap::new(),
        }
    }

    /// 拡大率 `scale` の出力に描くカーソル `icon`（xcursor の名前）を返す。テーマに無ければ `default`、それも無ければ `None`。
    fn get(&mut self, icon: &str, scale: f64) -> Option<&ThemeCursor> {
        let key = if self.load(icon, scale) {
            icon
        } else {
            "default"
        };
        self.load(key, scale);
        self.cache
            .get(&(key.to_owned(), scale_key(scale)))
            .and_then(Option::as_ref)
    }

    /// `icon` を（まだなら）拡大率 `scale` の大きさで読む。読めたかを返す。
    fn load(&mut self, icon: &str, scale: f64) -> bool {
        let key = (icon.to_owned(), scale_key(scale));
        if !self.cache.contains_key(&key) {
            let cursor = load_cursor(&self.name, icon, scaled_size(self.size, scale));
            if cursor.is_none() {
                tracing::debug!(theme = %self.name, icon, "cursor: not found in the theme");
            }
            self.cache.insert(key.clone(), cursor);
        }
        self.cache.get(&key).is_some_and(Option::is_some)
    }
}

/// テーマ `theme` のカーソル `icon` を、大きさが `size` に最も近い画像で読む。
pub fn load_cursor(theme: &str, icon: &str, size: u32) -> Option<ThemeCursor> {
    let path: PathBuf = xcursor::CursorTheme::load(theme).load_icon(icon)?;
    let data = std::fs::read(path).ok()?;
    parse_cursor(&data, size)
}

/// xcursor ファイルの中身から、`size` に最も近い画像（最初のコマ）を取り出す。壊れていれば `None`。
pub fn parse_cursor(data: &[u8], size: u32) -> Option<ThemeCursor> {
    let images = xcursor::parser::parse_xcursor(data)?;
    let nominal = images.iter().min_by_key(|i| i.size.abs_diff(size))?.size;
    let image = images.iter().find(|i| i.size == nominal)?;
    let (w, h) = (
        i32::try_from(image.width).ok()?,
        i32::try_from(image.height).ok()?,
    );
    let expected = usize::try_from(w)
        .ok()?
        .checked_mul(usize::try_from(h).ok()?)?
        .checked_mul(4)?;
    if w <= 0 || h <= 0 || image.pixels_rgba.len() < expected {
        return None;
    }
    let buffer = MemoryRenderBuffer::from_slice(
        &image.pixels_rgba,
        Fourcc::Argb8888,
        (w, h),
        1,
        Transform::Normal,
        None,
    );
    Some(ThemeCursor {
        buffer,
        hotspot: (
            i32::try_from(image.xhot).ok()?,
            i32::try_from(image.yhot).ok()?,
        ),
    })
}

impl ThemeCursor {
    /// ホットスポット（画像の左上から）。
    pub fn hotspot(&self) -> (i32, i32) {
        self.hotspot
    }
}

/// いまのカーソルの描画要素（最前面に置く）。`pos` はポインタの論理座標。
pub fn cursor_elements(
    renderer: &mut GlesRenderer,
    theme: &mut CursorTheme,
    fallback: &mut SolidColorBuffer,
    status: &CursorImageStatus,
    pos: Point<f64, Logical>,
    scale: f64,
) -> Vec<SceneElement> {
    match status {
        CursorImageStatus::Hidden => Vec::new(),
        CursorImageStatus::Surface(surface) => {
            let hotspot = with_states(surface, |states| {
                states
                    .data_map
                    .get::<Mutex<CursorImageAttributes>>()
                    .and_then(|attrs| attrs.lock().ok().map(|a| a.hotspot))
            })
            .unwrap_or_default();
            let at: Point<i32, Physical> =
                (pos - hotspot.to_f64()).to_physical_precise_round(scale);
            render_elements_from_surface_tree(renderer, surface, at, scale, 1.0, Kind::Cursor)
        }
        CursorImageStatus::Named(icon) => match theme.get(icon.name(), scale) {
            Some(cursor) => {
                let (hx, hy) = cursor.hotspot();
                // 画像は物理 px のまま（テーマの大きさ）。位置だけ拡大率を掛ける。
                let at: Point<f64, Physical> =
                    (pos.x * scale - f64::from(hx), pos.y * scale - f64::from(hy)).into();
                match MemoryRenderBufferRenderElement::from_buffer(
                    renderer,
                    at,
                    &cursor.buffer,
                    None,
                    None,
                    None,
                    Kind::Cursor,
                ) {
                    Ok(element) => vec![SceneElement::Memory(element)],
                    Err(e) => {
                        tracing::warn!("cursor: texture import failed: {e}");
                        vec![fallback_square(fallback, pos, scale)]
                    }
                }
            }
            None => vec![fallback_square(fallback, pos, scale)],
        },
    }
}

/// テーマが使えないときの、色つきの四角。
fn fallback_square(
    buffer: &mut SolidColorBuffer,
    pos: Point<f64, Logical>,
    scale: f64,
) -> SceneElement {
    buffer.update((FALLBACK_SIZE, FALLBACK_SIZE), FALLBACK_COLOR);
    let at: Point<i32, Physical> = pos.to_physical(scale).to_i32_round();
    SceneElement::Solid(SolidColorRenderElement::from_buffer(
        buffer,
        (at.x, at.y),
        1.0,
        1.0,
        Kind::Unspecified,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 大きさ `nominal` の 1x1 画像 1 つだけの xcursor ファイル。
    fn xcursor_file(nominal: u32, xhot: u32, yhot: u32) -> Vec<u8> {
        let mut v = Vec::new();
        let put = |v: &mut Vec<u8>, n: u32| v.extend_from_slice(&n.to_le_bytes());
        v.extend_from_slice(b"Xcur");
        put(&mut v, 16); // header size
        put(&mut v, 0x0001_0000); // version
        put(&mut v, 1); // ntoc
        put(&mut v, 0xfffd_0002); // toc: type = image
        put(&mut v, nominal); // toc: subtype
        put(&mut v, 28); // toc: position
        put(&mut v, 36); // chunk header size
        put(&mut v, 0xfffd_0002);
        put(&mut v, nominal);
        put(&mut v, 1); // chunk version
        put(&mut v, 1); // width
        put(&mut v, 1); // height
        put(&mut v, xhot);
        put(&mut v, yhot);
        put(&mut v, 0); // delay
        put(&mut v, 0xff11_2233); // pixel
        v
    }

    #[test]
    fn parses_a_cursor_and_keeps_the_hotspot() {
        let cursor = parse_cursor(&xcursor_file(24, 0, 0), 24);
        assert_eq!(cursor.map(|c| c.hotspot()), Some((0, 0)));
    }

    #[test]
    fn broken_data_is_none_not_a_panic() {
        assert!(parse_cursor(b"", 24).is_none());
        assert!(parse_cursor(b"not an xcursor file", 24).is_none());
        let mut truncated = xcursor_file(24, 0, 0);
        truncated.truncate(30);
        assert!(parse_cursor(&truncated, 24).is_none());
    }

    /// インストール済みのテーマ（`XCURSOR_THEME`、無ければ `default`）があれば、読めること。無ければ何もしない。
    #[test]
    fn installed_theme_loads_when_present() {
        let theme = std::env::var("XCURSOR_THEME").unwrap_or_else(|_| "default".to_owned());
        if let Some(cursor) = load_cursor(&theme, "default", 24) {
            let (hx, hy) = cursor.hotspot();
            eprintln!("cursor theme {theme}: hotspot=({hx}, {hy})");
            assert!(hx >= 0 && hy >= 0);
        }
    }

    #[test]
    fn theme_priority_is_settings_then_env_then_default() {
        let none = CursorSettings::default();
        assert_eq!(resolve_theme(&none, None, None), ("default".to_owned(), 24));
        assert_eq!(
            resolve_theme(&none, Some("Env"), Some("32")),
            ("Env".to_owned(), 32)
        );
        let set = CursorSettings {
            theme: Some("Cdl".to_owned()),
            size: Some(48),
            ..CursorSettings::default()
        };
        assert_eq!(
            resolve_theme(&set, Some("Env"), Some("32")),
            ("Cdl".to_owned(), 48)
        );
        // 環境変数の不正な大きさは既定に戻る。
        assert_eq!(resolve_theme(&none, None, Some("4")).1, 24);
        assert_eq!(resolve_theme(&none, None, Some("big")).1, 24);
        assert_eq!(resolve_theme(&none, Some(""), None).0, "default");
    }

    #[test]
    fn hides_only_after_the_idle_time() {
        use std::time::Duration;
        let ms = Duration::from_millis;
        assert!(!is_hidden(ms(5000), None));
        assert!(!is_hidden(ms(999), Some(1000)));
        assert!(is_hidden(ms(1000), Some(1000)));
        assert!(is_hidden(ms(60_000), Some(1000)));
        assert!(!is_hidden(Duration::ZERO, Some(1)));
    }

    #[test]
    fn the_cursor_image_size_follows_the_output_scale() {
        assert_eq!(scaled_size(24, 1.0), 24);
        assert_eq!(scaled_size(24, 2.0), 48);
        assert_eq!(scaled_size(24, 1.25), 30);
        // 1 未満は 1 として扱う。
        assert_eq!(scaled_size(24, 0.5), 24);
        assert_ne!(scale_key(1.0), scale_key(1.25));
        assert_eq!(scale_key(1.0), scale_key(0.5));
    }

    #[test]
    fn missing_theme_gives_no_cursor() {
        assert!(load_cursor("contrast-no-such-theme-xyz", "default", 24).is_none());
        let mut theme = CursorTheme::new("contrast-no-such-theme-xyz".to_owned(), 24);
        assert!(theme.get("default", 1.0).is_none());
        assert!(theme.get("text", 1.0).is_none());
    }
}
