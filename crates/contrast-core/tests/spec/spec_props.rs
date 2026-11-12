//! Kani ハーネスの代わりの性質テスト（proptest）: 幾何と ID。
//! 以前 src/verification/spec/ に置いていた Kani ハーネスと同じ主張を、ランダム入力で確かめる。
use std::collections::BTreeMap;

use contrast_core::{
    CoreError, ExternalKey, IdGen, IdKind, KeyMap, LayoutError, Point, Rect, WindowId,
};
use proptest::prelude::*;

/// 重なりや分割が起きやすい、小さな矩形。
fn small_rect() -> impl Strategy<Value = Rect> {
    (-1000i32..1000, -1000i32..1000, 1i32..500, 1i32..500)
        .prop_filter_map("有効な矩形", |(x, y, w, h)| Rect::new(x, y, w, h))
}

/// i32 の限界に近い矩形（右端・下端がぎりぎり収まるもの）。
fn extreme_rect() -> impl Strategy<Value = Rect> {
    (any::<i32>(), any::<i32>(), 1i32..=i32::MAX, 1i32..=i32::MAX)
        .prop_filter_map("有効な矩形", |(x, y, w, h)| Rect::new(x, y, w, h))
}

fn any_rect() -> impl Strategy<Value = Rect> {
    prop_oneof![3 => small_rect(), 1 => extreme_rect()]
}

fn start_value() -> impl Strategy<Value = u64> {
    prop_oneof![
        4 => any::<u64>(),
        1 => Just(0u64),
        1 => Just(u64::MAX - 1),
        1 => Just(u64::MAX),
    ]
}

proptest! {
    /// Rect::new は任意の i32 でパニックせず、失敗は「幅・高さが 1 未満」か「右端・下端が i32 に収まらない」ときだけ。
    #[test]
    fn rect_new_never_panics_and_edges_fit(x in any::<i32>(), y in any::<i32>(), w in any::<i32>(), h in any::<i32>()) {
        match Rect::new(x, y, w, h) {
            Some(r) => {
                prop_assert!(w >= 1 && h >= 1);
                prop_assert_eq!(i64::from(r.right()), i64::from(x) + i64::from(w));
                prop_assert_eq!(i64::from(r.bottom()), i64::from(y) + i64::from(h));
                prop_assert_eq!((r.x(), r.y(), r.width(), r.height()), (x, y, w, h));
            }
            None => {
                let fits = i32::try_from(i64::from(x) + i64::from(w)).is_ok()
                    && i32::try_from(i64::from(y) + i64::from(h)).is_ok();
                prop_assert!(w < 1 || h < 1 || !fits);
            }
        }
    }

    /// intersect は対称で、結果は両方の矩形に収まる。限界付近でもパニックしない。
    #[test]
    fn intersect_is_symmetric_and_contained(a in any_rect(), b in any_rect()) {
        let ab = a.intersect(&b);
        prop_assert_eq!(ab, b.intersect(&a));
        if let Some(i) = ab {
            prop_assert!(i.x() >= a.x() && i.x() >= b.x());
            prop_assert!(i.y() >= a.y() && i.y() >= b.y());
            prop_assert!(i.right() <= a.right() && i.right() <= b.right());
            prop_assert!(i.bottom() <= a.bottom() && i.bottom() <= b.bottom());
        }
    }

    /// intersect は、両方に含まれる点だけを覆う。
    #[test]
    fn intersect_covers_exactly_the_common_points(
        a in small_rect(), b in small_rect(), px in -1100i32..1100, py in -1100i32..1100
    ) {
        let p = Point::new(px, py);
        let both = a.contains(p) && b.contains(p);
        match a.intersect(&b) {
            Some(i) => prop_assert_eq!(i.contains(p), both),
            None => prop_assert!(!both),
        }
    }

    /// split_columns は、元の矩形を隙間なく n 列に分け、余りは先頭から 1 px ずつ配る。
    #[test]
    fn split_columns_partitions_the_rect(r in any_rect(), n in 1usize..=40) {
        let count = i32::try_from(n).expect("小さい");
        match r.split_columns(n) {
            Err(e) => {
                prop_assert_eq!(e, LayoutError::AreaTooSmall);
                prop_assert!(r.width() < count);
            }
            Ok(parts) => {
                prop_assert_eq!(parts.len(), n);
                let (base, extra) = (r.width() / count, r.width() % count);
                let mut cursor = r.x();
                for (idx, p) in parts.iter().enumerate() {
                    let bonus = i32::from(i32::try_from(idx).expect("小さい") < extra);
                    prop_assert_eq!((p.x(), p.width()), (cursor, base + bonus));
                    prop_assert_eq!((p.y(), p.height()), (r.y(), r.height()));
                    cursor = p.right();
                }
                prop_assert_eq!(cursor, r.right());
            }
        }
    }

    /// split_rows は、split_columns の縦版。
    #[test]
    fn split_rows_partitions_the_rect(r in any_rect(), n in 1usize..=40) {
        let count = i32::try_from(n).expect("小さい");
        match r.split_rows(n) {
            Err(e) => {
                prop_assert_eq!(e, LayoutError::AreaTooSmall);
                prop_assert!(r.height() < count);
            }
            Ok(parts) => {
                prop_assert_eq!(parts.len(), n);
                let (base, extra) = (r.height() / count, r.height() % count);
                let mut cursor = r.y();
                for (idx, p) in parts.iter().enumerate() {
                    let bonus = i32::from(i32::try_from(idx).expect("小さい") < extra);
                    prop_assert_eq!((p.y(), p.height()), (cursor, base + bonus));
                    prop_assert_eq!((p.x(), p.width()), (r.x(), r.width()));
                    cursor = p.bottom();
                }
                prop_assert_eq!(cursor, r.bottom());
            }
        }
    }

    /// IdGen::next は任意の開始値でパニックせず、失敗は開始値が u64::MAX のときだけ。
    #[test]
    fn idgen_next_never_panics_and_exhausts_only_at_the_limit(start in start_value()) {
        let mut g = IdGen::<WindowId>::starting_at(start);
        match g.next() {
            Ok(id) => {
                prop_assert_eq!(id.raw(), start);
                prop_assert!(start != u64::MAX);
            }
            Err(e) => {
                prop_assert_eq!(e, CoreError::IdExhausted);
                prop_assert_eq!(start, u64::MAX);
            }
        }
    }

    /// 続けて払い出した ID は、ちょうど 1 ずつ増える。
    #[test]
    fn idgen_is_strictly_monotone(start in start_value()) {
        let mut g = IdGen::<WindowId>::starting_at(start);
        if let (Ok(a), Ok(b)) = (g.next(), g.next()) {
            prop_assert_eq!(b.raw(), a.raw() + 1);
        }
    }
}

#[derive(Clone, Debug)]
enum MapOp {
    Insert(u64, usize),
    Remove(u64),
}

fn map_op() -> impl Strategy<Value = MapOp> {
    prop_oneof![
        (0u64..4, 0usize..4).prop_map(|(k, i)| MapOp::Insert(k, i)),
        (0u64..4).prop_map(MapOp::Remove),
    ]
}

proptest! {
    /// KeyMap は、キーと ID の 1 対 1 の単純なモデルと、結果も状態も一致する（全単射を含む）。
    #[test]
    fn keymap_agrees_with_a_simple_model(ops in prop::collection::vec(map_op(), 0..40)) {
        let mut g = IdGen::<WindowId>::new();
        let pool: Vec<WindowId> = (0..4).map(|_| g.next().expect("枯渇しない")).collect();
        let mut map = KeyMap::<WindowId>::new();
        let mut model: BTreeMap<u64, usize> = BTreeMap::new(); // キー → pool の添字
        for op in ops {
            match op {
                MapOp::Insert(k, i) => {
                    let dup = model.contains_key(&k) || model.values().any(|v| *v == i);
                    let result = map.insert(ExternalKey::new(k), pool[i]);
                    if dup {
                        prop_assert_eq!(result, Err(CoreError::DuplicateKey));
                    } else {
                        prop_assert_eq!(result, Ok(()));
                        model.insert(k, i);
                    }
                }
                MapOp::Remove(k) => {
                    let result = map.remove_by_key(ExternalKey::new(k));
                    match model.remove(&k) {
                        Some(i) => prop_assert_eq!(result, Ok(pool[i])),
                        None => prop_assert_eq!(result, Err(CoreError::UnknownKey)),
                    }
                }
            }
            prop_assert_eq!(map.len(), model.len());
            for k in 0..4u64 {
                prop_assert_eq!(
                    map.id_of(ExternalKey::new(k)),
                    model.get(&k).map(|i| pool[*i])
                );
            }
            for (i, id) in pool.iter().enumerate() {
                let owner = model
                    .iter()
                    .find(|(_, v)| **v == i)
                    .map(|(k, _)| ExternalKey::new(*k));
                prop_assert_eq!(map.key_of(*id), owner);
            }
        }
    }
}
