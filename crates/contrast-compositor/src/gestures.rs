//! タッチパッドのジェスチャー（3 本指・4 本指）と、DnD の端スクロール（niri の `on_gesture_swipe_*` と `dnd_scroll_gesture_*`）。
//!
//! - 3 本指: 指の移動の合計が 16 px（GNOME と同じ閾値）を超えたら向きを決める。横ならビューのジェスチャー（core の
//!   `ViewGesture*`。移動量に `作業領域の幅 / 1200` を掛ける）、縦ならワークスペースの切り替え（300 px で 1 つ、今の ±1 まで）。
//!   終わりに、速度から減速して止まる位置（`SwipeTracker::projected_end_pos`）を出して渡す。natural-scroll のデバイスは向きを反転。
//! - 4 本指: オーバービュー（`overview.rs`）。
//! - DnD: ドラッグ中、ポインタが作業領域の左右の端 30 px に 100 ms 居たら、端に近いほど速く（最大 1500 px/s）ビューを動かす。
//!
//! 時間を使う計算（速度・減速）はここで行い、core には移動量だけを渡す（core-spec の追補 18 節）。

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use contrast_core::{Command, ExternalKey, WorkspaceIndex};
use smithay::utils::{Logical, Point};

use crate::rubber_band::RubberBand;
use crate::spring::{Spring, SpringParams};
use crate::state::Contrast;

/// 速度を見る履歴の長さ（niri の `HISTORY_LIMIT`）。
const HISTORY_LIMIT: Duration = Duration::from_millis(150);
/// タッチパッドの減速率（niri の `DECELERATION_TOUCHPAD`）。
const DECELERATION_TOUCHPAD: f64 = 0.997;
/// 向きを決める移動量（px。GNOME Shell と同じ）。
const DECIDE_DISTANCE: f64 = 16.0;
/// ビューのジェスチャーで、作業領域の幅だけ動くのに要る指の移動（niri の `VIEW_GESTURE_WORKING_AREA_MOVEMENT`）。
const VIEW_MOVEMENT: f64 = 1200.0;
/// ワークスペースの切り替えで、1 つ動くのに要る指の移動（niri の `WORKSPACE_GESTURE_MOVEMENT`）。
const WORKSPACE_MOVEMENT: f64 = 300.0;
/// ワークスペースの切り替えで端を越えて引いたときのゴム（niri の `WORKSPACE_GESTURE_RUBBER_BAND`）。
const WORKSPACE_RUBBER_BAND: RubberBand = RubberBand {
    stiffness: 0.5,
    limit: 0.05,
};
/// DnD の端スクロール（niri の `dnd-edge-view-scroll` の既定）。
const DND_TRIGGER_WIDTH: f64 = 30.0;
const DND_DELAY: Duration = Duration::from_millis(100);
const DND_MAX_SPEED: f64 = 1500.0;
/// オーバービューでの DnD の縦の端スクロール（niri の `dnd-edge-workspace-switch` の既定。1 つ動くのに 1500 px 分）。
const DND_TRIGGER_HEIGHT: f64 = 50.0;
const DND_WORKSPACE_MOVEMENT: f64 = 1500.0;

/// 縦の端スクロールの強さ（−1〜1。上の端なら負）。`y` は作業領域の中の位置、`height` は作業領域の高さ、
/// `x`・`width` は、ワークスペースが並ぶ帯（縮めた幅）の中の横の位置と幅。帯の外は 0（niri の `dnd_scroll_gesture_scroll`）。
pub fn dnd_vertical_delta(x: f64, width: f64, y: f64, height: f64) -> f64 {
    if x < 0.0 || width <= x {
        return 0.0;
    }
    let y = y.clamp(0.0, height);
    let trigger = DND_TRIGGER_HEIGHT.clamp(0.0, height / 2.0);
    if trigger < 0.01 {
        return 0.0;
    }
    let d = if y < trigger {
        -(trigger - y)
    } else if height - y < trigger {
        trigger - (height - y)
    } else {
        0.0
    };
    d / trigger
}

/// 指の移動の履歴（niri の `SwipeTracker`）。
#[derive(Debug, Clone, Default)]
pub struct SwipeTracker {
    history: VecDeque<(f64, Duration)>,
    pos: f64,
}

impl SwipeTracker {
    /// 移動 `delta` を時刻 `t` に足す（時刻が戻るものは捨てる）。
    pub fn push(&mut self, delta: f64, t: Duration) {
        if self.history.back().is_some_and(|(_, last)| t < *last) {
            return;
        }
        self.history.push_back((delta, t));
        self.pos += delta;
        let newest = t;
        while self
            .history
            .front()
            .is_some_and(|(_, first)| newest > *first + HISTORY_LIMIT)
        {
            self.history.pop_front();
        }
    }

    /// 移動の合計。
    pub fn pos(&self) -> f64 {
        self.pos
    }

    /// 速度（px/s。直近 150 ms）。
    pub fn velocity(&self) -> f64 {
        let (Some((_, first)), Some((_, last))) = (self.history.front(), self.history.back())
        else {
            return 0.0;
        };
        let total = last.saturating_sub(*first).as_secs_f64();
        if total == 0.0 {
            return 0.0;
        }
        self.history.iter().map(|(d, _)| d).sum::<f64>() / total
    }

    /// 減速して止まる位置。
    pub fn projected_end_pos(&self) -> f64 {
        self.pos - self.velocity() / (1000.0 * DECELERATION_TOUCHPAD.ln())
    }
}

/// ワークスペースの切り替えのジェスチャーの終わりの位置（niri の `workspace_switch_gesture_end`）。
/// `start` は始めた位置、`count` は列の数。今の ±1 に収めて丸める。
pub fn workspace_target(start: usize, count: usize, projected_px: f64) -> usize {
    let last = count.saturating_sub(1);
    let min = start.saturating_sub(1) as f64;
    let max = (start + 1).min(last) as f64;
    (start as f64 + projected_px / WORKSPACE_MOVEMENT)
        .clamp(min, max)
        .round() as usize
}

/// 縦のスワイプの途中の、ワークスペースの位置の、始めた位置からのずれ（ワークスペース何個分か）。
/// 指の移動 `pos_px` を 300 px で 1 つとして、今の ±1（と、端）の外はゴムのように収める（niri の `workspace_switch_gesture_update`）。
pub fn workspace_follow(start: usize, count: usize, pos_px: f64) -> f64 {
    let last = count.saturating_sub(1);
    let min = start.saturating_sub(1) as f64 - start as f64;
    let max = (start + 1).min(last) as f64 - start as f64;
    WORKSPACE_RUBBER_BAND.clamp(min, max, pos_px / WORKSPACE_MOVEMENT)
}

/// 指を離した瞬間の、位置の動く速さ（ワークスペース/秒）。端を越えているときはゴムの傾きを掛ける。
pub fn workspace_release_velocity(
    start: usize,
    count: usize,
    pos_px: f64,
    velocity_px: f64,
) -> f64 {
    let last = count.saturating_sub(1);
    let min = start.saturating_sub(1) as f64 - start as f64;
    let max = (start + 1).min(last) as f64 - start as f64;
    velocity_px / WORKSPACE_MOVEMENT
        * WORKSPACE_RUBBER_BAND.clamp_derivative(min, max, pos_px / WORKSPACE_MOVEMENT)
}

/// 縦のスワイプで、ワークスペースが指についてくる間と、離したあとにバネで落ち着く間の状態。
/// 値は、いま表示中のワークスペースの位置からのずれ（ワークスペース何個分か。正なら下のものが上がってくる）。
#[derive(Debug, Clone, Copy)]
pub enum WsFollow {
    /// 指についていく。
    Gesture(f64),
    /// 離した。バネで 0 へ。
    Settling {
        /// 動き。
        spring: Spring,
        /// 始めた時刻。
        start: Instant,
    },
}

impl WsFollow {
    /// 今のずれ。
    pub fn value(&self) -> f64 {
        match self {
            Self::Gesture(v) => *v,
            Self::Settling { spring, start } => spring.value_at(start.elapsed().as_secs_f64()),
        }
    }

    /// バネが落ち着いたか。
    pub fn is_settled(&self) -> bool {
        matches!(self, Self::Settling { spring, start } if start.elapsed() >= spring.settle_time())
    }
}

/// DnD の端スクロールの強さ（−1〜1。左の端なら負）。`x` は作業領域の中の位置、`width` は作業領域の幅（niri の式）。
pub fn dnd_edge_delta(x: f64, width: f64) -> f64 {
    let x = x.clamp(0.0, width);
    let trigger = DND_TRIGGER_WIDTH.clamp(0.0, width / 2.0);
    if trigger < 0.01 {
        return 0.0;
    }
    let d = if x < trigger {
        -(trigger - x)
    } else if width - x < trigger {
        trigger - (width - x)
    } else {
        0.0
    };
    d / trigger
}

/// 進行中のスワイプ。
#[derive(Debug, Clone)]
pub enum Swipe {
    /// 3 本指で、まだ向きを決めていない（合計の移動）。
    Undecided(f64, f64),
    /// ビュー（core のジェスチャー）。`norm` は移動に掛ける係数。
    View { tracker: SwipeTracker, norm: f64 },
    /// ワークスペースの切り替え（出力、始めた位置、列の数）。
    Workspace {
        tracker: SwipeTracker,
        output: ExternalKey,
        start: usize,
        count: usize,
    },
    /// オーバービュー（4 本指）。
    Overview { tracker: SwipeTracker, start: f64 },
}

/// DnD の端スクロールの状態。
#[derive(Debug, Clone)]
pub struct DndScroll {
    /// ポインタの位置（全体の論理座標）。
    pub pos: Point<f64, Logical>,
    last: Instant,
    nonzero_since: Option<Instant>,
    tracker: SwipeTracker,
    started: Instant,
    /// core のジェスチャーを始めたか（列の無いワークスペースでは始まらない）。
    begun: bool,
    /// オーバービューでの縦の端スクロールの、表示中のワークスペースからのずれ（ワークスペース何個分か）。
    ws_offset: f64,
    /// 縦の端スクロールの強さが 0 でなくなった時刻（遅延の起点）。
    v_nonzero_since: Option<Instant>,
    /// 縦の端スクロールの前回の時刻。
    v_last: Instant,
}

/// ジェスチャーの状態。
#[derive(Debug, Default)]
pub struct Gestures {
    /// 進行中のスワイプ。
    pub swipe: Option<Swipe>,
    /// DnD の端スクロール。
    pub dnd: Option<DndScroll>,
    /// 縦のスワイプでワークスペースが指についてくる状態（描画に使う）。
    pub follow: Option<WsFollow>,
}

impl Contrast {
    /// スワイプが始まった。3 本指と 4 本指はコンポジタが使う（true）。
    pub fn swipe_begin(&mut self, fingers: i32) -> bool {
        match fingers {
            3 => {
                self.gestures.swipe = Some(Swipe::Undecided(0.0, 0.0));
                true
            }
            4 => {
                let start = self.overview_progress_target();
                self.gestures.swipe = Some(Swipe::Overview {
                    tracker: SwipeTracker::default(),
                    start,
                });
                self.overview_gesture_begin();
                true
            }
            _ => false,
        }
    }

    /// スワイプの移動（`dx`・`dy` は加速前、natural-scroll なら反転済み。`uninverted_dy` は反転前の縦）。
    pub fn swipe_update(&mut self, dx: f64, dy: f64, uninverted_dy: f64, t: Duration) {
        let Some(swipe) = self.gestures.swipe.take() else {
            return;
        };
        let next = match swipe {
            Swipe::Undecided(cx, cy) => {
                let (cx, cy) = (cx + dx, cy + dy);
                if cx * cx + cy * cy < DECIDE_DISTANCE * DECIDE_DISTANCE {
                    Some(Swipe::Undecided(cx, cy))
                } else {
                    self.decide_swipe(cx, cy)
                }
            }
            other => Some(other),
        };
        self.gestures.swipe = next;
        match self.gestures.swipe.as_mut() {
            Some(Swipe::View { tracker, norm }) => {
                tracker.push(dx, t);
                let offset = (tracker.pos() * *norm).round() as i32;
                self.gesture_command(Command::ViewGestureUpdate { offset });
            }
            Some(Swipe::Workspace {
                tracker,
                start,
                count,
                ..
            }) => {
                tracker.push(dy, t);
                let offset = workspace_follow(*start, *count, tracker.pos());
                self.gestures.follow = Some(WsFollow::Gesture(offset));
            }
            Some(Swipe::Overview { tracker, start }) => {
                // niri は反転前の縦の移動の符号を逆にして使う（上へで開く）。
                tracker.push(-uninverted_dy, t);
                let p = *start + tracker.pos() / crate::overview::GESTURE_MOVEMENT;
                self.overview_gesture_update(p);
            }
            _ => {}
        }
    }

    /// 向きを決める: 横ならビュー、縦ならワークスペースの切り替え（ポインタのある出力で）。
    fn decide_swipe(&mut self, cx: f64, cy: f64) -> Option<Swipe> {
        let pointer = self.seat.get_pointer()?.current_location();
        let output = self.output_at(pointer.x, pointer.y)?;
        let key = self.key_of_output(&output)?;
        if cx.abs() > cy.abs() {
            let width = self
                .work_area(&output)
                .map_or(1.0, |a| f64::from(a.width()));
            self.gesture_command(Command::ViewGestureBegin {
                output: key,
                dnd: false,
            });
            Some(Swipe::View {
                tracker: SwipeTracker::default(),
                norm: width / VIEW_MOVEMENT,
            })
        } else {
            let start = usize::from(self.shown_position(&output)?);
            let id = self.core_output(&output)?;
            let count = self.core.view().output_workspaces(id).len();
            Some(Swipe::Workspace {
                tracker: SwipeTracker::default(),
                output: key,
                start,
                count,
            })
        }
    }

    /// スワイプが終わった。
    pub fn swipe_end(&mut self, now: Duration) {
        match self.gestures.swipe.take() {
            Some(Swipe::View { mut tracker, norm }) => {
                tracker.push(0.0, now);
                let projected_offset = (tracker.projected_end_pos() * norm).round() as i32;
                // 吸着の動きに指の速度を引き継ぐ（niri は吸着のバネの初速に使う）。窓はビューと逆向きに動く。
                self.anim.set_pending_velocity_x(-tracker.velocity() * norm);
                self.gesture_command(Command::ViewGestureEnd { projected_offset });
            }
            Some(Swipe::Workspace {
                mut tracker,
                output,
                start,
                count,
            }) => {
                tracker.push(0.0, now);
                let target = workspace_target(start, count, tracker.projected_end_pos());
                tracing::info!(start, target, "gesture: workspace switch");
                // 離した位置から目標まで、指の速さを引き継いでバネで動かす（目標が表示中になったあとのずれで書く）。
                let shift = target as f64 - start as f64;
                let from = workspace_follow(start, count, tracker.pos()) - shift;
                self.gestures.follow = Some(WsFollow::Settling {
                    spring: Spring {
                        from,
                        to: 0.0,
                        initial_velocity: workspace_release_velocity(
                            start,
                            count,
                            tracker.pos(),
                            tracker.velocity(),
                        ),
                        params: SpringParams::workspace_switch(),
                    },
                    start: Instant::now(),
                });
                if target != start {
                    // 切り替えは、その出力をアクティブにしてから（core の SwitchWorkspace はアクティブな出力の列で動く）。
                    if self.core.view().active_output() != self.core.view().output_id(output) {
                        self.gesture_command(Command::OutputFocusRequested(output));
                    }
                    let index = WorkspaceIndex::new(u8::try_from(target).unwrap_or(u8::MAX));
                    self.core_action(contrast_core::Action::SwitchWorkspace(index));
                }
            }
            Some(Swipe::Overview { mut tracker, start }) => {
                tracker.push(0.0, now);
                let projected =
                    start + tracker.projected_end_pos() / crate::overview::GESTURE_MOVEMENT;
                self.overview_gesture_end(projected);
            }
            _ => {}
        }
    }

    /// ジェスチャーの命令を core に渡す（窓は補間せず、指についていく）。
    fn gesture_command(&mut self, command: Command) {
        self.anim
            .suppress(matches!(command, Command::ViewGestureUpdate { .. }));
        self.submit_command(command);
        self.anim.suppress(false);
    }

    /// DnD が始まった。
    pub fn dnd_started(&mut self) {
        let pos = self
            .seat
            .get_pointer()
            .map(|p| p.current_location())
            .unwrap_or_default();
        let now = Instant::now();
        self.gestures.dnd = Some(DndScroll {
            pos,
            last: now,
            nonzero_since: None,
            tracker: SwipeTracker::default(),
            started: now,
            begun: false,
            ws_offset: 0.0,
            v_nonzero_since: None,
            v_last: now,
        });
    }

    /// DnD 中のポインタの移動。
    pub fn dnd_motion(&mut self, pos: Point<f64, Logical>) {
        let begin = match self.gestures.dnd.as_mut() {
            Some(d) => {
                d.pos = pos;
                !d.begun
            }
            None => return,
        };
        if begin
            && let Some(output) = self.output_at(pos.x, pos.y)
            && let Some(key) = self.key_of_output(&output)
        {
            self.gesture_command(Command::ViewGestureBegin {
                output: key,
                dnd: true,
            });
            if let Some(d) = self.gestures.dnd.as_mut() {
                d.begun = true;
            }
        }
    }

    /// 毎フレーム: DnD の端スクロールを進める。
    pub fn dnd_tick(&mut self) {
        self.dnd_vertical_tick();
        let Some(d) = self.gestures.dnd.as_ref() else {
            return;
        };
        if !d.begun {
            return;
        }
        let pos = d.pos;
        let Some(output) = self.output_at(pos.x, pos.y) else {
            return;
        };
        let Some(area) = self.work_area(&output) else {
            return;
        };
        let delta = dnd_edge_delta(pos.x - f64::from(area.x()), f64::from(area.width()));
        let now = Instant::now();
        let Some(d) = self.gestures.dnd.as_mut() else {
            return;
        };
        let dt = now.duration_since(d.last).as_secs_f64();
        d.last = now;
        if delta == 0.0 {
            d.nonzero_since = None;
            return;
        }
        let since = *d.nonzero_since.get_or_insert(now);
        if now.duration_since(since) < DND_DELAY {
            return;
        }
        let elapsed = now.duration_since(d.started);
        d.tracker.push(delta * dt * DND_MAX_SPEED, elapsed);
        let offset = d.tracker.pos().round() as i32;
        self.gesture_command(Command::ViewGestureUpdate { offset });
    }

    /// オーバービューが開いている間の DnD の縦の端スクロール: ポインタが上下の端にいる間、ワークスペースの位置を進める。
    fn dnd_vertical_tick(&mut self) {
        if !self.overview.open {
            return;
        }
        let Some(d) = self.gestures.dnd.as_ref() else {
            return;
        };
        let pos = d.pos;
        let Some(output) = self.output_at(pos.x, pos.y) else {
            return;
        };
        let (Some(area), Some(geo), Some(shown)) = (
            self.work_area(&output),
            self.space.output_geometry(&output),
            self.shown_position(&output),
        ) else {
            return;
        };
        let count = self
            .core_output(&output)
            .map_or(1, |id| self.core.view().output_workspaces(id).len());
        let zoom = crate::overview::zoom_at(self.overview.value());
        let strip = f64::from(geo.size.w) * zoom;
        let x = pos.x - f64::from(geo.loc.x) - (f64::from(geo.size.w) - strip) / 2.0;
        let delta = dnd_vertical_delta(
            x,
            strip,
            pos.y - f64::from(area.y()),
            f64::from(area.height()),
        );
        let now = Instant::now();
        let Some(d) = self.gestures.dnd.as_mut() else {
            return;
        };
        let dt = now.duration_since(d.v_last).as_secs_f64();
        d.v_last = now;
        if delta == 0.0 {
            d.v_nonzero_since = None;
            return;
        }
        let since = *d.v_nonzero_since.get_or_insert(now);
        if now.duration_since(since) < DND_DELAY {
            return;
        }
        // 範囲の外へ引いても溜まらない（端で止める）。
        let min = -(usize::from(shown) as f64);
        let max = count.saturating_sub(1) as f64 - usize::from(shown) as f64;
        d.ws_offset =
            (d.ws_offset + delta * dt * DND_MAX_SPEED / DND_WORKSPACE_MOVEMENT).clamp(min, max);
        self.gestures.follow = Some(WsFollow::Gesture(d.ws_offset));
    }

    /// DnD が終わった（落とした・取りやめた）。
    pub fn dnd_ended(&mut self) {
        let Some(mut d) = self.gestures.dnd.take() else {
            return;
        };
        if d.ws_offset != 0.0 && matches!(self.gestures.follow, Some(WsFollow::Gesture(_))) {
            // 縦の端スクロールをしていた: いちばん近いワークスペースへ切り替え、そこまでバネで動かす。
            let output = self.active_output();
            let shown = output
                .as_ref()
                .and_then(|o| self.shown_position(o))
                .map_or(0, usize::from);
            let count = output
                .as_ref()
                .and_then(|o| self.core_output(o))
                .map_or(1, |id| self.core.view().output_workspaces(id).len());
            let target = (shown as f64 + d.ws_offset)
                .round()
                .clamp(0.0, count.saturating_sub(1) as f64) as usize;
            if target != shown {
                let index = WorkspaceIndex::new(u8::try_from(target).unwrap_or(u8::MAX));
                self.core_action(contrast_core::Action::SwitchWorkspace(index));
            }
            self.gestures.follow = Some(WsFollow::Settling {
                spring: Spring {
                    from: d.ws_offset - (target as f64 - shown as f64),
                    to: 0.0,
                    initial_velocity: 0.0,
                    params: SpringParams::workspace_switch(),
                },
                start: Instant::now(),
            });
        }
        if !d.begun {
            return;
        }
        let elapsed = d.started.elapsed();
        d.tracker.push(0.0, elapsed);
        // 動かなかったなら移動量 0（core はビューを保つ。D88）。
        let projected_offset = if d.tracker.pos() == 0.0 {
            0
        } else {
            d.tracker.projected_end_pos().round() as i32
        };
        self.gesture_command(Command::ViewGestureEnd { projected_offset });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn the_tracker_keeps_150ms_and_projects_with_deceleration() {
        let mut t = SwipeTracker::default();
        t.push(10.0, ms(0));
        t.push(10.0, ms(100));
        t.push(10.0, ms(200));
        assert_eq!(t.pos(), 30.0);
        // 0 ms の分は 150 ms より古いので外れる: (10 + 10) / 0.1 s = 200 px/s。
        assert!((t.velocity() - 200.0).abs() < 1e-9);
        let expected = 30.0 - 200.0 / (1000.0 * DECELERATION_TOUCHPAD.ln());
        assert!((t.projected_end_pos() - expected).abs() < 1e-9);
        assert!(t.projected_end_pos() > 30.0, "減速しても先へ進む");
        // 時刻が戻るものは捨てる。
        t.push(100.0, ms(50));
        assert_eq!(t.pos(), 30.0);
    }

    #[test]
    fn workspace_switches_are_limited_to_one_step() {
        assert_eq!(workspace_target(2, 5, 0.0), 2);
        assert_eq!(workspace_target(2, 5, 140.0), 2);
        assert_eq!(workspace_target(2, 5, 160.0), 3);
        assert_eq!(workspace_target(2, 5, 2000.0), 3);
        assert_eq!(workspace_target(2, 5, -2000.0), 1);
        assert_eq!(workspace_target(0, 3, -500.0), 0);
        assert_eq!(workspace_target(2, 3, 900.0), 2);
    }

    #[test]
    fn dnd_edges_scroll_harder_near_the_edge() {
        assert_eq!(dnd_edge_delta(500.0, 1000.0), 0.0);
        assert_eq!(dnd_edge_delta(0.0, 1000.0), -1.0);
        assert_eq!(dnd_edge_delta(15.0, 1000.0), -0.5);
        assert_eq!(dnd_edge_delta(1000.0, 1000.0), 1.0);
        assert_eq!(dnd_edge_delta(985.0, 1000.0), 0.5);
        assert_eq!(dnd_edge_delta(-50.0, 1000.0), -1.0);
        assert_eq!(dnd_edge_delta(5.0, 0.0), 0.0);
    }
}

impl Contrast {
    /// 縦のスワイプの途中・離したあとの、表示中のワークスペースからのずれ。無ければ `None`（落ち着いたら捨てる）。
    pub fn workspace_follow_offset(&self) -> Option<f64> {
        self.gestures
            .follow
            .filter(|f| !f.is_settled())
            .map(|f| f.value())
    }
}

#[cfg(test)]
mod follow_tests {
    use super::*;

    #[test]
    fn the_vertical_edge_scroll_follows_niris_trigger_height() {
        // 高さ 1000 の作業領域、帯は x 0..900。上の端 50 px（上端で −1、50 px の位置で 0）、下も同じ。
        assert_eq!(dnd_vertical_delta(10.0, 900.0, 0.0, 1000.0), -1.0);
        assert_eq!(dnd_vertical_delta(10.0, 900.0, 25.0, 1000.0), -0.5);
        assert_eq!(dnd_vertical_delta(10.0, 900.0, 500.0, 1000.0), 0.0);
        assert_eq!(dnd_vertical_delta(10.0, 900.0, 1000.0, 1000.0), 1.0);
        // 帯の外（横）は 0。
        assert_eq!(dnd_vertical_delta(-1.0, 900.0, 0.0, 1000.0), 0.0);
        assert_eq!(dnd_vertical_delta(900.0, 900.0, 0.0, 1000.0), 0.0);
    }

    #[test]
    fn the_view_follows_the_finger_one_workspace_per_300_px() {
        assert!((workspace_follow(1, 3, 150.0) - 0.5).abs() < 1e-12);
        assert!((workspace_follow(1, 3, -300.0) + 1.0).abs() < 1e-12);
    }

    #[test]
    fn past_the_neighbour_or_the_end_it_is_rubber_banded() {
        // 隣（+1）を越えると、+1 より少しだけ（限界 0.05 まで）動く。
        let over = workspace_follow(1, 3, 900.0);
        assert!(over > 1.0 && over < 1.05, "{over}");
        // 一番上（start 0）で下へ引くと、−1 ではなく、0 より少しだけ。
        let top = workspace_follow(0, 3, -300.0);
        assert!(top < 0.0 && top > -0.05, "{top}");
        // 一番下（start = count − 1）で上へ引く。
        let bottom = workspace_follow(2, 3, 300.0);
        assert!(bottom > 0.0 && bottom < 0.05, "{bottom}");
    }

    #[test]
    fn the_release_velocity_is_damped_only_past_the_range() {
        assert!((workspace_release_velocity(1, 3, 100.0, 600.0) - 2.0).abs() < 1e-12);
        assert!(workspace_release_velocity(1, 3, 900.0, 600.0) < 2.0);
    }

    #[test]
    fn a_settling_spring_ends_at_zero() {
        let f = WsFollow::Settling {
            spring: Spring {
                from: 0.4,
                to: 0.0,
                initial_velocity: 0.0,
                params: SpringParams::workspace_switch(),
            },
            start: Instant::now() - Duration::from_secs(5),
        };
        assert!(f.is_settled());
        assert!(f.value().abs() < 1e-3);
    }
}
