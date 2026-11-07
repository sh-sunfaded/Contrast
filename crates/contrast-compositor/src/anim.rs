//! 窓の矩形の補間（移動・大きさの変化・ワークスペース切替のアニメーション）。
//!
//! 補間するのは、窓ごとの「フレーム」（隙間を引いたあと、枠の外側の矩形）。配置の判断は core がするので、
//! ここは core が決めた行き先の矩形へ、今の矩形から滑らかに近づけるだけ。曲線は、ease-out の 3 次（`Layout.animationMs`）か、
//! niri の既定のバネ（`Layout.animation="spring"`。窓の移動・リサイズ・ビューの横移動は stiffness 800、ワークスペースの切り替えで
//! 新しく見える窓は 1000。どちらも減衰比 1.0）。時刻は呼び出し側が渡す（この型は時計を読まない）。

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use contrast_core::{Rect, WindowId};

use crate::spring::{Spring, SpringParams};

/// 補間の曲線の指定。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Motion {
    /// ease-out の 3 次。`Duration` はかける時間（0 なら補間しない）。
    Easing(Duration),
    /// niri の既定のバネ（窓の移動・リサイズは stiffness 800。ワークスペースの切り替えは 1000）。
    Spring,
}

impl From<Duration> for Motion {
    fn from(duration: Duration) -> Self {
        Self::Easing(duration)
    }
}

/// 始まるときに決めた曲線と、その所要時間。
#[derive(Clone, Copy, Debug, PartialEq)]
enum Curve {
    /// ease-out の 3 次と、かける時間。
    Easing(Duration),
    /// 0 から 1 へのバネと、収束までの時間。
    Spring(Spring, Duration),
}

impl Curve {
    /// 時刻 `elapsed` の進み具合（0.0〜1.0）と、終わったか。
    fn at(self, elapsed: Duration) -> (f64, bool) {
        match self {
            Self::Easing(total) => {
                let t = progress(elapsed, total);
                (ease_out(t), t >= 1.0)
            }
            Self::Spring(spring, settle) => (
                spring.value_at(elapsed.as_secs_f64()).clamp(0.0, 1.0),
                elapsed >= settle,
            ),
        }
    }

    /// 曲線を作る。バネは、ワークスペースの切り替え（`workspace`）かどうかでパラメータが違う。
    fn new(motion: Motion, workspace: bool) -> Self {
        match motion {
            Motion::Easing(total) => Self::Easing(total),
            Motion::Spring => {
                let params = if workspace {
                    SpringParams::workspace_switch()
                } else {
                    SpringParams::movement()
                };
                let spring = Spring::progress(params);
                Self::Spring(spring, spring.settle_time())
            }
        }
    }
}

/// 進行中の補間 1 つ。
#[derive(Clone, Copy, Debug, PartialEq)]
struct Anim {
    /// 始まりの矩形。
    from: Rect,
    /// 行き先の矩形。
    to: Rect,
    /// 始まりの時刻。
    start: Instant,
    /// 曲線。
    curve: Curve,
}

/// 経過時間 `elapsed` が `total` のどこまで進んだか（0.0〜1.0）。`total` が 0 なら 1.0。
pub fn progress(elapsed: Duration, total: Duration) -> f64 {
    if total.is_zero() {
        return 1.0;
    }
    (elapsed.as_secs_f64() / total.as_secs_f64()).clamp(0.0, 1.0)
}

/// ease-out（3 次）。0.0 で 0.0、1.0 で 1.0、単調に増え、終わりに近づくほどゆっくりになる。
pub fn ease_out(t: f64) -> f64 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

/// `a` から `b` へ `t`（0.0〜1.0）だけ進んだ整数（四捨五入）。
fn lerp(a: i32, b: i32, t: f64) -> i32 {
    let v = f64::from(a) + (f64::from(b) - f64::from(a)) * t;
    v.round() as i32
}

/// 矩形 `from` から `to` へ `t`（0.0〜1.0。イージングは呼び出し側）だけ進んだ矩形。
pub fn lerp_rect(from: Rect, to: Rect, t: f64) -> Rect {
    if t >= 1.0 {
        return to;
    }
    Rect::new(
        lerp(from.x(), to.x(), t),
        lerp(from.y(), to.y(), t),
        lerp(from.width(), to.width(), t),
        lerp(from.height(), to.height(), t),
    )
    .unwrap_or(to)
}

/// `rect` を縦に `dy` ずらした矩形。
fn shifted(rect: Rect, dy: i32) -> Option<Rect> {
    Rect::new(
        rect.x(),
        rect.y().saturating_add(dy),
        rect.width(),
        rect.height(),
    )
}

/// 窓ごとの補間の状態。
#[derive(Default)]
pub struct Animator {
    /// 進行中の補間。
    running: BTreeMap<WindowId, Anim>,
    /// 最後に表示した矩形（補間が終わった窓も持つ。隠れた窓は忘れる）。
    shown: BTreeMap<WindowId, Rect>,
    /// ワークスペースの切り替えで新しく見える窓が、縦にこれだけずれた位置から入ってくる（px）。
    slide: Option<i32>,
    /// 最後に表示したワークスペースの番号。切り替えの向きを決める。
    last_workspace: Option<u8>,
    /// 補間を止めている間（出力の大きさの変更中など）。矩形はすぐ行き先になる。
    suppressed: bool,
}

impl Animator {
    /// 窓 `id` の行き先を `to` にして、いま表示する矩形を返す。
    ///
    /// 始まりは、いま表示している矩形（補間の途中なら途中の矩形）。まだ表示していない窓は、
    /// ワークスペースの切り替え中なら縦にずらした位置、そうでなければ補間せず `to`。
    /// `animate` が偽、イージングの時間が 0、補間を止めている間、始まりと行き先が同じ、のときは補間せず `to`。
    pub fn retarget(
        &mut self,
        id: WindowId,
        to: Rect,
        now: Instant,
        motion: impl Into<Motion>,
        animate: bool,
    ) -> Rect {
        let motion = motion.into();
        let shown = self.shown.get(&id).copied();
        // 表示済みでない窓が、ワークスペースの切り替えで縦にずれた位置から入ってくるとき。
        let slid = shown.is_none() && self.slide.is_some();
        let from = shown.or_else(|| self.slide.and_then(|dy| shifted(to, dy)));
        let enabled = !matches!(motion, Motion::Easing(d) if d.is_zero());
        match from {
            Some(from) if animate && !self.suppressed && enabled && from != to => {
                self.running.insert(
                    id,
                    Anim {
                        from,
                        to,
                        start: now,
                        curve: Curve::new(motion, slid),
                    },
                );
                self.shown.insert(id, from);
                from
            }
            _ => {
                self.running.remove(&id);
                self.shown.insert(id, to);
                to
            }
        }
    }

    /// 時刻 `now` までの補間を進める。矩形が変わった窓の (id, 矩形) を返す。終わった補間は消える。
    pub fn tick(&mut self, now: Instant) -> Vec<(WindowId, Rect)> {
        let mut updates = Vec::new();
        let mut done = Vec::new();
        for (id, anim) in &self.running {
            let (value, finished) = anim.curve.at(now.saturating_duration_since(anim.start));
            // 終わったら、丸めずに行き先そのもの。
            let rect = if finished {
                anim.to
            } else {
                lerp_rect(anim.from, anim.to, value)
            };
            updates.push((*id, rect));
            if finished {
                done.push(*id);
            }
        }
        for (id, rect) in &updates {
            self.shown.insert(*id, *rect);
        }
        for id in done {
            self.running.remove(&id);
        }
        updates
    }

    /// 補間の途中なら、いま表示している矩形。
    pub fn displayed(&self, id: WindowId) -> Option<Rect> {
        self.running
            .contains_key(&id)
            .then(|| self.shown.get(&id).copied())
            .flatten()
    }

    /// 補間が進行中か（次のフレームも描く必要があるか）。
    pub fn is_running(&self) -> bool {
        !self.running.is_empty()
    }

    /// 窓を忘れる（閉じた窓）。
    pub fn forget(&mut self, id: WindowId) {
        self.running.remove(&id);
        self.shown.remove(&id);
    }

    /// `visible` にない窓を忘れる（隠れた窓。次に見えるときは、新しく見える窓として扱う）。
    pub fn retain(&mut self, visible: &BTreeSet<WindowId>) {
        self.running.retain(|id, _| visible.contains(id));
        self.shown.retain(|id, _| visible.contains(id));
    }

    /// 表示するワークスペースが `workspace` になった。前と違えば、新しく見える窓は、番号が大きくなる向きなら
    /// 下から、小さくなる向きなら上から（`height` だけずれた位置から）入ってくる。`submit` の終わりに `end_slide` で戻す。
    pub fn workspace_shown(&mut self, workspace: u8, height: i32) {
        if let Some(previous) = self.last_workspace
            && previous != workspace
        {
            self.slide = Some(if workspace > previous {
                height
            } else {
                height.saturating_neg()
            });
        }
        self.last_workspace = Some(workspace);
    }

    /// ワークスペースの切り替えによる縦のずれをやめる。
    pub fn end_slide(&mut self) {
        self.slide = None;
    }

    /// 補間を止める／再開する（止めている間の `retarget` は、すぐ行き先になる）。
    pub fn suppress(&mut self, suppressed: bool) {
        self.suppressed = suppressed;
    }
}

#[cfg(test)]
mod tests {
    use contrast_core::{Core, CoreConfig};

    use super::*;

    fn r(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect::new(x, y, w, h).expect("positive rect")
    }

    /// `n` 番目の窓の id（core に登録して払い出してもらう）。
    fn ids(n: usize) -> Vec<WindowId> {
        use contrast_core::{Columns, Command, DefaultPolicy, ExternalKey, OutputName, WindowMeta};
        let mut core = Core::new(CoreConfig::new(4), DefaultPolicy::new(), Columns::new());
        core.apply(Command::OutputAdded {
            key: ExternalKey::new(1),
            name: OutputName::new("t"),
            geometry: r(0, 0, 1000, 600),
        })
        .expect("output");
        (0..n)
            .map(|i| {
                let key = ExternalKey::new(10 + i as u64);
                core.apply(Command::WindowMapped {
                    key,
                    meta: WindowMeta::default(),
                })
                .expect("window");
                core.view().window_id(key).expect("id")
            })
            .collect()
    }

    const MS: Duration = Duration::from_millis(100);

    #[test]
    fn progress_is_clamped() {
        assert_eq!(progress(Duration::ZERO, MS), 0.0);
        assert_eq!(progress(Duration::from_millis(50), MS), 0.5);
        assert_eq!(progress(Duration::from_millis(500), MS), 1.0);
        assert_eq!(progress(Duration::from_millis(1), Duration::ZERO), 1.0);
    }

    #[test]
    fn easing_is_monotonic_with_fixed_ends() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert_eq!(ease_out(-3.0), 0.0);
        assert_eq!(ease_out(9.0), 1.0);
        let mut last = 0.0;
        for i in 1..=100 {
            let v = ease_out(f64::from(i) / 100.0);
            assert!(v >= last, "{i}");
            last = v;
        }
        // 速く始まって、ゆっくり終わる。
        assert!(ease_out(0.5) > 0.5);
    }

    #[test]
    fn lerp_rect_moves_and_resizes() {
        let (a, b) = (r(0, 0, 100, 100), r(100, 50, 300, 200));
        assert_eq!(lerp_rect(a, b, 0.0), a);
        assert_eq!(lerp_rect(a, b, 0.5), r(50, 25, 200, 150));
        assert_eq!(lerp_rect(a, b, 1.0), b);
        // 逆向き（縮む・戻る）も正の大きさのまま。
        assert_eq!(lerp_rect(b, a, 0.5), r(50, 25, 200, 150));
    }

    #[test]
    fn a_window_without_history_is_not_animated() {
        let id = ids(1)[0];
        let mut a = Animator::default();
        let to = r(10, 10, 100, 100);
        assert_eq!(a.retarget(id, to, Instant::now(), MS, true), to);
        assert!(!a.is_running());
    }

    #[test]
    fn a_known_window_animates_to_the_new_rect_and_finishes() {
        let id = ids(1)[0];
        let (mut a, t0) = (Animator::default(), Instant::now());
        let (from, to) = (r(0, 0, 100, 100), r(200, 0, 100, 100));
        a.retarget(id, from, t0, MS, true);
        // 行き先が変わると、いまの矩形から始まる。
        assert_eq!(a.retarget(id, to, t0, MS, true), from);
        assert!(a.is_running());
        assert_eq!(a.displayed(id), Some(from));

        let mid = a.tick(t0 + Duration::from_millis(50));
        assert_eq!(mid.len(), 1);
        let x = mid[0].1.x();
        assert!(x > 100 && x < 200, "ease-out は半分より先まで進む: {x}");
        assert_eq!(a.displayed(id).map(|r| r.x()), Some(x));

        let end = a.tick(t0 + Duration::from_millis(100));
        assert_eq!(end, [(id, to)]);
        assert!(!a.is_running());
        assert_eq!(a.displayed(id), None);
        // 終わったあとは、何も返さない。
        assert!(a.tick(t0 + Duration::from_millis(200)).is_empty());
    }

    #[test]
    fn retargeting_in_the_middle_continues_from_the_displayed_rect() {
        let id = ids(1)[0];
        let (mut a, t0) = (Animator::default(), Instant::now());
        a.retarget(id, r(0, 0, 100, 100), t0, MS, true);
        a.retarget(id, r(300, 0, 100, 100), t0, MS, true);
        let mid = a.tick(t0 + Duration::from_millis(40))[0].1;
        let t1 = t0 + Duration::from_millis(40);
        // 途中で行き先が変わっても、飛ばずに、いまの矩形から始まる。
        assert_eq!(a.retarget(id, r(0, 0, 100, 100), t1, MS, true), mid);
    }

    #[test]
    fn no_animation_when_disabled_suppressed_or_unchanged() {
        let id = ids(1)[0];
        let (mut a, t0) = (Animator::default(), Instant::now());
        let (from, to) = (r(0, 0, 100, 100), r(200, 0, 100, 100));
        a.retarget(id, from, t0, MS, true);
        assert_eq!(a.retarget(id, to, t0, MS, false), to);
        assert!(!a.is_running());
        a.retarget(id, from, t0, Duration::ZERO, true);
        assert_eq!(a.retarget(id, to, t0, Duration::ZERO, true), to);
        a.suppress(true);
        assert_eq!(a.retarget(id, from, t0, MS, true), from);
        a.suppress(false);
        // 同じ矩形は補間しない。
        assert_eq!(a.retarget(id, from, t0, MS, true), from);
        assert!(!a.is_running());
    }

    #[test]
    fn workspace_switch_slides_in_new_windows_only() {
        let v = ids(2);
        let (mut a, t0) = (Animator::default(), Instant::now());
        let to = r(0, 0, 100, 100);
        // 最初の表示では、ずらさない。
        a.workspace_shown(0, 600);
        assert_eq!(a.retarget(v[0], to, t0, MS, true), to);
        a.end_slide();
        // 0 → 1: 新しく見える窓は下から入る。
        a.workspace_shown(1, 600);
        assert_eq!(a.retarget(v[1], to, t0, MS, true), r(0, 600, 100, 100));
        // 表示済みの窓は、ずらさない。
        assert_eq!(a.retarget(v[0], to, t0, MS, true), to);
        a.end_slide();
        // 1 → 0: 上から入る。
        let mut b = Animator::default();
        b.workspace_shown(1, 600);
        b.end_slide();
        b.workspace_shown(0, 600);
        assert_eq!(b.retarget(v[1], to, t0, MS, true), r(0, -600, 100, 100));
        // 同じワークスペースの再表示では、ずらさない。
        let mut c = Animator::default();
        c.workspace_shown(2, 600);
        c.workspace_shown(2, 600);
        assert_eq!(c.retarget(v[0], to, t0, MS, true), to);
    }

    #[test]
    fn hidden_and_closed_windows_are_forgotten() {
        let v = ids(2);
        let (mut a, t0) = (Animator::default(), Instant::now());
        let (from, to) = (r(0, 0, 100, 100), r(200, 0, 100, 100));
        for id in &v {
            a.retarget(*id, from, t0, MS, true);
            a.retarget(*id, to, t0, MS, true);
        }
        a.retain(&BTreeSet::from([v[0]]));
        assert_eq!(a.displayed(v[1]), None);
        assert!(a.displayed(v[0]).is_some());
        a.forget(v[0]);
        assert!(!a.is_running());
        // 忘れた窓は、次は新しい窓として扱う（補間しない）。
        assert_eq!(a.retarget(v[0], to, t0, MS, true), to);
    }
}
