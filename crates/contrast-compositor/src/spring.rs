//! バネのアニメーション（減衰振動）の純粋な計算。niri の既定のアニメーション（窓の移動・リサイズ・ビューの横移動・
//! ワークスペースの切り替え）が使う曲線に合わせる。
//!
//! 位置 `x(t)` は、質量 `m`、バネ定数 `k`、減衰 `b` の減衰振動 `m x'' + b x' + k (x − to) = 0`（初期値 `x(0) = from`、
//! `x'(0) = v0`）の解。減衰比 `ζ = b / (2 √(m k))` で 3 通りの閉形式になる（不足減衰・臨界減衰・過減衰）。
//! niri の設定は減衰比と stiffness（`k`）、質量は 1。ここは物理の式から書いたもので、検証は数値積分との比較で行う。
//! 時刻は秒（`f64`）で渡す。この型は時計を読まない。

use std::time::Duration;

/// バネのパラメータ。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringParams {
    /// 減衰 `b`。
    damping: f64,
    /// 質量 `m`（常に 1）。
    mass: f64,
    /// バネ定数 `k`（niri の `stiffness`）。
    stiffness: f64,
    /// 収束とみなす、目標との差。
    epsilon: f64,
}

impl SpringParams {
    /// 減衰比 `damping_ratio`、`stiffness`、`epsilon` から作る。負の値は 0 にする。質量は 1。
    pub fn new(damping_ratio: f64, stiffness: f64, epsilon: f64) -> Self {
        let mass = 1.0;
        let stiffness = stiffness.max(0.0);
        let critical = 2.0 * (mass * stiffness).sqrt();
        Self {
            damping: damping_ratio.max(0.0) * critical,
            mass,
            stiffness,
            epsilon: epsilon.max(0.0),
        }
    }

    /// niri の既定の窓の移動・リサイズ・ビューの横移動（減衰比 1.0、stiffness 800、epsilon 0.0001）。
    pub fn movement() -> Self {
        Self::new(1.0, 800.0, 0.0001)
    }

    /// niri の既定のワークスペースの切り替え（減衰比 1.0、stiffness 1000、epsilon 0.0001）。
    pub fn workspace_switch() -> Self {
        Self::new(1.0, 1000.0, 0.0001)
    }
}

/// `from` から `to` へ動くバネ。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    /// 始まりの値。
    pub from: f64,
    /// 行き先の値。
    pub to: f64,
    /// 始まりの速度。
    pub initial_velocity: f64,
    /// パラメータ。
    pub params: SpringParams,
}

impl Spring {
    /// 0 から 1 へ、速度 0 で動くバネ（進み具合を表す）。
    pub fn progress(params: SpringParams) -> Self {
        Self {
            from: 0.0,
            to: 1.0,
            initial_velocity: 0.0,
            params,
        }
    }

    /// 時刻 `t`（秒、0 以上）の値。
    pub fn value_at(&self, t: f64) -> f64 {
        let t = t.max(0.0);
        let m = self.params.mass;
        let beta = self.params.damping / (2.0 * m);
        let omega0 = (self.params.stiffness / m).sqrt();
        let x0 = self.from - self.to;
        let v0 = self.initial_velocity;
        let envelope = (-beta * t).exp();
        let value = if (beta - omega0).abs() <= f64::from(f32::EPSILON) {
            // 臨界減衰。
            envelope * (x0 + (beta * x0 + v0) * t)
        } else if beta < omega0 {
            // 不足減衰。
            let w = (omega0 * omega0 - beta * beta).sqrt();
            envelope * (x0 * (w * t).cos() + (beta * x0 + v0) / w * (w * t).sin())
        } else {
            // 過減衰。
            let w = (beta * beta - omega0 * omega0).sqrt();
            // 2 つの指数の和で書く（cosh／sinh と exp の積は、t が大きいと inf × 0 で NaN になる）。
            let slow = (v0 + (beta + w) * x0) / (2.0 * w);
            let fast = x0 - slow;
            slow * ((w - beta) * t).exp() + fast * (-(beta + w) * t).exp()
        };
        self.to + value
    }

    /// 目標に初めて収束する（`to` との差が `epsilon` 以内になり、`to` を超えてもいない）までの時間。
    /// 1 ミリ秒刻みで最大 3 秒まで探し、見つからなければ 3 秒。動かない（`from == to`）なら 0。
    pub fn settle_time(&self) -> Duration {
        if (self.to - self.from).abs() <= f64::EPSILON {
            return Duration::ZERO;
        }
        for ms in 1..=3000_u32 {
            let y = self.value_at(f64::from(ms) / 1000.0);
            if (self.to - y).abs() <= self.params.epsilon {
                return Duration::from_millis(u64::from(ms));
            }
        }
        Duration::from_secs(3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spring(damping_ratio: f64, stiffness: f64, from: f64, to: f64, v0: f64) -> Spring {
        Spring {
            from,
            to,
            initial_velocity: v0,
            params: SpringParams::new(damping_ratio, stiffness, 0.0001),
        }
    }

    /// 参照モデル: 運動方程式 `x'' = −(k/m)(x − to) − (b/m) x'` を、細かい刻みの数値積分（速度ベルレ法）で解く。
    fn integrate(s: &Spring, t: f64) -> f64 {
        let (m, b, k) = (s.params.mass, s.params.damping, s.params.stiffness);
        let dt = 1e-5;
        let steps = (t / dt).round() as u64;
        let (mut x, mut v) = (s.from, s.initial_velocity);
        let accel = |x: f64, v: f64| -(k / m) * (x - s.to) - (b / m) * v;
        for _ in 0..steps {
            // 中点法（2 次精度）。
            let a = accel(x, v);
            let (xm, vm) = (x + v * dt / 2.0, v + a * dt / 2.0);
            let am = accel(xm, vm);
            x += vm * dt;
            v += am * dt;
        }
        x
    }

    #[test]
    fn it_starts_at_from_and_ends_at_to() {
        for ratio in [0.3, 0.6, 1.0, 1.4, 4.0] {
            let s = spring(ratio, 800.0, 0.0, 1.0, 0.0);
            assert!((s.value_at(0.0) - 0.0).abs() < 1e-12, "{ratio}");
            assert!((s.value_at(10.0) - 1.0).abs() < 1e-6, "{ratio}");
        }
    }

    #[test]
    fn the_closed_form_matches_numerical_integration_in_every_regime() {
        // 不足減衰・臨界減衰・過減衰、初速あり／なし、下向き。
        for (ratio, k, from, to, v0) in [
            (0.5, 800.0, 0.0, 1.0, 0.0),
            (1.0, 800.0, 0.0, 1.0, 0.0),
            (1.0, 1000.0, 3.0, -2.0, 0.0),
            (2.5, 800.0, 0.0, 1.0, 0.0),
            (0.8, 500.0, 1.0, 0.0, 5.0),
            (1.0, 800.0, 0.0, 1.0, -4.0),
        ] {
            let s = spring(ratio, k, from, to, v0);
            for t in [0.01, 0.05, 0.1, 0.25, 0.5] {
                let (closed, numeric) = (s.value_at(t), integrate(&s, t));
                assert!(
                    (closed - numeric).abs() < 1e-3,
                    "ratio={ratio} k={k} t={t}: closed={closed} numeric={numeric}"
                );
            }
        }
    }

    #[test]
    fn a_critically_damped_spring_never_overshoots_and_rises_monotonically() {
        for k in [800.0, 1000.0] {
            let s = spring(1.0, k, 0.0, 1.0, 0.0);
            let mut last = 0.0;
            for i in 0..=2000 {
                let v = s.value_at(f64::from(i) / 1000.0);
                assert!(v >= last - 1e-12 && v <= 1.0 + 1e-12, "k={k} i={i} v={v}");
                last = v;
            }
        }
    }

    #[test]
    fn an_underdamped_spring_overshoots() {
        let s = spring(0.4, 800.0, 0.0, 1.0, 0.0);
        let peak = (0..1000)
            .map(|i| s.value_at(f64::from(i) / 1000.0))
            .fold(0.0_f64, f64::max);
        assert!(peak > 1.01, "{peak}");
    }

    #[test]
    fn the_settle_time_is_short_for_the_niri_defaults_and_stiffer_is_faster() {
        let movement = Spring::progress(SpringParams::movement()).settle_time();
        let workspace = Spring::progress(SpringParams::workspace_switch()).settle_time();
        // 臨界減衰で stiffness 800 なら、約 0.4 秒で 0.0001 以内になる。
        assert!(
            movement > Duration::from_millis(250) && movement < Duration::from_millis(600),
            "{movement:?}"
        );
        assert!(workspace < movement, "{workspace:?} < {movement:?}");
        // その時刻には本当に収束している。
        let s = Spring::progress(SpringParams::movement());
        assert!((1.0 - s.value_at(movement.as_secs_f64())).abs() <= 0.0001 + 1e-9);
        // 動かないバネは 0。
        assert_eq!(
            spring(1.0, 800.0, 2.0, 2.0, 0.0).settle_time(),
            Duration::ZERO
        );
    }

    #[test]
    fn degenerate_parameters_do_not_produce_nan() {
        for s in [
            spring(0.0, 800.0, 0.0, 1.0, 0.0),
            spring(1.0, 0.0, 0.0, 1.0, 0.0),
            spring(-1.0, -5.0, 0.0, 1.0, 0.0),
            spring(1.0, 800.0, 1.0, 1.0, 0.0),
        ] {
            for t in [0.0, 0.1, 1.0, 100.0] {
                assert!(s.value_at(t).is_finite(), "{s:?} {t}");
            }
            let _ = s.settle_time();
        }
    }

    #[test]
    fn negative_time_is_the_start() {
        let s = spring(1.0, 800.0, 5.0, 1.0, 0.0);
        assert_eq!(s.value_at(-1.0), s.value_at(0.0));
    }
}
