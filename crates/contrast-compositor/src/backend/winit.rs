//! winit バックエンド: ホストの窓を出力として扱う。
//!
//! 既定は窓 1 つを出力 1 つ（名前 `winit`）にする。複数の出力の動作を nested で確かめるために、
//! 環境変数 `CONTRAST_WINIT_OUTPUTS=N`（2〜4）で、窓を横に N 等分した N 個の出力（`winit-1`〜）にできる（検証用。niri には無い）。
//! 分けたときは、各出力の要素をその出力の範囲で切り抜いて、1 枚の描画先に並べて描く。

use std::{cell::RefCell, rc::Rc};

use smithay::{
    backend::{
        renderer::{ImportDma, damage::OutputDamageTracker, gles::GlesRenderer},
        winit::{self, WinitEvent, WinitGraphicsBackend},
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::EventLoop,
    utils::{Point, Rectangle, Transform},
};

use contrast_core::Rect;

use crate::render::{RenderError, finish_frame, render_scene};
use crate::state::Contrast;

/// 出力のリフレッシュレート（mHz）。winit では目安でしかない。
const REFRESH: i32 = 60_000;

/// 窓を分ける数の上限。
const MAX_SPLIT: usize = 4;

/// `CONTRAST_WINIT_OUTPUTS` の値から、出力の数（1〜4）。不正・未指定は 1。
fn split_count(value: Option<&str>) -> usize {
    value
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| (1..=MAX_SPLIT).contains(n))
        .unwrap_or(1)
}

/// 幅 `width` の窓を `n` 等分した出力の `(x, 幅)`。余りは最後の出力に足す。
fn split_columns(width: i32, n: usize) -> Vec<(i32, i32)> {
    let count = i32::try_from(n.max(1)).unwrap_or(1);
    let base = (width / count).max(1);
    (0..count)
        .map(|i| {
            let x = base * i;
            let w = if i == count - 1 {
                (width - x).max(1)
            } else {
                base
            };
            (x, w)
        })
        .collect()
}

/// winit の窓を開き、出力を作り、描画とイベント処理をイベントループに登録する。
pub fn init_winit(
    event_loop: &mut EventLoop<Contrast>,
    state: &mut Contrast,
) -> Result<(), Box<dyn std::error::Error>> {
    let (backend, winit): (WinitGraphicsBackend<GlesRenderer>, _) = winit::init()?;
    // dmabuf のインポートはレンダラーが要るので、描画のコールバックと共有する。
    let backend = Rc::new(RefCell::new(backend));
    let (size, formats, feedback) = {
        let mut b = backend.borrow_mut();
        let formats: Vec<_> = b.renderer().dmabuf_formats().iter().copied().collect();
        let feedback = crate::render::dmabuf_feedback(b.renderer());
        (b.window_size(), formats, feedback)
    };
    let importer = backend.clone();
    state.enable_dmabuf(
        formats,
        feedback,
        Box::new(move |dmabuf| match importer.try_borrow_mut() {
            Ok(mut b) => b.renderer().import_dmabuf(dmabuf, None).is_ok(),
            Err(_) => false,
        }),
    );

    let count = split_count(std::env::var("CONTRAST_WINIT_OUTPUTS").ok().as_deref());
    let split = count > 1;
    let mut outputs: Vec<Output> = Vec::new();
    for (i, (x, w)) in split_columns(size.w, count).into_iter().enumerate() {
        let name = if split {
            format!("winit-{}", i + 1)
        } else {
            "winit".to_string()
        };
        let output = Output::new(
            name.clone(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "Contrast".into(),
                model: "Nested".into(),
            },
        );
        let _global = output.create_global::<Contrast>(&state.display_handle);
        let mode = Mode {
            size: (w, size.h).into(),
            refresh: REFRESH,
        };
        // 1 枚のときは、出力の変換で GL の上下の反転を打ち消す（従来どおり）。分けたときは描画先の側で反転する。
        let transform = if split {
            Transform::Normal
        } else {
            Transform::Flipped180
        };
        output.change_current_state(Some(mode), Some(transform), None, Some((x, 0).into()));
        output.set_preferred(mode);
        state.space.map_output(&output, (x, 0));
        let rect = Rect::new(x, 0, w, size.h).ok_or("winit window size is not positive")?;
        state.output_added(&output, rect);
        tracing::info!(name, x, size = ?mode.size, "output created");
        outputs.push(output);
    }

    let mut damage_tracker = match (split, outputs.first()) {
        (false, Some(output)) => OutputDamageTracker::from_output(output),
        _ => OutputDamageTracker::new(size, 1.0, Transform::Flipped180),
    };

    event_loop
        .handle()
        .insert_source(winit, move |event, _, state| match event {
            WinitEvent::Resized { size, .. } => {
                tracing::info!(?size, "output resized");
                for (output, (x, w)) in outputs.iter().zip(split_columns(size.w, outputs.len())) {
                    output.change_current_state(
                        Some(Mode {
                            size: (w, size.h).into(),
                            refresh: REFRESH,
                        }),
                        None,
                        None,
                        Some((x, 0).into()),
                    );
                    state.space.map_output(output, (x, 0));
                    {
                        let mut map = smithay::desktop::layer_map_for_output(output);
                        let changed = map.arrange();
                        tracing::info!(output = output.name(), changed, zone = ?map.non_exclusive_zone(), "layers re-arranged");
                    }
                    match Rect::new(x, 0, w, size.h) {
                        Some(rect) => {
                            state.output_changed(output, rect);
                            state.sync_output_insets(output);
                        }
                        None => tracing::warn!(?size, "output size is not positive, layout skipped"),
                    }
                }
                if split {
                    damage_tracker = OutputDamageTracker::new(size, 1.0, Transform::Flipped180);
                }
            }
            WinitEvent::Input(event) => state.process_input_event(event),
            WinitEvent::Redraw => {
                let Ok(mut backend) = backend.try_borrow_mut() else {
                    return;
                };
                if let Err(e) = render_winit(state, &mut backend, &outputs, split, &mut damage_tracker)
                {
                    tracing::error!("render: {e}, stopping");
                    state.loop_signal.stop();
                    return;
                }
                backend.window().request_redraw();
            }
            WinitEvent::CloseRequested => state.loop_signal.stop(),
            _ => {}
        })?;

    Ok(())
}

/// winit の窓に 1 フレーム描いて提出し、後始末（frame コールバックなど）をする。
fn render_winit(
    state: &mut Contrast,
    backend: &mut WinitGraphicsBackend<GlesRenderer>,
    outputs: &[Output],
    split: bool,
    damage_tracker: &mut OutputDamageTracker,
) -> Result<(), RenderError> {
    state.tick_animations();
    // ホストがカーソルを描くので、隠すときはホストの窓に頼む（変わったときだけ）。
    let hidden = state.cursor_hidden();
    if hidden != state.host_cursor_hidden {
        backend.window().set_cursor_visible(!hidden);
        state.host_cursor_hidden = hidden;
    }
    let damage = Rectangle::from_size(backend.window_size());
    // オフスクリーンへの描画（窓の開閉の絵）は、窓の面を bind する前に済ませる。
    state.prepare_window_animations(backend.renderer());
    state.take_window_screenshot_if_pending(backend.renderer());
    // ホストがカーソルを描くので、ポインタの絵は入らない。
    state.process_screenshot_ui(backend.renderer(), &mut |_, _| Vec::new());
    {
        let (renderer, mut framebuffer) = backend
            .bind()
            .map_err(|e| RenderError::Bind(e.to_string()))?;
        match (split, outputs.first()) {
            (false, Some(output)) => {
                render_scene(state, renderer, &mut framebuffer, output, damage_tracker)?;
            }
            _ => {
                let mut elements = Vec::new();
                for output in outputs {
                    elements.extend(crate::render::cropped_output_elements(
                        state,
                        renderer,
                        output,
                        Point::from((0, 0)),
                    )?);
                }
                let background = state.policy.background();
                damage_tracker
                    .render_output(renderer, &mut framebuffer, 0, &elements, background)
                    .map_err(|e| RenderError::Render(e.to_string()))?;
            }
        }
    }
    backend
        .submit(Some(&[damage]))
        .map_err(|e| RenderError::Submit(e.to_string()))?;
    finish_frame(state, outputs);
    // 提出のあとで読む（オフスクリーンを bind すると、窓の面が current でなくなって submit に失敗する）。
    if state.screenshot_pending {
        // アクティブな出力を撮る（niri の screenshot-screen は、フォーカス中の出力）。ホストがカーソルを描くので、カーソルは入らない。
        let output = state.active_output().or_else(|| outputs.first().cloned());
        if let Some(output) = output {
            let origin = state
                .space
                .output_geometry(&output)
                .map(|g| g.loc)
                .unwrap_or_default();
            if split {
                // 分けたときは、描画と同じ「出力ごとに切り抜いた要素」を全部の出力ぶん集め、撮る出力の範囲を描く
                // （切り抜きが効いていなければ、隣の出力の画面外の窓が写る）。
                let mut elements = Vec::new();
                for o in outputs {
                    elements.extend(crate::render::cropped_output_elements(
                        state,
                        backend.renderer(),
                        o,
                        origin,
                    )?);
                }
                crate::render::take_screenshot_if_pending(
                    state,
                    backend.renderer(),
                    &output,
                    &elements,
                );
            } else {
                let elements = crate::render::scene_elements(
                    state,
                    backend.renderer(),
                    &output,
                    origin,
                    Vec::new(),
                )?;
                crate::render::take_screenshot_if_pending(
                    state,
                    backend.renderer(),
                    &output,
                    &elements,
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_split_count_is_one_to_four() {
        assert_eq!(split_count(None), 1);
        assert_eq!(split_count(Some("2")), 2);
        assert_eq!(split_count(Some(" 4 ")), 4);
        assert_eq!(split_count(Some("5")), 1);
        assert_eq!(split_count(Some("0")), 1);
        assert_eq!(split_count(Some("x")), 1);
    }

    #[test]
    fn columns_cover_the_window_without_gaps() {
        assert_eq!(split_columns(1000, 1), [(0, 1000)]);
        assert_eq!(split_columns(1001, 2), [(0, 500), (500, 501)]);
        let cols = split_columns(999, 3);
        assert_eq!(cols, [(0, 333), (333, 333), (666, 333)]);
    }
}
