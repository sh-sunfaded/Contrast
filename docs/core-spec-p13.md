# contrast-core 仕様の追補案: 19 節 移すだけの move-column-to-workspace（P13）

> `docs/core-spec.md` は人間だけが編集する（17 節は `docs/core-spec-p11.md`、18 節は `docs/core-spec-p12.md`）。
> `tests/niri_parity/move_column_no_focus.rs` とその実装は、この文書を仕様として扱う。

## 19. `move-column-to-workspace … focus=false`（P13。niri-complete）

根拠は niri の `src/layout/monitor.rs` の `move_column_to_workspace(idx, activate)`（`activate` が false）と、
`src/layout/scrolling.rs` の `remove_column_by_idx`（アクティブな列を外したら、右の列、なければ左の列をアクティブにする）。

- **D93 新しい Action**: `MoveColumnToWorkspaceNoFocus(WorkspaceIndex)`。対象の決め方（`Dynamic` は末尾に丸め、`Fixed` は範囲外で `WorkspaceIndexOutOfRange`、
  `Equal` では状態不変で `Ok`、フォーカスが None なら `NoFocusedWindow`、対象が今のワークスペースなら状態不変で `Ok`）は D67 と同じ。
- **D94 動作**: D67 と同じく、フォーカス窓が Tiled なら列ごと（幅・窓ごとの高さ・最大化の状態を保つ）を、対象の「最後にフォーカスした列」の右隣（窓が無ければ先頭）へ、
  Floating・Fullscreen なら窓だけを移す。ただし、**表示中のワークスペースは切り替えない**。フォーカスは元のワークスペースに残る:
  列ごと移したときは、元の列の位置の右の列（無ければ左の列）の「最後にフォーカスした窓」、列が残っていなければ None。窓だけを移したときは D13 と同じ（元のワークスペースの順序列の先頭、無ければ None）。
  対象のワークスペースのフォーカス履歴は変えない（移った窓は対象で最新にならない）。表示が変わらないので、D65 の整理はしない。D64（末尾の空）は通常どおり。
- **D95 niri との差（記録）**: niri は浮動の窓を外したあと、残った浮動の窓か、タイルの列へフォーカスを移す（浮動の窓どうしの規則がある）。Contrast は D13 と同じ規則にする。
