# contrast-core 仕様の追補: 19 節 縦のスワイプとゴム（P15）— core は変更しない

> `docs/core-spec.md` は人間だけが編集する。この追補は、「ジェスチャー系の残り」（縦のスワイプの追従、rubber band、
> オーバービューでの DnD の縦の端スクロール）に **core の変更が要らない**ことの記録。

## 19. 判断

根拠は niri のソース `src/layout/monitor.rs`（`workspace_switch_gesture_begin`・`_update`・`_end`・`dnd_scroll_gesture_*`）と
`src/rubber_band.rs`、`src/layout/scrolling.rs`（横の `view_offset_gesture_update`）。

- **D93 縦のスワイプの連続量はシェルのもの**: niri の縦のジェスチャーは、ワークスペースの位置を小数（`current_idx`）で持ち、
  `start_idx + 指の移動 / 300 px` を、今の ±1（と端）にゴム（硬さ 0.5、限界 0.05）で収めて描く。終わりに `round` で 1 つのワークスペースを選ぶ。
  core が持つのは**離散のアクティブなワークスペース**（`SwitchWorkspace`、D74 の整理を含む）だけで、小数の位置は描画の状態。
  よって core には新しい Command・状態を足さない。シェルが、指の間は小数の位置で描き、終わりに既存の `SwitchWorkspace` を 1 回出す。
- **D94 ゴムは縦だけ**: niri の横のスクロール（`view_offset_gesture_update`）にゴムは無い（`scrolling.rs` に該当なし）。core の D87
  （`dnd` でなければ範囲を制限しない）のままでよい。
- **D95 DnD の縦の端スクロール**: ワークスペースの小数の位置を、ポインタが作業領域の上下の端にいる間だけ進め、DnD が終わったら
  最も近いワークスペースへ `SwitchWorkspace`（D94 と同じ）。core の変更は要らない（シェルの状態）。
- **D96 core が答えられない問い**: 窓の列・列の中の位置は `StateView` に無いので、IPC はタイルの矩形から数える（P12 の記録のとおり）。

テスト: ゴムの式（`rubber_band.rs`）、縦の追従の式（`gestures.rs`）はコンパイラ側の単体テスト。core のテストは変更なし。
