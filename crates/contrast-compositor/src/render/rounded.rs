//! 窓の面の見た目: 内側の要素を、テクスチャシェーダ（角丸のマスクと減光の乗数）で描く要素 `StyledSurface`。
//!
//! 描画以外（位置・ダメージ・ID など）は中身にそのまま任せる。違いは次の 3 点。
//! - 角丸（`radius > 0`）のときは、角が透けるので不透明領域を空にする。`radius == 0` なら中身の不透明領域のまま
//!   （減光はアルファを変えないので、不透明な画素は不透明のまま）。
//! - 直接スキャンアウトはさせない（`underlying_storage` は `None`）。
//! - 減光（INV-R1: 矩形は変えない）: プリマルチプライドの RGB に `dim` を掛け、A は触らない。
//!   乗数が変わったときのダメージ追跡のために、乗数が変わった窓の要素には新しい `Id` を使う（`DimIds`）。
//!   中身の `current_commit` のままでは、Smithay 0.7.0 の `damage/mod.rs:563` が、乗数だけの変化を再描画しない。

use smithay::{
    backend::renderer::{
        element::{Element, Id, Kind, RenderElement, UnderlyingStorage},
        gles::{GlesError, GlesFrame, GlesRenderer, GlesTexProgram, Uniform},
        utils::{CommitCounter, DamageSet, OpaqueRegions},
    },
    utils::{Buffer, Physical, Point, Rectangle, Scale, Transform},
};

/// 角丸と減光のシェーダで描く窓の面。`E` は窓の面の要素（シェーダのテストではテクスチャの要素）。
pub struct StyledSurface<E> {
    inner: E,
    program: GlesTexProgram,
    radius: f32,
    dim: f32,
    id: Id,
}

impl<E: Element> StyledSurface<E> {
    /// `inner` を半径 `radius`（px。0 以下で角丸なし）の角丸で、乗数 `dim`（1 で減光なし）を掛けて描く要素にする。
    /// `Id` は中身と同じ。
    pub fn new(inner: E, program: GlesTexProgram, radius: f32, dim: f32) -> Self {
        let id = inner.id().clone();
        Self {
            inner,
            program,
            radius,
            dim,
            id,
        }
    }

    /// 要素の `Id` を替える（乗数が変わった窓を、ダメージ追跡に新しい要素として見せる）。
    #[must_use]
    pub fn with_id(mut self, id: Id) -> Self {
        self.id = id;
        self
    }

    /// 掛ける乗数。
    pub fn dim(&self) -> f32 {
        self.dim
    }
}

impl<E: Element> Element for StyledSurface<E> {
    fn id(&self) -> &Id {
        &self.id
    }

    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }

    fn location(&self, scale: Scale<f64>) -> Point<i32, Physical> {
        self.inner.location(scale)
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        self.inner.src()
    }

    fn transform(&self) -> Transform {
        self.inner.transform()
    }

    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.inner.geometry(scale)
    }

    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.inner.damage_since(scale, commit)
    }

    fn opaque_regions(&self, scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        if self.radius > 0.0 {
            OpaqueRegions::default()
        } else {
            self.inner.opaque_regions(scale)
        }
    }

    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }

    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl<E: RenderElement<GlesRenderer>> RenderElement<GlesRenderer> for StyledSurface<E> {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError> {
        frame.override_default_tex_program(
            self.program.clone(),
            vec![
                Uniform::new("elem_size", (dst.size.w as f32, dst.size.h as f32)),
                Uniform::new("radius", self.radius),
                Uniform::new("dim", self.dim),
            ],
        );
        let result = RenderElement::<GlesRenderer>::draw(&self.inner, frame, src, dst, damage, &[]);
        frame.clear_tex_program_override();
        result
    }

    fn underlying_storage(&self, _renderer: &mut GlesRenderer) -> Option<UnderlyingStorage<'_>> {
        None
    }
}

/// 乗数が変わった窓の要素に新しい `Id` を割り当てる表。中身の `Id`（窓の面ごと）→（最後の乗数、減光する要素の `Id`）。
///
/// 乗数が変わらないあいだは同じ `Id` を返す（差分追跡が効く）。変わったら新しい `Id`。
/// フレームの最初に `begin_frame`、最後に `end_frame` を呼ぶと、そのフレームで使わなかった項目を捨てる。
#[derive(Default)]
pub struct DimIds {
    entries: Vec<DimEntry>,
}

/// `DimIds` の 1 項目。
struct DimEntry {
    inner: Id,
    dim: f32,
    styled: Id,
    seen: bool,
}

impl DimIds {
    /// フレームの最初: 使われた印を消す。
    pub fn begin_frame(&mut self) {
        for e in &mut self.entries {
            e.seen = false;
        }
    }

    /// 中身の `Id` が `inner`、乗数が `dim` の要素に使う `Id`。
    pub fn styled_id(&mut self, inner: &Id, dim: f32) -> Id {
        if let Some(e) = self.entries.iter_mut().find(|e| &e.inner == inner) {
            e.seen = true;
            if e.dim.to_bits() != dim.to_bits() {
                e.dim = dim;
                e.styled = Id::new();
            }
            return e.styled.clone();
        }
        let styled = Id::new();
        self.entries.push(DimEntry {
            inner: inner.clone(),
            dim,
            styled: styled.clone(),
            seen: true,
        });
        styled
    }

    /// フレームの最後: 使われなかった項目を捨てる。
    pub fn end_frame(&mut self) {
        self.entries.retain(|e| e.seen);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_id_is_kept_while_the_dim_is_the_same_and_replaced_when_it_changes() {
        let inner = Id::new();
        let mut ids = DimIds::default();
        ids.begin_frame();
        let a = ids.styled_id(&inner, 0.75);
        ids.end_frame();
        ids.begin_frame();
        let b = ids.styled_id(&inner, 0.75);
        ids.end_frame();
        assert_eq!(a, b);
        ids.begin_frame();
        let c = ids.styled_id(&inner, 0.5);
        ids.end_frame();
        assert_ne!(b, c);
        assert_ne!(c, inner);
    }

    #[test]
    fn entries_that_are_not_used_in_a_frame_are_dropped() {
        let inner = Id::new();
        let mut ids = DimIds::default();
        ids.begin_frame();
        let a = ids.styled_id(&inner, 0.75);
        ids.end_frame();
        ids.begin_frame();
        ids.end_frame();
        ids.begin_frame();
        let b = ids.styled_id(&inner, 0.75);
        ids.end_frame();
        assert_ne!(a, b, "使われなかったフレームで項目は消え、新しい Id になる");
    }
}
