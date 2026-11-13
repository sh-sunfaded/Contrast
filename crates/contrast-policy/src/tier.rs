//! 注意の段階（Tier）の値と検査。CDL を知らない純粋な部分。
//!
//! - `Tiers`: フォーカス段階 1 つと、フォーカス外の段階（この版では 1 つ）。
//! - `validate_tiers`: INV-A1（DU-9・DU-12）の検査。`0 < l <= 1`、`F > n_1 > …`（狭義の単調減少）、
//!   `F - max(N) > max(N) - min(N)`（狭義。差は OKLCH の L の差）。
//! - `dim_factor`: 窓の画素に掛ける乗数。**仮置き**: 中立のグレーでだけ正確な近似で、目標値ではない。
//!   `l` は「窓の画素を何倍に暗くするか」の指標で、周囲（壁紙・隣の窓）に対する知覚コントラストの保証ではない（DU-18・DU-19）。

use crate::codes;

/// フォーカス外の段階の個数の上限（DU-15: この版では 1 つ）。
pub const MAX_REST_TIERS: usize = 1;

/// 組み込みの、フォーカス外の段階の `l`（仮置き。DU-18）。
pub const BUILTIN_REST_L: f32 = 0.8;

/// 1 つの段階。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tier {
    /// OKLCH の明度 L（0 より大きく 1 以下）。窓の画素の明るさの目安。
    pub l: f32,
    /// 枠（ring）の色（RGBA）。`None` なら枠を描かない。
    pub ring_color: Option<[f32; 4]>,
}

/// 検査済みの段階の組。
#[derive(Debug, Clone, PartialEq)]
pub struct Tiers {
    /// フォーカス段階（最も明るい）。
    pub focus: Tier,
    /// フォーカス外の段階（明るい順。この版では 1 つ）。
    pub rest: Vec<Tier>,
}

impl Tiers {
    /// 組み込みの既定: フォーカス `l = 1.0`（枠なし）、フォーカス外 `l = 0.8`（枠なし）。
    pub fn builtin() -> Self {
        Self {
            focus: Tier {
                l: 1.0,
                ring_color: None,
            },
            rest: vec![Tier {
                l: BUILTIN_REST_L,
                ring_color: None,
            }],
        }
    }

    /// フォーカス外の窓の画素に掛ける乗数（0〜1。段階が無ければ 1）。
    pub fn unfocused_dim(&self) -> f32 {
        self.rest
            .first()
            .map_or(1.0, |t| dim_factor(self.focus.l, t.l))
    }

    /// フォーカス窓の枠の色。
    pub fn focus_ring(&self) -> Option<[f32; 4]> {
        self.focus.ring_color
    }

    /// フォーカス外の窓の枠の色。
    pub fn unfocused_ring(&self) -> Option<[f32; 4]> {
        self.rest.first().and_then(|t| t.ring_color)
    }
}

/// 検査の入力 1 つ（CDL の `FocusTier` / `Tier` 要素の中身）。
#[derive(Debug, Clone, PartialEq)]
pub struct TierSpec {
    /// 明度 L。
    pub l: f32,
    /// 枠の色。
    pub ring_color: Option<[f32; 4]>,
    /// 診断用の名前（`Tier` だけ）。
    pub name: Option<String>,
}

/// 誤りのある段階の位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TierSlot {
    /// `FocusTier`。
    Focus,
    /// `Tier`（書いた順の 0 始まり）。
    Rest(usize),
}

/// 段階の検査の誤り。診断コードは `code()`。
#[derive(Debug, Clone, PartialEq)]
pub enum TierError {
    /// `l` が有限でない、または 0 以下か 1 超（CT060）。
    BadL {
        /// 誤りのある段階。
        slot: TierSlot,
        /// 書かれた値。
        value: f32,
    },
    /// 段階が単調でない: 直前の段階以上の `l`（CT062）。
    NotMonotonic {
        /// 誤りのある段階。
        slot: TierSlot,
        /// 直前の段階の `l`。
        previous: f32,
        /// この段階の `l`。
        value: f32,
    },
    /// フォーカス段階との差が、フォーカス外の段階全体の幅以下（CT063。DU-12）。
    SpreadTooLarge {
        /// フォーカス段階の `l`。
        focus: f32,
        /// フォーカス外の `l` の最大。
        max_rest: f32,
        /// フォーカス外の `l` の最小。
        min_rest: f32,
    },
    /// `Tier` の個数が合わない: この版は 1 つだけ（CT064）。
    RestCount {
        /// 書かれた個数。
        count: usize,
    },
    /// `name` が空か、重複（CT065）。
    BadName {
        /// 誤りのある段階。
        slot: TierSlot,
    },
}

impl TierError {
    /// 診断コード。
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadL { .. } => codes::TIER_BAD_L,
            Self::NotMonotonic { .. } => codes::TIER_NOT_MONOTONIC,
            Self::SpreadTooLarge { .. } => codes::TIER_SPREAD,
            Self::RestCount { .. } => codes::TIER_COUNT,
            Self::BadName { .. } => codes::TIER_BAD_NAME,
        }
    }

    /// 誤りのある段階。段階全体の誤り（`SpreadTooLarge`・`RestCount`）は `None`。
    pub fn slot(&self) -> Option<TierSlot> {
        match self {
            Self::BadL { slot, .. } | Self::NotMonotonic { slot, .. } | Self::BadName { slot } => {
                Some(*slot)
            }
            Self::SpreadTooLarge { .. } | Self::RestCount { .. } => None,
        }
    }

    /// 診断のメッセージ。
    pub fn message(&self) -> String {
        match self {
            Self::BadL { value, .. } => {
                format!("`l` は 0 より大きく 1 以下の数（{value} は使えない）")
            }
            Self::NotMonotonic {
                previous, value, ..
            } => format!(
                "段階の `l` は、書いた順に狭義の単調減少（直前が {previous}、この段階が {value}）"
            ),
            Self::SpreadTooLarge {
                focus,
                max_rest,
                min_rest,
            } => format!(
                "フォーカス段階との差（F={focus}、フォーカス外の最大={max_rest}）が、フォーカス外の段階全体の幅（最小={min_rest}）より大きくない"
            ),
            Self::RestCount { count } => {
                format!("`Tier` はちょうど {MAX_REST_TIERS} つ（{count} 個は使えない）")
            }
            Self::BadName { .. } => "`name` は空でなく、他の段階と重ならない".to_owned(),
        }
    }
}

/// `0 < l <= 1`（有限）か。
fn l_in_range(l: f32) -> bool {
    l.is_finite() && l > 0.0 && l <= 1.0
}

/// 段階を検査して `Tiers` にする。誤りは見つけた順に全部返す。
///
/// 差の計算は f64 に広げて行う（f32 同士の差は f64 で厳密）。
pub fn validate_tiers(focus: &TierSpec, rest: &[TierSpec]) -> Result<Tiers, Vec<TierError>> {
    let mut errors = Vec::new();

    let slots = std::iter::once((TierSlot::Focus, focus))
        .chain(rest.iter().enumerate().map(|(i, t)| (TierSlot::Rest(i), t)));
    let mut all_valid = true;
    for (slot, t) in slots {
        if !l_in_range(t.l) {
            all_valid = false;
            errors.push(TierError::BadL { slot, value: t.l });
        }
    }

    if rest.len() != MAX_REST_TIERS {
        errors.push(TierError::RestCount { count: rest.len() });
    }

    let mut seen: Vec<&str> = Vec::new();
    for (i, t) in rest.iter().enumerate() {
        if let Some(name) = t.name.as_deref() {
            if name.is_empty() || seen.contains(&name) {
                errors.push(TierError::BadName {
                    slot: TierSlot::Rest(i),
                });
            }
            seen.push(name);
        }
    }

    if all_valid {
        let mut previous = focus.l;
        let mut monotonic = true;
        for (i, t) in rest.iter().enumerate() {
            if t.l >= previous {
                monotonic = false;
                errors.push(TierError::NotMonotonic {
                    slot: TierSlot::Rest(i),
                    previous,
                    value: t.l,
                });
            }
            previous = t.l;
        }
        if monotonic {
            let max_rest = rest.iter().map(|t| t.l).fold(f32::MIN, f32::max);
            let min_rest = rest.iter().map(|t| t.l).fold(f32::MAX, f32::min);
            let gap = f64::from(focus.l) - f64::from(max_rest);
            let spread = f64::from(max_rest) - f64::from(min_rest);
            if !rest.is_empty() && gap <= spread {
                errors.push(TierError::SpreadTooLarge {
                    focus: focus.l,
                    max_rest,
                    min_rest,
                });
            }
        }
    }

    if errors.is_empty() {
        Ok(Tiers {
            focus: Tier {
                l: focus.l,
                ring_color: focus.ring_color,
            },
            rest: rest
                .iter()
                .map(|t| Tier {
                    l: t.l,
                    ring_color: t.ring_color,
                })
                .collect(),
        })
    } else {
        Err(errors)
    }
}

/// 窓の画素（エンコード済み sRGB の値）に掛ける乗数。`(l_tier / l_focus)^(3/2.2)`、0〜1 に収める。
///
/// 仮置き（DU-18）: 中立のグレーでは OKLab の L が線形の明るさの立方根に近いこと、
/// エンコード済みの値のガンマが約 2.2 であることを使った近似。色つきの窓では正確でない。
/// `l_focus` か `l_tier` が有限の正の数でないときは 1（減光なし）。
pub fn dim_factor(l_focus: f32, l_tier: f32) -> f32 {
    if !(l_in_range(l_focus) && l_in_range(l_tier)) {
        return 1.0;
    }
    let k = (l_tier / l_focus).powf(3.0 / 2.2);
    if k.is_finite() {
        k.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(l: f32) -> TierSpec {
        TierSpec {
            l,
            ring_color: None,
            name: None,
        }
    }

    fn codes_of(r: Result<Tiers, Vec<TierError>>) -> Vec<&'static str> {
        match r {
            Ok(_) => Vec::new(),
            Err(e) => e.iter().map(TierError::code).collect(),
        }
    }

    #[test]
    fn builtin_is_valid_and_dims() {
        let b = Tiers::builtin();
        let v = validate_tiers(
            &spec(b.focus.l),
            &b.rest.iter().map(|t| spec(t.l)).collect::<Vec<_>>(),
        );
        assert_eq!(v, Ok(b.clone()));
        let k = b.unfocused_dim();
        assert!(k > 0.7 && k < 0.8, "k = {k}");
    }

    #[test]
    fn accepts_the_basic_pair() {
        assert!(validate_tiers(&spec(1.0), &[spec(0.8)]).is_ok());
    }

    #[test]
    fn rejects_by_code() {
        assert_eq!(
            codes_of(validate_tiers(&spec(0.8), &[spec(0.9)])),
            [codes::TIER_NOT_MONOTONIC]
        );
        assert_eq!(
            codes_of(validate_tiers(&spec(1.0), &[spec(1.0)])),
            [codes::TIER_NOT_MONOTONIC]
        );
        assert_eq!(
            codes_of(validate_tiers(&spec(1.0), &[spec(0.0)])),
            [codes::TIER_BAD_L]
        );
        assert_eq!(
            codes_of(validate_tiers(&spec(1.2), &[spec(0.8)])),
            [codes::TIER_BAD_L]
        );
        assert_eq!(
            codes_of(validate_tiers(&spec(f32::NAN), &[spec(0.8)])),
            [codes::TIER_BAD_L]
        );
        assert_eq!(
            codes_of(validate_tiers(&spec(1.0), &[spec(0.9), spec(0.5)])),
            [codes::TIER_COUNT, codes::TIER_SPREAD]
        );
        assert_eq!(
            codes_of(validate_tiers(&spec(1.0), &[])),
            [codes::TIER_COUNT]
        );
    }

    #[test]
    fn spread_boundary_is_strict() {
        // 差 0.25 = 幅 0.25（2 進で厳密）: 拒否。
        assert_eq!(
            codes_of(validate_tiers(&spec(1.0), &[spec(0.75), spec(0.5)])),
            [codes::TIER_COUNT, codes::TIER_SPREAD]
        );
        // 差 0.25 > 幅 0.125: 通る（個数の誤りだけ）。
        assert_eq!(
            codes_of(validate_tiers(&spec(1.0), &[spec(0.75), spec(0.625)])),
            [codes::TIER_COUNT]
        );
    }

    #[test]
    fn names_must_be_unique_and_nonempty() {
        let named = |n: &str| TierSpec {
            name: Some(n.to_owned()),
            ..spec(0.8)
        };
        assert_eq!(
            codes_of(validate_tiers(&spec(1.0), &[named("")])),
            [codes::TIER_BAD_NAME]
        );
        assert!(validate_tiers(&spec(1.0), &[named("dim")]).is_ok());
    }

    #[test]
    fn dim_factor_properties() {
        assert!((dim_factor(1.0, 1.0) - 1.0).abs() < 1e-6);
        assert!(dim_factor(1.0, 0.5) < dim_factor(1.0, 0.8));
        assert!((dim_factor(1.0, 0.8) - 0.8_f32.powf(3.0 / 2.2)).abs() < 1e-6);
        // 不正な入力は減光なし。
        assert!((dim_factor(0.0, 0.5) - 1.0).abs() < 1e-6);
        assert!((dim_factor(1.0, f32::NAN) - 1.0).abs() < 1e-6);
        // 明るくする向きは扱わない（1 に収める）。
        assert!((dim_factor(0.5, 1.0) - 1.0).abs() < 1e-6);
    }
}
