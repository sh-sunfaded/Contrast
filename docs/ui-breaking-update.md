# Contrast UI 破壊的アップデート提案 v0.3

日付: 2026-11-13 / 置き場所: `docs/ui-breaking-update.md`（v0.2 を置き換え）

改訂内容（v0.3）: U2 の実装指示で DU-10〜DU-19 を決めた。DU-1 を DU-10 で訂正（「出力」= アクティブな出力）。DU-18（乗数と `l = 0.8` は仮置き）と DU-19（`l` は知覚コントラストの保証ではない）を §7 の未決に加えた。実装の状況は §6 の U2 に書いた。

改訂内容（v0.2）: U0 報告（`.cc-logs/report-u0-ui-update.md`）の事実と、Q1〜Q8 の決定を反映。v0.1 の前提（core が P0〜P2、枠 3px、サーバー側装飾）は誤りだった。実際は HEAD `114bc2c`（v1.0.0）、core は P15、枠 3px は組み込み既定のみ（実設定は 4px）、xdg-decoration は公開していない。

## 0. 目的と非目的

- 目的: UI 作業効率を上げる。「Contrast＝差」を設計原理にする（明度＝注意、空間＝記憶、時間＝因果）
- 非目的: niri 互換のキー操作・レイアウト意味論の変更（ただし DU-1 は niri 互換に触れうるので §7 で要確認）/ contrast-core への CDL 依存の持ち込み / Kani の利用 / CDL リポジトリの変更

## 1. 再考証（v0.1 から維持）

根拠の確度は低〜中。研究は統計グラフ・Windows 7・メニュー操作が対象で、タイリング型コンポジタへの当てはめは仮説。一部は講義資料などの二次資料で、実装前に一次文献を確認する。

- 窓の再配置がタスク切替の主コスト（Jeuris ら 2018）: 動機づけ。自己計測で確かめる
- 一括変形は空間記憶への影響が小さい（Scarr 2014）: 不変条件の根拠（仮説）
- ガイドが多いと空間学習が減る（Scarr 2014）: ヒントは呼出時のみ、習熟で減衰
- ズームは認知負荷を伴う（Cockburn ら 2008）: 離散段階、最後に着手
- 動きは一貫していれば有益（Heer & Robertson 2007）: 操作ごとに固定、短く
- APCA の Lc は文字用途の指標: 目標値は実測で決める。Lc 75/60/45 は流用しない
- 400ms は経験則: 動きの上限として採用、既定値は仮置き

## 2. 3 層構造

- L0 niri 互換挙動: 変更しない（DU-1 のみ例外候補）
- L1 不変条件: core のテストと検証で守る。CDL から触れない
- L2 可変: CDL で宣言する見た目・動き

## 3. 決定事項

| ID | 決定 |
|----|------|
| DU-1（DU-10 で訂正） | INV-F1: 表示中ワークスペースに窓が 1 枚でもある出力では、フォーカスは Some（← 出力が複数のときは曖昧。DU-10 で「アクティブな出力」に訂正）。`windowOpen` の `focus:false` は「既にフォーカス窓があるとき」だけ有効で、フォーカスが None のときは無視して新しい窓にフォーカスを渡す |
| DU-2 | INV-S1/S2/Z1 の観測は公開 API を追加せず、仕様テストが `WindowView.rect` と `Effect::Configure` から列を復元する。足りなければ、その時点で API 追加を提案する |
| DU-3 | core で守る間隔は「隣り合う列の隙間 = 0」のみ。`gap` / `outerGap` はコンポジタの描画テストで確かめる |
| DU-4 | 新しい不変条件は `tests/spec` の検査関数に置く。`InvariantViolation` に variant を足さない |
| DU-5 | Motion の曲線は長さを持つ easing 系だけ。spring は Motion の対象外とし、niri 互換の置き方は U3 計画で提案する |
| DU-6 | CDL は変えない。順序付き列挙と範囲は Contrast の変換で検査する。「フォーカス段階は 1 つだけ」は専用の一意の子（例: `FocusTier`）で守る設計を U2 で検討する |
| DU-7 | Profile / `when` と `Env` 宣言は U4 まで保留する。U2・U3 は静的ルートのまま進める |
| DU-8 | 旧 prop は一度に壊す。旧 prop は未知 prop として診断（リロードは旧設定を保持）。`config/`・`examples/` は同時に書き換える |
| DU-9 | INV-A1 の解釈は「フォーカス段階との差が、非フォーカス段階全体の幅より大きい」 |
| DU-10 | **DU-1 の訂正**: INV-F1 は「アクティブな出力（`active_output()`）の表示中ワークスペースに窓が 1 枚でもあれば、フォーカスは Some」。`windowOpen` の `focus:false` は、既にフォーカス窓があるときだけ有効で、フォーカスが None のときは無視して新しい窓にフォーカスを渡す |
| DU-11 | `focusColor`・`borderColor` は Tier の `ringColor` に移す。`background`・`shadowColor` は `Appearance` に残す |
| DU-12 | INV-A1 の式は `F − max(N) > max(N) − min(N)`（狭義）。差は OKLCH の L の差。APCA の Lc は使わない |
| DU-13（暫定） | フォーカス None で、アクティブでない出力の表示中ワークスペースに `focus:false` の窓が開いたら、窓にフォーカスを渡し、アクティブな出力もそちらへ移す（`set_focus` の既存の動作）。niri の実挙動は未確認。この場合の期待値は人間が確定する（テストで固定しない） |
| DU-14 | 人間の仕様テストの置き場は `crates/contrast-compositor/tests/spec/` と `crates/contrast-policy-cdl/tests/spec/`（人間が編集） |
| DU-15（DU-20 で改訂） | 段階数は `FocusTier` + `Tier` 0 個または 1 個（計 1〜2 段）。スキーマは `Tier` の個数を制限せず、変換が 2 個以上を CT064 で拒否する（この版では 1 つまで） |
| DU-20 | `Tier` は 0 個または 1 個。0 個は「減光なし（乗数 1.0）」。2 個以上は CT064。`Tiers` を省略したときの組み込みの既定は変えない（`FocusTier l=1.0` + `Tier l=0.8`）。INV-A1 は `Tier` が 0 個のとき自明に成り立つ。`examples/niri.cdl` は `Tier` を置かない（niri には減光がない） |
| DU-16 | `config/appearance.cdl` は現行と同じ枠（4px・`#7fc8ff`）を `borderWidth` と `FocusTier.ringColor` で残し、減光だけを足す。`config/theme.cdl` に `TIER_*` 定数を足す |
| DU-17 | 減光の方式は「窓のテクスチャシェーダに乗数を足す」（`StyledSurface` の `dim`）。アルファ・矩形・枠・影は変えない。`screenshot-window` は常に乗数 1 |
| DU-18 | 乗数 `k = (L_tier / L_focus)^(3/2.2)` と、組み込みの `l = 0.8` は**仮置き**（中立のグレーでだけ正確な近似）。目標値ではない |
| DU-19 | Tier の `l` は「窓の画素を何倍に暗くするか」の指標で、周囲（壁紙・隣の窓）に対する知覚コントラストの保証ではない |

## 4. 不変条件

| ID | 内容 | 守る場所 | 検証 | 着手 |
|----|------|---------|------|------|
| INV-S1 | 列の相対順序・幅・間隔は「列を変える命令」以外で変化しない。「列を変える命令」は挿入・削除・移動・幅変更を起こす命令全般（クライアント起因の `WindowMapped` / `WindowUnmapped` を含む。分類は U0 報告 §4）。後続列の変位は一括平行移動のみ。明記する例外: `OutputChanged` / `OutputInsetsChanged` による Proportion 幅の再計算（幅のみ変わり、順序と一括性は保たれる）。ビューのジェスチャーと `view_x` のクランプは一括平行移動で、反しない | core | proptest | U1 |
| INV-S2 | ビュー変換は平行移動と一様拡縮のみ（リフローなし）。現状は平行移動のみで自明に成立 | core | proptest | U5 |
| INV-F1 | DU-10 のとおり（DU-1 を訂正） | core | proptest | U1 |
| INV-R1 | 窓の矩形は、減光・枠・影の有無や強さで変わらない（枠は窓の外側に描く現状を保つ） | コンポジタ | 描画テスト | U2 |
| INV-A1 | DU-9 のとおり。段階は単調 | スキーマ検証（変換） | 診断（要素単位の位置） | U2 |
| INV-T1 | Motion の長さは上限（既定 400ms）を超えられない。spring は対象外（DU-5） | スキーマ検証（変換） | 診断（要素単位の位置） | U3 |
| INV-M1 | 操作種別→動き種別の対応は固定（スクロール=平行移動、フォーカス=明度遷移、ワークスペース=縦移動）。対象操作の範囲は §7 | 描画層 | 例示テスト | U3 |
| INV-Z1 | ズーム段階は離散（2〜3）。段階間で窓の相対位置が保たれる（丸めの許容は §7） | core（幾何）＋スキーマ | proptest | U5 |
| INV-K1 | キー層（mode）から必ず抜けられる（Escape 等） | コンポジタ | 例示テスト | U4 |

## 5. 破壊的変更

### 5.1 core

- CB-1: INV-F1 の実現。`window_mapped.rs:76` 周辺と `fix_focus_on_active_output`（`handlers/mod.rs:210`）を直す。公開 API のシグネチャは変えない。D6・D7 との整合を確認する
- CB-2: `core-spec.md` への追記（人間が書く）。INV-S1 の分類表と例外、INV-F1 の範囲
- CB-3（U5）: ビュー変換型（平行移動＋拡縮）、ズーム段、矩形の拡縮関数。公開 API の追加なので、人間が規則 5 の例外を明示して許可する

### 5.2 コンポジタ

- CP-1: フォーカス外の窓の減光（現状は未実装、新規）。方式は U2 計画で比較する（窓のバッファに掛ける / 重ねる矩形 / シェーダ）。`screenshot-window` には入れない
- CP-2: 枠・影・角丸を「既定で描かない・任意」にする（`render/mod.rs:500-614`）
- CP-3（U3）: Motion を操作種別ごとにする（`anim.rs`、`window_anim.rs`）。窓の開閉演出（150ms 固定）の扱いは §7
- CP-4（U4）: キー層の照合と一覧表示（`input/bindings.rs`、`hotkey_overlay.rs`）

### 5.3 CDL 設定（Contrast 側スキーマ）

- CC-1: Appearance の色直指定（`focusColor`・`borderColor`・`background`・`shadowColor`）→ Tier トークン。`borderWidth`・`focusColor` の必須を外す。`Settings.appearance` とポートの `focus_border_color()` / `unfocused_border_color()` を変更する
- CC-2（U3）: `Layout.animation` / `Layout.animationMs` を廃止し、Motion に置き換える。`MAX_ANIMATION_MS = 2000` と INV-T1 の上限を整理する
- CC-3: Tier / `FocusTier` を新規追加する。検査は Contrast の変換で行い、新しい診断コードを割り当てる
- CC-4: `config/`・`examples/`・`crates/contrast-policy-cdl/tests/` の書き換え（U0 報告 §2.1 の一覧）
- CC-5（U4）: Keybind の同一性を (mode, 修飾キー, キー名) に変える（CT031）。`Env` 宣言と Profile も U4

## 6. フェーズ

各フェーズは、計画の承認後に着手する。コードの変更は Claude Code が行い、`tests/spec` は人間が書く。

- U1（人間）: 仕様テストと `core-spec.md` の追記。INV-S1・INV-F1・INV-R1、niri との差の確認（§7）
- U2（Claude Code）: Tier・減光・枠の任意化（CC-1・CC-3・CC-4、CP-1・CP-2）と、core の CB-1。CB-1 は U1 の仕様テストが先
  - 実施状況: 段階 B1（`Tiers` の検査・CT060〜CT065）、B2（スキーマ・変換）、C1（減光）、B3（旧 prop の廃止・枠の任意化・`config/`・`examples/` の書き換え）、D（文書）は実施。**段階 A（core の CB-1）は未実施**: `docs/core-spec.md` に INV-F1 の追記と `tests/spec` の検査が無い（実行前のゲート G2 が不成立）。したがって DU-10 の core 側（`focus:false` でフォーカス None のとき新しい窓にフォーカスを渡す）は未実装。詳細は `.cc-logs/report-u2-impl.md`
- U3（Claude Code）: Motion（CC-2、CP-3）
- U4（Claude Code）: Profile と `Env`（出所の決定が先）、キー層（CC-5、CP-4）
- U5（Claude Code）: ズームとヒント減衰（CB-3 は人間の許可が先）

## 7. 未決・要確認

- niri との差（U0 報告 §5-B）: 列の挿入位置、挿入・削除時の `view_x`、削除後に動く列、広い列のスクロール、Proportion 幅の再計算。U1 で人間が確かめる
- DU-1（DU-10）の niri 側の挙動（フォーカスが None のときに `focus:false` で窓が開く場合）
- INV-M1 の対象操作（開く・閉じる・ワークスペース・リサイズ・スクロールのどれを含めるか）と、開閉演出 150ms 固定の扱い
- niri 互換の spring の置き方（DU-5。U3 計画で提案）
- INV-Z1 の丸めの許容（i32 の丸めでは厳密一致にならない）
- Tier の L の値と、視覚差の計測指標: 段階数（2 段）と指標（OKLCH の L 差）は DU-15・DU-12 で決めた。**L の値（組み込みの 0.8）と乗数の式は仮置き（DU-18）**で、実測で決める
- `l` と、周囲（壁紙・隣の窓）に対する知覚コントラストの関係（DU-19）: `l` は窓の画素への乗数の指標にすぎず、周囲との差は保証しない。保証が要るなら、背景を含めた測り方を別に決める
- DU-13: フォーカス None で、アクティブでない出力に `focus:false` の窓が開いたときの期待値（niri の実挙動の確認と、人間による確定が先）
- DU-10 の core 側の実装（U2 の段階 A）: `docs/core-spec.md` の追記（INV-F1）と `tests/spec` の検査が先
- 減光の実装方式: U2 で DU-17 に決めた。乗数の値は DU-18（仮置き）。実機（DRM）の見た目は未確認（`scripts/contrast-realhw.sh` で人間が確認する）
- ワークスペース種別の出所（core に無い。U4 の前に決める）
- ヒント減衰の閾値とズームの段数（U5）

## 8. 検証

- proptest と参照モデル比較。Kani は使わない
- 研究項目: niri 互換の設定を基準に、実作業でタスク切替時間と迷子の頻度を比べる（手順は別途）
