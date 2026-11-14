//! 動き（Motion）の設定の値と、純粋な補間の関数（U3。`docs/ui-spec.md` §4.2・§5）。
//!
//! CDL・GL・時計を知らない。時刻は `elapsed`（経過時間）として引数で受ける。
//!
//! 守る不変条件:
//! - INV-T1: easing 系の長さは `MAX_MOTION_MS`（400ms）以下。設定で上げられない。`niriSpring` は長さを持たず対象外。
//! - INV-M1: 動きは 6 種別（`MotionKind`）。命令から種別は `kind_of` が 1 つに決める。

use std::time::Duration;

use contrast_core::Action;

/// easing 系の長さの上限（ミリ秒。INV-T1）。設定で上げられない。
pub const MAX_MOTION_MS: u32 = 400;

/// 動きの種別（INV-M1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionKind {
    /// 平行移動（スクロール・窓の移動・列の並べ替え）。
    Scroll,
    /// 大きさの変化。
    Resize,
    /// ワークスペースの縦移動。
    Workspace,
    /// フォーカスの明度遷移（減光の補間）。
    Focus,
    /// 窓が開く。
    Open,
    /// 窓が閉じる。
    Close,
}

/// easing の曲線。`f(0) = 0`、`f(1) = 1`、単調非減少。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EasingCurve {
    /// `1 − (1−t)^3`（既定）。
    #[default]
    EaseOutCubic,
    /// `1 − (1−t)^2`。
    EaseOutQuad,
    /// `1 − 2^(−10t)`（端点を厳密にそろえる）。
    EaseOutExpo,
    /// 前半 `4t^3`、後半 `1 − (−2t+2)^3 / 2`。
    EaseInOutCubic,
}

/// 動きの曲線。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionCurve {
    /// 時間で決まる easing。長さは `MotionSpec::duration_ms`。
    Easing(EasingCurve),
    /// niri のバネ（互換専用）。長さを持たず（物理で決まる）、INV-T1 の対象外。`Focus` には使えない。
    NiriSpring,
}

/// 1 つの種別の動き。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotionSpec {
    /// 曲線。
    pub curve: MotionCurve,
    /// 長さ（ミリ秒。0〜`MAX_MOTION_MS`。0 は動きなし）。`NiriSpring` では使わず 0。
    pub duration_ms: u32,
}

impl MotionSpec {
    /// 種別ごとの既定の長さ（ミリ秒）。`Focus` は 120、ほかは 150。
    pub fn default_duration_ms(kind: MotionKind) -> u32 {
        match kind {
            MotionKind::Focus => 120,
            _ => 150,
        }
    }

    /// 種別の既定（`easeOutCubic` と既定の長さ）。
    pub fn default_for(kind: MotionKind) -> Self {
        Self {
            curve: MotionCurve::Easing(EasingCurve::EaseOutCubic),
            duration_ms: Self::default_duration_ms(kind),
        }
    }

    /// 長さ。`NiriSpring` は長さを持たない（`None`）。
    pub fn duration(&self) -> Option<Duration> {
        match self.curve {
            MotionCurve::Easing(_) => Some(Duration::from_millis(u64::from(self.duration_ms))),
            MotionCurve::NiriSpring => None,
        }
    }

    /// 補間しない（即時）か。easing で長さが 0 のとき。
    pub fn is_instant(&self) -> bool {
        matches!(self.curve, MotionCurve::Easing(_)) && self.duration_ms == 0
    }
}

/// 種別ごとの動き。`None` の種別は即時（補間しない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MotionSettings {
    /// 平行移動。
    pub scroll: Option<MotionSpec>,
    /// 大きさの変化。
    pub resize: Option<MotionSpec>,
    /// ワークスペースの縦移動。
    pub workspace: Option<MotionSpec>,
    /// 明度遷移。
    pub focus: Option<MotionSpec>,
    /// 窓が開く。
    pub open: Option<MotionSpec>,
    /// 窓が閉じる。
    pub close: Option<MotionSpec>,
}

impl MotionSettings {
    /// 種別の動き。`None` は即時。
    pub fn spec(&self, kind: MotionKind) -> Option<MotionSpec> {
        match kind {
            MotionKind::Scroll => self.scroll,
            MotionKind::Resize => self.resize,
            MotionKind::Workspace => self.workspace,
            MotionKind::Focus => self.focus,
            MotionKind::Open => self.open,
            MotionKind::Close => self.close,
        }
    }

    /// 種別の動きを置く。
    pub fn set(&mut self, kind: MotionKind, spec: MotionSpec) {
        let slot = match kind {
            MotionKind::Scroll => &mut self.scroll,
            MotionKind::Resize => &mut self.resize,
            MotionKind::Workspace => &mut self.workspace,
            MotionKind::Focus => &mut self.focus,
            MotionKind::Open => &mut self.open,
            MotionKind::Close => &mut self.close,
        };
        *slot = Some(spec);
    }
}

/// 曲線 `curve` の `t`（0〜1。範囲外は収める）での値。端点は厳密（`f(0) = 0`、`f(1) = 1`）。
pub fn ease(curve: EasingCurve, t: f32) -> f32 {
    // NaN と範囲外は端点に収める（端点は厳密）。
    if t.is_nan() || t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    let v = match curve {
        EasingCurve::EaseOutCubic => 1.0 - (1.0 - t).powi(3),
        EasingCurve::EaseOutQuad => 1.0 - (1.0 - t).powi(2),
        EasingCurve::EaseOutExpo => 1.0 - 2f32.powf(-10.0 * t),
        EasingCurve::EaseInOutCubic => {
            if t < 0.5 {
                4.0 * t * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
            }
        }
    };
    v.clamp(0.0, 1.0)
}

/// 進行度（0〜1）。`duration` が 0 なら 1。`elapsed >= duration` で 1。
pub fn progress(elapsed: Duration, duration: Duration) -> f32 {
    if duration.is_zero() || elapsed >= duration {
        return 1.0;
    }
    (elapsed.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0)
}

/// 明度の乗数 `from` から `to` への補間。`t` は進行度（0〜1）で、`curve` を通して混ぜる。
/// `t <= 0` で厳密に `from`、`t >= 1` で厳密に `to`。結果は `from` と `to` の間。
pub fn dim_at(from: f32, to: f32, t: f32, curve: EasingCurve) -> f32 {
    if t.is_nan() || t <= 0.0 {
        return from;
    }
    if t >= 1.0 {
        return to;
    }
    let mixed = from + (to - from) * ease(curve, t);
    // 丸めで端点をはみ出さないように収める。
    mixed.clamp(from.min(to), from.max(to))
}

/// 命令（意味操作）が起こす矩形の動きの種別（INV-M1。`docs/ui-spec.md` §5）。
///
/// **要レビュー（人間）**: `FocusMonitor`・`MoveColumnToMonitor`・`MoveToWorkspace`・`ToggleFloating` は
/// 仮の対応（U3 計画 §4.2 の表）。`CloseFocused`（隣の列が詰める動き）と、`Action` の未知の variant
/// （`#[non_exhaustive]`）は Scroll。
pub fn kind_of(action: &Action) -> MotionKind {
    match action {
        Action::SwitchPresetColumnWidth
        | Action::MaximizeColumn
        | Action::SetColumnWidth(_)
        | Action::SetWindowHeight(_)
        | Action::SwitchPresetWindowHeight
        | Action::ResetWindowHeight
        | Action::ToggleFloating
        | Action::ToggleFullscreen => MotionKind::Resize,
        Action::SwitchWorkspace(_)
        | Action::MoveToWorkspace(_)
        | Action::MoveColumnToWorkspace(_)
        | Action::MoveColumnToWorkspaceNoFocus(_)
        | Action::MoveWorkspaceUp
        | Action::MoveWorkspaceDown
        | Action::FocusMonitor(_)
        | Action::MoveColumnToMonitor(_) => MotionKind::Workspace,
        // 平行移動の命令と、未知の variant（`#[non_exhaustive]`）。
        _ => MotionKind::Scroll,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use contrast_core::{Action, ColumnWidth, Direction, WindowHeight};

    const ALL: [EasingCurve; 4] = [
        EasingCurve::EaseOutCubic,
        EasingCurve::EaseOutQuad,
        EasingCurve::EaseOutExpo,
        EasingCurve::EaseInOutCubic,
    ];

    /// 別に書いた閉じた式（実装と独立な参照）。
    fn reference(curve: EasingCurve, t: f32) -> f32 {
        match curve {
            EasingCurve::EaseOutCubic => 1.0 - (1.0 - t).powi(3),
            EasingCurve::EaseOutQuad => 1.0 - (1.0 - t).powi(2),
            EasingCurve::EaseOutExpo => 1.0 - 2f32.powf(-10.0 * t),
            EasingCurve::EaseInOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                }
            }
        }
    }

    #[test]
    fn endpoints_are_exact() {
        for c in ALL {
            assert_eq!(ease(c, 0.0), 0.0, "{c:?}");
            assert_eq!(ease(c, 1.0), 1.0, "{c:?}");
            assert_eq!(ease(c, -3.0), 0.0, "{c:?}");
            assert_eq!(ease(c, 7.0), 1.0, "{c:?}");
            assert_eq!(ease(c, f32::NAN), 0.0, "{c:?}");
        }
    }

    #[test]
    fn ease_is_monotonic_and_matches_the_closed_form() {
        for c in ALL {
            let mut prev = 0.0;
            for i in 0..=1000u16 {
                let t = f32::from(i) / 1000.0;
                let v = ease(c, t);
                assert!(v >= prev, "{c:?} t={t}: {v} < {prev}");
                assert!((0.0..=1.0).contains(&v), "{c:?} t={t}: {v}");
                if i != 0 && i != 1000 {
                    assert!((v - reference(c, t)).abs() < 2e-3, "{c:?} t={t}");
                }
                prev = v;
            }
        }
    }

    #[test]
    fn ease_is_deterministic() {
        for c in ALL {
            assert_eq!(ease(c, 0.37).to_bits(), ease(c, 0.37).to_bits());
        }
    }

    #[test]
    fn progress_runs_from_zero_to_one() {
        let d = Duration::from_millis(150);
        assert_eq!(progress(Duration::ZERO, d), 0.0);
        assert_eq!(progress(d, d), 1.0);
        assert_eq!(progress(Duration::from_secs(9), d), 1.0);
        assert_eq!(progress(Duration::from_millis(75), d), 0.5);
        let mut prev = 0.0;
        for ms in 0..=200u64 {
            let p = progress(Duration::from_millis(ms), d);
            assert!(p >= prev && (0.0..=1.0).contains(&p));
            prev = p;
        }
        // 長さ 0 は即座に終わる。
        assert_eq!(progress(Duration::ZERO, Duration::ZERO), 1.0);
    }

    #[test]
    fn dim_at_has_exact_ends_and_stays_between() {
        for c in ALL {
            for (from, to) in [(1.0f32, 0.8), (0.8, 1.0), (0.5, 0.5)] {
                assert_eq!(dim_at(from, to, 0.0, c), from);
                assert_eq!(dim_at(from, to, 1.0, c), to);
                assert_eq!(dim_at(from, to, -1.0, c), from);
                assert_eq!(dim_at(from, to, 2.0, c), to);
                let (lo, hi) = (from.min(to), from.max(to));
                let mut prev = from;
                for i in 0..=100u16 {
                    let k = dim_at(from, to, f32::from(i) / 100.0, c);
                    assert!((lo..=hi).contains(&k), "{c:?} {from}->{to} {k}");
                    // `from` から `to` へ単調。
                    assert!(
                        if to >= from { k >= prev } else { k <= prev },
                        "{c:?} {from}->{to}"
                    );
                    prev = k;
                }
            }
        }
    }

    #[test]
    fn kind_of_follows_the_table() {
        use MotionKind::{Resize, Scroll, Workspace};
        let ws = contrast_core::WorkspaceIndex::new(1);
        let half = contrast_core::Ratio::new(1, 2).unwrap();
        let rows: Vec<(Action, MotionKind)> = vec![
            (Action::FocusDirection(Direction::Left), Scroll),
            (Action::MoveWindow(Direction::Right), Scroll),
            (Action::FocusColumnFirst, Scroll),
            (Action::FocusColumnLast, Scroll),
            (Action::CenterColumn, Scroll),
            (Action::MoveColumnToFirst, Scroll),
            (Action::MoveColumnToLast, Scroll),
            (Action::ConsumeWindowIntoColumn, Scroll),
            (Action::ExpelWindowFromColumn, Scroll),
            (Action::ConsumeOrExpelWindowLeft, Scroll),
            (Action::ConsumeOrExpelWindowRight, Scroll),
            (Action::CloseFocused, Scroll),
            (Action::SwitchPresetColumnWidth, Resize),
            (Action::MaximizeColumn, Resize),
            (
                Action::SetColumnWidth(ColumnWidth::Proportion(half)),
                Resize,
            ),
            (Action::SetWindowHeight(WindowHeight::Auto), Resize),
            (Action::SwitchPresetWindowHeight, Resize),
            (Action::ResetWindowHeight, Resize),
            (Action::ToggleFloating, Resize),
            (Action::ToggleFullscreen, Resize),
            (Action::SwitchWorkspace(ws), Workspace),
            (Action::MoveToWorkspace(ws), Workspace),
            (Action::MoveColumnToWorkspace(ws), Workspace),
            (Action::MoveColumnToWorkspaceNoFocus(ws), Workspace),
            (Action::MoveWorkspaceUp, Workspace),
            (Action::MoveWorkspaceDown, Workspace),
            (Action::FocusMonitor(Direction::Left), Workspace),
            (Action::MoveColumnToMonitor(Direction::Up), Workspace),
        ];
        for (action, want) in rows {
            assert_eq!(kind_of(&action), want, "{action:?}");
        }
    }

    #[test]
    fn spec_lookup_and_defaults() {
        let mut m = MotionSettings::default();
        assert_eq!(m.spec(MotionKind::Scroll), None);
        m.set(
            MotionKind::Focus,
            MotionSpec::default_for(MotionKind::Focus),
        );
        let f = m.spec(MotionKind::Focus).unwrap();
        assert_eq!(f.duration_ms, 120);
        assert_eq!(f.curve, MotionCurve::Easing(EasingCurve::EaseOutCubic));
        assert_eq!(MotionSpec::default_for(MotionKind::Open).duration_ms, 150);
        assert_eq!(f.duration(), Some(Duration::from_millis(120)));
        let spring = MotionSpec {
            curve: MotionCurve::NiriSpring,
            duration_ms: 0,
        };
        assert_eq!(spring.duration(), None);
        assert!(!spring.is_instant());
        assert!(
            MotionSpec {
                curve: MotionCurve::Easing(EasingCurve::EaseOutCubic),
                duration_ms: 0
            }
            .is_instant()
        );
        assert_eq!(MAX_MOTION_MS, 400);
    }
}
