//! `contrast_policy::validate_tiers`（INV-A1。DU-9・DU-12）の性質テスト。
//!
//! `contrast-policy` には proptest が無い（依存を足せない）ので、proptest が使えるこのクレートに置く。
//! オラクルは `validate_tiers` の実装とは別に書いた述語: `0 < l <= 1`、狭義の単調減少、
//! `F - max(N) > max(N) - min(N)`、`Tier` はちょうど 1 つ、`name` は空でなく重ならない。
//! 生成器は有限・NaN・無限・負・0・1 超・重複・非単調と、0.05 刻みの値（境界を含む）と一様乱数を混ぜる。

use contrast_policy::{TierSpec, codes, validate_tiers};
use proptest::prelude::*;

/// `l` の候補。
fn l_value() -> impl Strategy<Value = f32> {
    prop_oneof![
        3 => (0u32..=21).prop_map(|k| k as f32 * 0.05),
        2 => -0.5f32..1.5f32,
        1 => Just(f32::NAN),
        1 => Just(f32::INFINITY),
        1 => Just(f32::NEG_INFINITY),
        1 => Just(0.0f32),
        1 => Just(1.0f32),
        1 => Just(-0.0f32),
    ]
}

/// 名前の候補（省略・空・重複しやすい短い名前）。
fn name() -> impl Strategy<Value = Option<String>> {
    prop_oneof![
        4 => Just(None),
        1 => Just(Some(String::new())),
        2 => prop_oneof![Just("a"), Just("b")].prop_map(|s| Some(s.to_owned())),
    ]
}

/// 段階 1 つ。
fn spec() -> impl Strategy<Value = TierSpec> {
    (l_value(), name()).prop_map(|(l, name)| TierSpec {
        l,
        ring_color: None,
        name,
    })
}

/// オラクル: 期待する診断コードの集合（ソート済み・重複なし）。
fn oracle(focus: &TierSpec, rest: &[TierSpec]) -> Vec<&'static str> {
    let ok = |l: f32| l.is_finite() && 0.0 < l && l <= 1.0;
    let mut out = Vec::new();
    let all_valid = ok(focus.l) && rest.iter().all(|t| ok(t.l));
    if !all_valid {
        out.push(codes::TIER_BAD_L);
    }
    if rest.len() != 1 {
        out.push(codes::TIER_COUNT);
    }
    let mut seen: Vec<&str> = Vec::new();
    let mut bad_name = false;
    for t in rest {
        if let Some(n) = &t.name {
            if n.is_empty() || seen.contains(&n.as_str()) {
                bad_name = true;
            }
            seen.push(n);
        }
    }
    if bad_name {
        out.push(codes::TIER_BAD_NAME);
    }
    if all_valid {
        let mut chain = vec![f64::from(focus.l)];
        chain.extend(rest.iter().map(|t| f64::from(t.l)));
        let monotonic = chain.windows(2).all(|w| matches!(w, [a, b] if a > b));
        if !monotonic {
            out.push(codes::TIER_NOT_MONOTONIC);
        } else if let (Some(max), Some(min)) = (
            chain.iter().skip(1).copied().reduce(f64::max),
            chain.iter().skip(1).copied().reduce(f64::min),
        ) && f64::from(focus.l) - max <= max - min
        {
            out.push(codes::TIER_SPREAD);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

proptest! {
    #[test]
    fn validate_tiers_agrees_with_the_oracle(
        focus in spec(),
        rest in proptest::collection::vec(spec(), 0..=3),
    ) {
        let expected = oracle(&focus, &rest);
        let actual = match validate_tiers(&focus, &rest) {
            Ok(_) => Vec::new(),
            Err(errors) => {
                let mut codes: Vec<&'static str> = errors.iter().map(|e| e.code()).collect();
                codes.sort_unstable();
                codes.dedup();
                codes
            }
        };
        prop_assert_eq!(&actual, &expected);
        if expected.is_empty() {
            let tiers = validate_tiers(&focus, &rest).map_err(|e| TestCaseError::fail(format!("{e:?}")))?;
            prop_assert_eq!(tiers.focus.l, focus.l);
            prop_assert_eq!(tiers.rest.len(), rest.len());
        }
    }
}
