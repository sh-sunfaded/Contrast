# contrast-core 仕様の追補案: 20 節 窓ごとの最初の列幅（P14）

> `docs/core-spec.md` は人間だけが編集する。`tests/niri_parity/placement_width.rs` とその実装は、この文書を仕様として扱う。

## 20. 窓ルールの `default-column-width`（P14。niri-complete）

作者の niri の窓ルール（wezterm に `default-column-width {}`）に合わせる。根拠は niri の `src/layout/workspace.rs` の
`resolve_default_width`（窓ルールの幅が設定の既定より優先）と `resolve_scrolling_width`（幅が無ければ窓の大きさの `Fixed`）。
「窓に決めさせる」（`{}`）の、窓の大きさを読む部分はシェルの仕事で、core には決まった幅を渡す。

- **D96 Placement**: `Placement::with_column_width(ColumnWidth) -> Placement` を足す（`Placement` は `#[non_exhaustive]`。既定は無し）。
  `Policy::place` がこれを返したら、その窓が `Scrolling` で最初に入る新しい列の幅を、`default_column_width` の代わりにこの幅にする。
- **D97 範囲**: 最初の列だけ（Floating・Fullscreen で始まった窓は、Tiled になって最初に入る列）。そのあと別の列へ移る（Expel など）ときは通常どおり既定の幅。
  `Equal` では使わない。窓を閉じれば忘れる。
