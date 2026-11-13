//! 減光（`StyledSurface` の `dim`）のオフスクリーンのテスト。実際の GL（`/dev/dri/renderD*`）で、テクスチャを描いて画素を比べる。
//!
//! 描画ノードが無い環境では、各テストは `SKIPPED (no EGL render node): <名前>` を標準エラーに出して戻る。
//! その場合、そのテストは**何も検証していない**（`cargo test -- --nocapture` で `SKIPPED` を数える）。

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            ImportMem, Renderer,
            damage::OutputDamageTracker,
            element::{Id, Kind, solid::SolidColorRenderElement, texture::TextureRenderElement},
            gles::{GlesRenderer, GlesTexture},
            utils::CommitCounter,
        },
    },
    utils::{Logical, Physical, Point, Rectangle, Size, Transform},
};

use super::{
    DimIds, StyledSurface,
    look::{Shaders, offscreen::renderer},
    offscreen::render_to_rgba,
};

/// テクスチャの一辺（px）。
const SIDE: i32 = 32;
/// 試す乗数（`l = 0.8` の仮置きに近い値と、強い値）。
const DIMS: [f32; 2] = [0.7435, 0.5];

/// 描画ノードが使えなければスキップの印を出して `None`。
fn gl(name: &str) -> Option<(GlesRenderer, Shaders)> {
    let Some(mut r) = renderer() else {
        eprintln!("SKIPPED (no EGL render node): {name}");
        return None;
    };
    let Some(sh) = Shaders::get(&mut r) else {
        eprintln!("SKIPPED (shaders did not compile): {name}");
        return None;
    };
    Some((r, sh))
}

/// 左半分が不透明（200,100,50,255）、右半分が半透明（プリマルチプライドで 100,50,25,128）のテクスチャ。
fn texture(r: &mut GlesRenderer) -> Result<GlesTexture, String> {
    let mut data = Vec::new();
    for _y in 0..SIDE {
        for x in 0..SIDE {
            if x < SIDE / 2 {
                data.extend([200u8, 100, 50, 255]);
            } else {
                data.extend([100u8, 50, 25, 128]);
            }
        }
    }
    r.import_memory(&data, Fourcc::Abgr8888, (SIDE, SIDE).into(), false)
        .map_err(|e| e.to_string())
}

/// テクスチャの要素。位置は描画先の左上からの物理座標。
fn element(
    r: &GlesRenderer,
    tex: &GlesTexture,
    id: Id,
    at: (f64, f64),
) -> TextureRenderElement<GlesTexture> {
    TextureRenderElement::from_static_texture(
        id,
        r.context_id(),
        Point::<f64, Physical>::from(at),
        tex.clone(),
        1,
        Transform::Normal,
        None,
        None,
        Some(Size::<i32, Logical>::from((SIDE, SIDE))),
        None,
        Kind::Unspecified,
    )
}

/// 画素 `(x, y)`。
fn px(rgba: &[u8], width: i32, x: i32, y: i32) -> [u8; 4] {
    let i = usize::try_from((y * width + x) * 4).unwrap_or(0);
    [
        rgba.get(i).copied().unwrap_or(0),
        rgba.get(i + 1).copied().unwrap_or(0),
        rgba.get(i + 2).copied().unwrap_or(0),
        rgba.get(i + 3).copied().unwrap_or(0),
    ]
}

/// 大きさ `size` の透明な描画先に、`elements` を描いて RGBA を読む。
fn draw<E: smithay::backend::renderer::element::RenderElement<GlesRenderer>>(
    r: &mut GlesRenderer,
    size: i32,
    elements: &[E],
) -> Result<Vec<u8>, String> {
    render_to_rgba(
        r,
        Size::<i32, Physical>::from((size, size)),
        1.0,
        elements,
        [0.0; 4],
    )
}

/// `plain`（減光なし）と `dimmed`（乗数 `k`）の全画素を比べる: A は同じ、RGB は `k` 倍（丸めの許容 2）。
fn assert_dimmed(plain: &[u8], dimmed: &[u8], size: i32, k: f32) {
    assert_eq!(plain.len(), dimmed.len());
    for y in 0..size {
        for x in 0..size {
            let p = px(plain, size, x, y);
            let d = px(dimmed, size, x, y);
            assert_eq!(p[3], d[3], "A at ({x},{y}) k={k}");
            for c in 0..3 {
                let want = (f32::from(p[c]) * k).round();
                assert!(
                    (f32::from(d[c]) - want).abs() <= 2.0,
                    "RGB[{c}] at ({x},{y}) k={k}: got {}, want {want}",
                    d[c]
                );
            }
        }
    }
}

#[test]
fn dim_scales_rgb_keeps_alpha_and_handles_partial_transparency() -> Result<(), String> {
    let Some((mut r, sh)) = gl("dim_scales_rgb_keeps_alpha_and_handles_partial_transparency")
    else {
        return Ok(());
    };
    let tex = texture(&mut r)?;
    let plain_elem = element(&r, &tex, Id::new(), (0.0, 0.0));
    let plain = draw(&mut r, SIDE, &[plain_elem])?;
    // 比較の前提: 左は不透明、右は半透明。
    assert_eq!(px(&plain, SIDE, 2, 2), [200, 100, 50, 255]);
    assert_eq!(px(&plain, SIDE, SIDE - 3, 2)[3], 128);
    for k in DIMS {
        let styled = StyledSurface::new(
            element(&r, &tex, Id::new(), (0.0, 0.0)),
            sh.rounded.clone(),
            0.0,
            k,
        );
        let dimmed = draw(&mut r, SIDE, &[styled])?;
        assert_dimmed(&plain, &dimmed, SIDE, k);
        // 実際に暗くなっている。
        assert!(px(&dimmed, SIDE, 2, 2)[0] < px(&plain, SIDE, 2, 2)[0]);
    }
    Ok(())
}

#[test]
fn radius_zero_and_dim_one_do_not_change_any_pixel() -> Result<(), String> {
    let Some((mut r, sh)) = gl("radius_zero_and_dim_one_do_not_change_any_pixel") else {
        return Ok(());
    };
    let tex = texture(&mut r)?;
    let plain_elem = element(&r, &tex, Id::new(), (0.0, 0.0));
    let plain = draw(&mut r, SIDE, &[plain_elem])?;
    let styled = StyledSurface::new(
        element(&r, &tex, Id::new(), (0.0, 0.0)),
        sh.rounded.clone(),
        0.0,
        1.0,
    );
    let same = draw(&mut r, SIDE, &[styled])?;
    for y in 0..SIDE {
        for x in 0..SIDE {
            let (p, s) = (px(&plain, SIDE, x, y), px(&same, SIDE, x, y));
            for c in 0..4 {
                assert!(
                    p[c].abs_diff(s[c]) <= 1,
                    "({x},{y}) channel {c}: plain {p:?}, styled {s:?}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn rounded_corners_still_clip_when_dimmed() -> Result<(), String> {
    let Some((mut r, sh)) = gl("rounded_corners_still_clip_when_dimmed") else {
        return Ok(());
    };
    let tex = texture(&mut r)?;
    let k = DIMS[0];
    let styled = StyledSurface::new(
        element(&r, &tex, Id::new(), (0.0, 0.0)),
        sh.rounded.clone(),
        12.0,
        k,
    );
    let out = draw(&mut r, SIDE, &[styled])?;
    assert!(px(&out, SIDE, 0, 0)[3] < 13, "角は切れる");
    assert_eq!(px(&out, SIDE, 8, 16)[3], 255, "角丸の内側の A は変わらない");
    let want = (200.0 * k).round();
    assert!((f32::from(px(&out, SIDE, 8, 16)[0]) - want).abs() <= 2.0);
    Ok(())
}

#[test]
fn partly_outside_the_output_is_still_dimmed_only_where_drawn() -> Result<(), String> {
    let Some((mut r, sh)) = gl("partly_outside_the_output_is_still_dimmed_only_where_drawn") else {
        return Ok(());
    };
    let tex = texture(&mut r)?;
    let k = DIMS[0];
    for at in [(-16.0, -16.0), (16.0, 16.0), (-31.0, 0.0)] {
        let plain_elem = element(&r, &tex, Id::new(), at);
        let plain = draw(&mut r, SIDE, &[plain_elem])?;
        let styled =
            StyledSurface::new(element(&r, &tex, Id::new(), at), sh.rounded.clone(), 0.0, k);
        let dimmed = draw(&mut r, SIDE, &[styled])?;
        assert_dimmed(&plain, &dimmed, SIDE, k);
    }
    Ok(())
}

#[test]
fn pixels_outside_the_window_are_not_dimmed() -> Result<(), String> {
    let Some((mut r, sh)) = gl("pixels_outside_the_window_are_not_dimmed") else {
        return Ok(());
    };
    let tex = texture(&mut r)?;
    let size = 48;
    // 窓（8..40）の外側に、枠の帯の領域（4..44）。枠は減光されない要素。
    let band = || {
        SolidColorRenderElement::new(
            Id::new(),
            Rectangle::<i32, Physical>::new(Point::from((4, 4)), Size::from((40, 40))),
            CommitCounter::default(),
            [0.2, 0.4, 0.6, 1.0],
            Kind::Unspecified,
        )
    };
    smithay::backend::renderer::element::render_elements! {
        Scene<=GlesRenderer>;
        Window=StyledSurface<TextureRenderElement<GlesTexture>>,
        Band=SolidColorRenderElement,
    }
    let render = |r: &mut GlesRenderer, k: f32| -> Result<Vec<u8>, String> {
        let window = StyledSurface::new(
            element(r, &tex, Id::new(), (8.0, 8.0)),
            sh.rounded.clone(),
            0.0,
            k,
        );
        draw(r, size, &[Scene::Window(window), Scene::Band(band())])
    };
    let bright = render(&mut r, 1.0)?;
    let dark = render(&mut r, DIMS[0])?;
    for (x, y) in [
        (4, 4),
        (5, 5),
        (43, 43),
        (6, 20),
        (41, 20),
        (20, 5),
        (20, 42),
    ] {
        assert_eq!(
            px(&bright, size, x, y),
            px(&dark, size, x, y),
            "枠の領域 ({x},{y}) は減光の影響を受けない"
        );
    }
    // 窓の中は暗くなる。
    assert!(px(&dark, size, 12, 20)[0] < px(&bright, size, 12, 20)[0]);
    // 出力の外の角は透明のまま。
    assert_eq!(px(&dark, size, 0, 0), [0, 0, 0, 0]);
    Ok(())
}

/// 損傷（`damage_output`）が空か。
fn is_empty(d: Option<&Vec<Rectangle<i32, Physical>>>) -> bool {
    d.is_none_or(Vec::is_empty)
}

#[test]
fn a_dim_change_needs_a_new_id_to_be_redrawn() -> Result<(), String> {
    let Some((mut r, sh)) = gl("a_dim_change_needs_a_new_id_to_be_redrawn") else {
        return Ok(());
    };
    let tex = texture(&mut r)?;
    let inner = Id::new();
    let mut tracker = OutputDamageTracker::new(
        Size::<i32, Physical>::from((SIDE, SIDE)),
        1.0,
        Transform::Normal,
    );
    let make = |r: &GlesRenderer, dim: f32, id: Option<Id>| {
        let s = StyledSurface::new(
            element(r, &tex, inner.clone(), (0.0, 0.0)),
            sh.rounded.clone(),
            0.0,
            dim,
        );
        match id {
            Some(id) => s.with_id(id),
            None => s,
        }
    };
    let k = DIMS[0];
    let mut ids = DimIds::default();

    // 1 枚目: 全面の損傷。
    let (d, _) = tracker
        .damage_output(0, &[make(&r, 1.0, None)])
        .map_err(|e| e.to_string())?;
    assert!(!is_empty(d), "最初のフレームは全面");
    // 変化なし: 損傷なし。
    let (d, _) = tracker
        .damage_output(1, &[make(&r, 1.0, None)])
        .map_err(|e| e.to_string())?;
    assert!(is_empty(d), "変化がなければ損傷なし");
    // 対照: 乗数だけ変えて Id を替えない → 損傷が出ない（これが DimIds を使う理由）。
    let (d, _) = tracker
        .damage_output(1, &[make(&r, k, None)])
        .map_err(|e| e.to_string())?;
    assert!(
        is_empty(d),
        "Id が同じなら、乗数が変わっても損傷は出ない（Smithay 0.7.0 の仕様）"
    );
    // 対策: 乗数が変わったら新しい Id → 全面の損傷。
    ids.begin_frame();
    let id = ids.styled_id(&inner, k);
    ids.end_frame();
    let (d, _) = tracker
        .damage_output(1, &[make(&r, k, Some(id.clone()))])
        .map_err(|e| e.to_string())?;
    let full = Rectangle::<i32, Physical>::from_size(Size::from((SIDE, SIDE)));
    let covered = d.is_some_and(|rects| {
        rects
            .iter()
            .fold(0i64, |a, r| a + i64::from(r.size.w) * i64::from(r.size.h))
            >= i64::from(full.size.w) * i64::from(full.size.h)
    });
    assert!(covered, "新しい Id では要素の全面が損傷する: {d:?}");
    // 同じ乗数・同じ Id が続けば、また損傷なし。
    ids.begin_frame();
    let again = ids.styled_id(&inner, k);
    ids.end_frame();
    assert_eq!(id, again);
    let (d, _) = tracker
        .damage_output(1, &[make(&r, k, Some(again))])
        .map_err(|e| e.to_string())?;
    assert!(is_empty(d), "乗数が変わらなければ損傷なし");
    Ok(())
}
