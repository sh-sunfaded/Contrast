//! フォーカスの明度遷移（`<Motion><Focus .../></Motion>`）。窓ごとの減光の乗数 `k` を、時間で補間する。
//!
//! 補間の値は `DimAnimator::displayed` の 1 か所で決める。描画の 2 か所（`prepare_window_animations` の絵と
//! `render::scene_elements` の面）と、開閉の絵（`Snapshot.dim`）はそれを読む。`screenshot-window` は常に 1.0
//! （`render::dim::snapshot_dim`）で、ここの値を使わない。時刻は引数で受ける（この型は時計を読まない）。
//!
//! 守ること:
//! - 履歴の無い窓（新しく開いた・隠れていた窓）は補間せず目標から始める。
//! - 新しい目標は動き中のものを置き換える。始点は今表示している値（速度は引き継がない）。
//! - 終わったら `displayed` は目標に厳密に一致する。
//! - リロードで `Tier` の `l`（フォーカス外の乗数）が変わったフレームは補間せず即時。

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use contrast_core::WindowId;
use contrast_policy::motion::{dim_at, progress};
use contrast_policy::{EasingCurve, MotionCurve, MotionKind, MotionSettings};

/// 明度遷移の曲線と長さ。`None`（`<Focus>` が無い・長さ 0）は即時。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocusMotion {
    /// 曲線。
    pub curve: EasingCurve,
    /// 長さ（0 でない）。
    pub duration: Duration,
}

/// `<Motion>` の `Focus` から曲線と長さを取る。`motion` が `None`（`<Motion>` なし）・`<Focus>` なし・長さ 0 は `None`（即時）。
/// `niriSpring` は `Focus` に置けない（CT072）ので、来ても即時として扱う。
pub fn focus_motion(motion: Option<MotionSettings>) -> Option<FocusMotion> {
    let spec = motion?.spec(MotionKind::Focus)?;
    match spec.curve {
        MotionCurve::Easing(curve) if spec.duration_ms != 0 => Some(FocusMotion {
            curve,
            duration: Duration::from_millis(u64::from(spec.duration_ms)),
        }),
        _ => None,
    }
}

/// 窓 1 枚の遷移。
#[derive(Clone, Copy, Debug, PartialEq)]
struct Entry {
    /// 始点の乗数。
    from: f32,
    /// 目標の乗数。
    to: f32,
    /// 始めた時刻。
    start: Instant,
    /// 動いているときの曲線と長さ。`None` は止まっている（`displayed == to`）。
    motion: Option<FocusMotion>,
    /// いま表示する乗数。
    displayed: f32,
}

/// 窓ごとの明度遷移。
#[derive(Default)]
pub struct DimAnimator {
    /// 窓ごとの遷移。
    entries: BTreeMap<WindowId, Entry>,
    /// 前のフレームのフォーカス外の乗数（リロードの検出に使う）。
    last_rest: Option<f32>,
    /// このフレームは補間しない（リロードで `Tier` の `l` が変わった）。
    instant_frame: bool,
}

impl DimAnimator {
    /// フレームの始めに呼ぶ。フォーカス外の乗数 `rest` が前のフレームと違えば、このフレームは補間しない。
    pub fn begin_frame(&mut self, rest: f32) {
        self.instant_frame = self.last_rest.is_some_and(|previous| previous != rest);
        self.last_rest = Some(rest);
    }

    /// 窓 `id` の目標の乗数を `target` にして、いま表示する乗数を返す。
    /// 履歴が無ければ補間せず `target`。目標が変わったら、いま表示している値から `motion` で動かす（`None` なら即時）。
    pub fn retarget(
        &mut self,
        id: WindowId,
        target: f32,
        now: Instant,
        motion: Option<FocusMotion>,
    ) -> f32 {
        let motion = if self.instant_frame { None } else { motion };
        let settled = |displayed: f32| Entry {
            from: displayed,
            to: displayed,
            start: now,
            motion: None,
            displayed,
        };
        let Some(entry) = self.entries.get(&id).copied() else {
            self.entries.insert(id, settled(target));
            return target;
        };
        // 目標が同じなら、動いていてもそのまま。
        if (entry.to - target).abs() <= f32::EPSILON {
            return entry.displayed;
        }
        let from = entry.displayed;
        match motion {
            Some(m) if (from - target).abs() > f32::EPSILON => {
                self.entries.insert(
                    id,
                    Entry {
                        from,
                        to: target,
                        start: now,
                        motion: Some(m),
                        displayed: from,
                    },
                );
                from
            }
            _ => {
                self.entries.insert(id, settled(target));
                target
            }
        }
    }

    /// 時刻 `now` まで進める。まだ動いている窓があれば `true`。終わった窓は `displayed` が目標に一致する。
    pub fn tick(&mut self, now: Instant) -> bool {
        let mut running = false;
        for entry in self.entries.values_mut() {
            let Some(m) = entry.motion else {
                continue;
            };
            let p = progress(now.saturating_duration_since(entry.start), m.duration);
            if p >= 1.0 {
                // 終わったら、丸めずに目標そのもの。
                entry.displayed = entry.to;
                entry.motion = None;
            } else {
                entry.displayed = dim_at(entry.from, entry.to, p, m.curve);
                running = true;
            }
        }
        running
    }

    /// 窓 `id` のいま表示する乗数。履歴が無ければ `None`。
    pub fn displayed(&self, id: WindowId) -> Option<f32> {
        self.entries.get(&id).map(|e| e.displayed)
    }

    /// 遷移が進行中か（次のフレームも描く必要があるか）。
    pub fn is_running(&self) -> bool {
        self.entries.values().any(|e| e.motion.is_some())
    }

    /// 窓を忘れる。
    pub fn forget(&mut self, id: WindowId) {
        self.entries.remove(&id);
    }

    /// `visible` にない窓を忘れる（隠れた窓。次に見えるときは履歴の無い窓として扱う）。
    pub fn retain(&mut self, visible: &BTreeSet<WindowId>) {
        self.entries.retain(|id, _| visible.contains(id));
    }
}

#[cfg(test)]
mod tests {
    use contrast_core::{
        Columns, Command, Core, CoreConfig, DefaultPolicy, ExternalKey, OutputName, Rect,
        WindowMeta,
    };
    use contrast_policy::MotionSpec;

    use super::*;

    /// `n` 番目の窓の id（core に登録して払い出してもらう）。
    fn ids(n: usize) -> Vec<WindowId> {
        let mut core = Core::new(CoreConfig::new(4), DefaultPolicy::new(), Columns::new());
        core.apply(Command::OutputAdded {
            key: ExternalKey::new(1),
            name: OutputName::new("t"),
            geometry: Rect::new(0, 0, 1000, 600).expect("rect"),
        })
        .expect("output");
        (0..n)
            .map(|i| {
                let key = ExternalKey::new(10 + u64::try_from(i).expect("small"));
                core.apply(Command::WindowMapped {
                    key,
                    meta: WindowMeta::default(),
                })
                .expect("window");
                core.view().window_id(key).expect("id")
            })
            .collect()
    }

    const MS100: Option<FocusMotion> = Some(FocusMotion {
        curve: EasingCurve::EaseOutCubic,
        duration: Duration::from_millis(100),
    });

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    #[test]
    fn focus_motion_comes_from_the_focus_element_only() {
        let mut m = MotionSettings::default();
        assert_eq!(focus_motion(None), None);
        assert_eq!(focus_motion(Some(m)), None);
        m.set(
            MotionKind::Focus,
            MotionSpec::default_for(MotionKind::Focus),
        );
        assert_eq!(
            focus_motion(Some(m)),
            Some(FocusMotion {
                curve: EasingCurve::EaseOutCubic,
                duration: Duration::from_millis(120),
            })
        );
        // 長さ 0 は即時。
        m.set(
            MotionKind::Focus,
            MotionSpec {
                curve: MotionCurve::Easing(EasingCurve::EaseOutQuad),
                duration_ms: 0,
            },
        );
        assert_eq!(focus_motion(Some(m)), None);
        // 矩形の種別だけ書いても Focus は即時。
        let mut s = MotionSettings::default();
        s.set(
            MotionKind::Scroll,
            MotionSpec::default_for(MotionKind::Scroll),
        );
        assert_eq!(focus_motion(Some(s)), None);
    }

    #[test]
    fn a_window_without_history_starts_at_its_target() {
        let id = ids(1)[0];
        let (mut d, t0) = (DimAnimator::default(), Instant::now());
        assert_eq!(d.displayed(id), None);
        assert_eq!(d.retarget(id, 0.8, t0, MS100), 0.8);
        assert_eq!(d.displayed(id), Some(0.8));
        assert!(!d.is_running());
    }

    #[test]
    fn a_new_target_moves_from_the_displayed_value_and_ends_exactly_on_it() {
        let id = ids(1)[0];
        let (mut d, t0) = (DimAnimator::default(), Instant::now());
        d.retarget(id, 1.0, t0, MS100);
        // フォーカスを失う: 1.0 → 0.8。始点は今の値。
        assert_eq!(d.retarget(id, 0.8, t0, MS100), 1.0);
        assert!(d.is_running());
        assert!(d.tick(at(t0, 50)));
        let mid = d.displayed(id).expect("mid");
        assert!(mid < 1.0 && mid > 0.8, "{mid}");
        // 曲線どおり（easeOutCubic の半分は 0.875）。
        let want = dim_at(1.0, 0.8, 0.5, EasingCurve::EaseOutCubic);
        assert!((mid - want).abs() < 1e-6, "{mid} {want}");
        assert!(!d.tick(at(t0, 100)));
        assert_eq!(d.displayed(id), Some(0.8));
        assert!(!d.is_running());
        // 動きが終わったあとに同じ目標を渡しても動かない。
        assert_eq!(d.retarget(id, 0.8, at(t0, 200), MS100), 0.8);
        assert!(!d.is_running());
    }

    #[test]
    fn interrupting_restarts_from_the_displayed_value() {
        let id = ids(1)[0];
        let (mut d, t0) = (DimAnimator::default(), Instant::now());
        d.retarget(id, 1.0, t0, MS100);
        d.retarget(id, 0.8, t0, MS100);
        d.tick(at(t0, 40));
        let shown = d.displayed(id).expect("shown");
        // 途中でフォーカスが戻る: 今の値から 1.0 へ。飛ばない。
        assert_eq!(d.retarget(id, 1.0, at(t0, 40), MS100), shown);
        d.tick(at(t0, 140));
        assert_eq!(d.displayed(id), Some(1.0));
    }

    #[test]
    fn no_motion_means_instant() {
        let id = ids(1)[0];
        let (mut d, t0) = (DimAnimator::default(), Instant::now());
        d.retarget(id, 1.0, t0, None);
        assert_eq!(d.retarget(id, 0.8, t0, None), 0.8);
        assert!(!d.is_running());
        assert_eq!(d.displayed(id), Some(0.8));
        // 動いている途中で動き無しの目標が来ても、即時に目標。
        d.retarget(id, 1.0, t0, MS100);
        assert!(d.is_running());
        assert_eq!(d.retarget(id, 0.8, t0, None), 0.8);
        assert!(!d.is_running());
    }

    #[test]
    fn a_reload_that_changes_the_rest_multiplier_is_instant_for_that_frame_only() {
        let id = ids(1)[0];
        let (mut d, t0) = (DimAnimator::default(), Instant::now());
        d.begin_frame(0.8);
        d.retarget(id, 0.8, t0, MS100);
        // `Tier` の `l` が変わった（0.8 → 0.6）: 補間しない。
        d.begin_frame(0.6);
        assert_eq!(d.retarget(id, 0.6, at(t0, 10), MS100), 0.6);
        assert!(!d.is_running());
        // 次のフレームからは普通に補間する。
        d.begin_frame(0.6);
        assert_eq!(d.retarget(id, 1.0, at(t0, 20), MS100), 0.6);
        assert!(d.is_running());
    }

    #[test]
    fn hidden_and_closed_windows_are_forgotten() {
        let v = ids(2);
        let (mut d, t0) = (DimAnimator::default(), Instant::now());
        for id in &v {
            d.retarget(*id, 1.0, t0, MS100);
            d.retarget(*id, 0.8, t0, MS100);
        }
        d.retain(&BTreeSet::from([v[0]]));
        assert_eq!(d.displayed(v[1]), None);
        assert!(d.displayed(v[0]).is_some());
        d.forget(v[0]);
        assert!(!d.is_running());
        // 忘れた窓は履歴の無い窓として、目標から始まる。
        assert_eq!(d.retarget(v[0], 0.8, t0, MS100), 0.8);
    }

    #[test]
    fn the_closing_snapshot_reads_the_displayed_value_not_the_target() {
        // 閉じる窓の絵の乗数は、画面で見えていた値（`displayed`）。補間の途中で閉じても、一瞬明るくならない。
        let id = ids(1)[0];
        let (mut d, t0) = (DimAnimator::default(), Instant::now());
        d.retarget(id, 1.0, t0, MS100);
        d.retarget(id, 0.8, t0, MS100);
        d.tick(at(t0, 30));
        let seen = d.displayed(id).expect("seen");
        assert!(seen > 0.8 && seen < 1.0);
        // 最後のフレームの値をスナップショットに入れ、`Animation` の用途ではそのまま、`ScreenshotWindow` では 1.0。
        use crate::render::dim::{SnapshotPurpose, snapshot_dim};
        assert_eq!(snapshot_dim(SnapshotPurpose::Animation, seen), seen);
        assert_eq!(snapshot_dim(SnapshotPurpose::ScreenshotWindow, seen), 1.0);
    }
}
