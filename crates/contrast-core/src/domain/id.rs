//! 識別子と対応表: 各 ID 型、`IdGen`、`ExternalKey`、`KeyMap`。
//!
//! 守る不変条件:
//! - I6（前半）ID は単調増加で再利用しない。`IdGen` は払い出すたびに raw を 1 進め、
//!   戻さない。`u64::MAX` に達したら以後は常に `Err(IdExhausted)` を返す。
//! - I6（後半）`ExternalKey` と ID の対応は全単射。`KeyMap` は 2 方向の表を常に同時に更新し、
//!   `Err` を返した操作は表を一切変えない。
//!
//! 算術は `checked_add` だけを使い、パニックしない。

use alloc::collections::BTreeMap;
use core::marker::PhantomData;

use crate::error::CoreError;

/// u64 の newtype である ID 型に共通の操作。
pub trait IdKind: Copy + Ord {
    /// raw 値から ID を作る。
    fn from_raw(raw: u64) -> Self;
    /// ID の raw 値を返す。
    fn raw(self) -> u64;
}

/// 出力（モニタ）の ID。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct OutputId(u64);

/// トップレベル窓の ID。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct WindowId(u64);

/// ワークスペースの ID。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorkspaceId(u64);

impl IdKind for OutputId {
    fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    fn raw(self) -> u64 {
        self.0
    }
}

impl IdKind for WindowId {
    fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    fn raw(self) -> u64 {
        self.0
    }
}

impl IdKind for WorkspaceId {
    fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    fn raw(self) -> u64 {
        self.0
    }
}

/// ワークスペースの 0 始まりのインデックス。範囲の検査は使う側（`workspace_count`）が行う。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorkspaceIndex(u8);

impl WorkspaceIndex {
    /// インデックス `n` を作る。
    #[must_use]
    pub const fn new(n: u8) -> Self {
        Self(n)
    }

    /// インデックスの値を返す。
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// シェル側の識別子。core にとっては不透明な u64 で、ID と対応づけるだけ。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExternalKey(u64);

impl ExternalKey {
    /// raw 値 `raw` のキーを作る。
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }
}

/// 登録結果として返すエンティティの ID。
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityId {
    /// 出力。
    Output(OutputId),
    /// 窓。
    Window(WindowId),
}

/// ID の払い出し器。raw = 1 から 1 ずつ増やして払い出し、同じ値を二度返さない（I6）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdGen<T: IdKind> {
    /// 次に払い出す raw 値。`u64::MAX` なら枯渇している。
    next_raw: u64,
    /// 払い出す ID の型。
    kind: PhantomData<T>,
}

impl<T: IdKind> IdGen<T> {
    /// 最初の ID が raw = 1 になる払い出し器を作る。
    #[must_use]
    pub const fn new() -> Self {
        Self::starting_at(1)
    }

    /// 次に払い出す raw 値を `next_raw` にした払い出し器を作る（テスト用）。
    #[must_use]
    pub const fn starting_at(next_raw: u64) -> Self {
        Self {
            next_raw,
            kind: PhantomData,
        }
    }
    /// 現在値の ID を返して 1 進める。
    ///
    /// # Errors
    /// 現在値が `u64::MAX` なら `CoreError::IdExhausted`。このとき状態は変わらないので、以後も同じ。
    #[allow(
        clippy::should_implement_trait,
        reason = "Iterator ではない。枯渇は終端ではなくエラー（IdExhausted）として返す"
    )]
    pub fn next(&mut self) -> Result<T, CoreError> {
        let advanced = self.next_raw.checked_add(1).ok_or(CoreError::IdExhausted)?;
        let id = T::from_raw(self.next_raw);
        self.next_raw = advanced;
        Ok(id)
    }
}

impl<T: IdKind> Default for IdGen<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// `ExternalKey` と ID の双方向の対応表。2 つの表は常に互いの逆写像になる（I6）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyMap<T: IdKind> {
    /// key → id。
    by_key: BTreeMap<ExternalKey, T>,
    /// id → key。
    by_id: BTreeMap<T, ExternalKey>,
}

impl<T: IdKind> KeyMap<T> {
    /// 空の対応表を作る。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            by_key: BTreeMap::new(),
            by_id: BTreeMap::new(),
        }
    }

    /// 登録されている対応の数を返す。
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    /// 対応が 1 つもなければ true。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    /// `key` と `id` の対応を登録する。
    ///
    /// # Errors
    /// `key` か `id` のどちらかが登録済みなら `CoreError::DuplicateKey`。
    /// 両方を確認してから書き込むので、`Err` のとき表は変わらない。
    pub fn insert(&mut self, key: ExternalKey, id: T) -> Result<(), CoreError> {
        if self.by_key.contains_key(&key) || self.by_id.contains_key(&id) {
            return Err(CoreError::DuplicateKey);
        }
        self.by_key.insert(key, id);
        self.by_id.insert(id, key);
        Ok(())
    }

    /// `key` の対応を両方向から消し、対応していた ID を返す。
    ///
    /// # Errors
    /// `key` が未登録なら `CoreError::UnknownKey`。このとき表は変わらない。
    pub fn remove_by_key(&mut self, key: ExternalKey) -> Result<T, CoreError> {
        let id = self.by_key.remove(&key).ok_or(CoreError::UnknownKey)?;
        self.by_id.remove(&id);
        Ok(id)
    }

    /// `key` に対応する ID を返す。
    #[must_use]
    pub fn id_of(&self, key: ExternalKey) -> Option<T> {
        self.by_key.get(&key).copied()
    }

    /// `id` に対応するキーを返す。
    #[must_use]
    pub fn key_of(&self, id: T) -> Option<ExternalKey> {
        self.by_id.get(&id).copied()
    }

    /// 試験用: 逆引きだけを書き換えて、2 つの表を食い違わせる（I6 の検出を確かめるため）。
    #[cfg(test)]
    pub(crate) fn corrupt_reverse(&mut self, id: T, key: ExternalKey) {
        self.by_id.insert(id, key);
    }

    /// 2 つの表の要素数が等しく、全 (k, v) で逆引きが (v, k) に戻るなら true（I6 の検査用）。
    #[cfg(any(test, feature = "invariants"))]
    #[must_use]
    pub fn is_bijective(&self) -> bool {
        self.by_key.len() == self.by_id.len()
            && self
                .by_key
                .iter()
                .all(|(k, v)| self.by_id.get(v) == Some(k))
    }
}

impl<T: IdKind> Default for KeyMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod keymap_tests {
    use super::*;

    /// `is_bijective` は、要素数が合い、全部の逆引きが戻るときだけ true。
    #[test]
    fn is_bijective_needs_equal_lengths_and_matching_reverse_entries() {
        let mut map: KeyMap<WindowId> = KeyMap::new();
        assert!(map.is_bijective());
        assert!(
            map.insert(ExternalKey::new(1), WindowId::from_raw(10))
                .is_ok()
        );
        assert!(
            map.insert(ExternalKey::new(2), WindowId::from_raw(20))
                .is_ok()
        );
        assert!(map.is_bijective());

        // 要素数が違う（逆引きだけ余分）。
        let mut longer = map.clone();
        longer
            .by_id
            .insert(WindowId::from_raw(30), ExternalKey::new(3));
        assert!(!longer.is_bijective());

        // 要素数は同じだが、逆引きが戻らない。
        let mut crossed = map.clone();
        crossed.corrupt_reverse(WindowId::from_raw(10), ExternalKey::new(2));
        assert_eq!(crossed.by_key.len(), crossed.by_id.len());
        assert!(!crossed.is_bijective());
    }
}
