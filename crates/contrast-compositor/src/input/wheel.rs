//! マウスのホイールのバインド（niri の `Mod+WheelScrollDown` など）の部品（純粋）。
//!
//! niri は、ホイールの軸イベントを、押している修飾キーに完全に一致するホイールのバインドがあるときだけ消費する。
//! 軸の値（`v120`。1 目盛りが 120）を足し込んで目盛りの数にし、目盛りごとにバインドを実行する。
//! バインドの `cooldown-ms` は、最初の実行から、その時間のあいだ同じバインドを実行しない。

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use contrast_policy::Mods;

/// ホイール 1 目盛りの `v120` の値。
pub(crate) const WHEEL_TICK_V120: f64 = 120.0;

/// 軸の値を足し込んで、目盛りの数にする（niri の `ScrollTracker` と同じ）。
///
/// 向きが変わったら足し込みを捨てる。1 回に数えるのは最大 127 目盛り。端数は次に持ち越す。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScrollTracker {
    /// 1 目盛りの値。
    tick: f64,
    /// 直前の入力。
    last: f64,
    /// 足し込み。
    acc: f64,
}

impl ScrollTracker {
    /// 1 目盛りが `tick` のトラッカーを作る。
    pub(crate) const fn new(tick: f64) -> Self {
        Self {
            tick,
            last: 0.0,
            acc: 0.0,
        }
    }

    /// `amount` を足して、確定した目盛りの数を返す。正は下／右、負は上／左。
    pub(crate) fn accumulate(&mut self, amount: f64) -> i32 {
        let changed_direction =
            (self.last > 0.0 && amount < 0.0) || (self.last < 0.0 && amount > 0.0);
        if changed_direction {
            self.acc = 0.0;
        }
        self.last = amount;
        self.acc += amount;

        if self.acc.abs() < self.tick {
            return 0;
        }
        let clamped = self.acc.clamp(-127.0 * self.tick, 127.0 * self.tick);
        // `tick` は正の定数。切り捨てて整数にする。
        let ticks = (clamped / self.tick).trunc();
        self.acc %= self.tick;
        ticks as i32
    }

    /// 足し込みを捨てる（ホイールのバインドが当たらない修飾キーのとき）。
    pub(crate) fn reset(&mut self) {
        self.last = 0.0;
        self.acc = 0.0;
    }
}

/// バインドの識別（修飾キー、キー名）。
type BindId = (bool, bool, bool, bool, String);

/// バインドの `cooldown-ms` の管理。
#[derive(Debug, Default)]
pub(crate) struct CooldownGate {
    /// バインドごとの、最後に実行した時刻。
    last: BTreeMap<BindId, Instant>,
}

impl CooldownGate {
    /// バインド `(mods, key)` を `now` に実行してよいか。よければ時刻を覚える。
    /// `cooldown` が `None` なら常によい。
    pub(crate) fn allow(
        &mut self,
        mods: Mods,
        key: &str,
        cooldown: Option<Duration>,
        now: Instant,
    ) -> bool {
        let Some(cooldown) = cooldown else {
            return true;
        };
        let id = (
            mods.alt,
            mods.shift,
            mods.ctrl,
            mods.logo,
            key.to_ascii_lowercase(),
        );
        if let Some(last) = self.last.get(&id)
            && now.saturating_duration_since(*last) < cooldown
        {
            return false;
        }
        self.last.insert(id, now);
        true
    }
}

/// 目盛りの数 `ticks` を、押す向きの回数に分ける（`(正の向きの回数, 負の向きの回数)`）。
pub(crate) fn directions(ticks: i32) -> (u32, u32) {
    (ticks.max(0).unsigned_abs(), ticks.min(0).unsigned_abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker() -> ScrollTracker {
        ScrollTracker::new(WHEEL_TICK_V120)
    }

    #[test]
    fn one_notch_is_one_tick() {
        assert_eq!(tracker().accumulate(120.0), 1);
        assert_eq!(tracker().accumulate(-120.0), -1);
    }

    #[test]
    fn small_amounts_accumulate_and_the_remainder_carries_over() {
        let mut t = tracker();
        assert_eq!(t.accumulate(60.0), 0);
        assert_eq!(t.accumulate(60.0), 1);
        assert_eq!(t.accumulate(60.0), 0);
        assert_eq!(t.accumulate(90.0), 1);
        // 150 を足して 120 を引いた 30 が残っている。
        assert_eq!(t.accumulate(90.0), 1);
    }

    #[test]
    fn changing_direction_drops_the_accumulation() {
        let mut t = tracker();
        assert_eq!(t.accumulate(100.0), 0);
        assert_eq!(t.accumulate(-100.0), 0);
        assert_eq!(t.accumulate(-20.0), -1);
    }

    #[test]
    fn several_notches_at_once_and_the_clamp() {
        assert_eq!(tracker().accumulate(360.0), 3);
        assert_eq!(tracker().accumulate(-240.0), -2);
        assert_eq!(tracker().accumulate(120.0 * 1000.0), 127);
        assert_eq!(tracker().accumulate(-120.0 * 1000.0), -127);
    }

    #[test]
    fn reset_forgets_everything() {
        let mut t = tracker();
        assert_eq!(t.accumulate(100.0), 0);
        t.reset();
        assert_eq!(t.accumulate(100.0), 0);
        assert_eq!(t.accumulate(20.0), 1);
    }

    #[test]
    fn directions_split_the_sign() {
        assert_eq!(directions(3), (3, 0));
        assert_eq!(directions(-2), (0, 2));
        assert_eq!(directions(0), (0, 0));
        assert_eq!(directions(i32::MIN), (0, 2_147_483_648));
    }

    #[test]
    fn the_cooldown_blocks_for_the_duration_from_the_first_run() {
        let mut gate = CooldownGate::default();
        let m = Mods {
            logo: true,
            ..Mods::default()
        };
        let t0 = Instant::now();
        let cd = Some(Duration::from_millis(150));
        assert!(gate.allow(m, "WheelScrollDown", cd, t0));
        assert!(!gate.allow(m, "wheelscrolldown", cd, t0 + Duration::from_millis(100)));
        // 拒否された入力では、時刻は延びない（最初の実行から 150ms）。
        assert!(gate.allow(m, "WheelScrollDown", cd, t0 + Duration::from_millis(150)));
        assert!(!gate.allow(m, "WheelScrollDown", cd, t0 + Duration::from_millis(200)));
    }

    #[test]
    fn the_cooldown_is_per_bind_and_optional() {
        let mut gate = CooldownGate::default();
        let m = Mods::default();
        let ctrl = Mods { ctrl: true, ..m };
        let t0 = Instant::now();
        let cd = Some(Duration::from_secs(1));
        assert!(gate.allow(m, "WheelScrollDown", cd, t0));
        assert!(gate.allow(ctrl, "WheelScrollDown", cd, t0));
        assert!(gate.allow(m, "WheelScrollUp", cd, t0));
        // 間引き指定が無ければ、いつでもよく、時刻も覚えない。
        assert!(gate.allow(m, "WheelScrollLeft", None, t0));
        assert!(gate.allow(m, "WheelScrollLeft", None, t0));
    }
}
