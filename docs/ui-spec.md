# Contrast UI 仕様 v1.0 草案（破壊的アップデートの統合仕様）

日付: 2026-11-14 / 置き場所の想定: `docs/ui-spec.md`。採用すると `docs/ui-breaking-update.md`（v0.3）を置き換える。

状態の凡例: **[確定]** 人間が承認済み / **[実装済]** U2 で実装・検証済み（`report-u2-impl.md`、`report-u2-remaining.md`） / **[暫定]** 推奨案で、人間の承認待ち / **[未決]** 決定材料が足りない。

**2026-11-14 の承認**: DU-21〜29（§10）は承認済み。表中の [暫定] のうち DU-21〜29 に由来するもの（Motion の仕様、INV-T1・INV-M1、IPC、U3 の順序）は [確定] と読む。命令→種別の表、niri との差、ワークスペース種別の出所は、引き続き [未決]。**CDL の破壊的アップデートは、必要なら Contrast の U3 より先に行う**（技術的負債を残さないため。§9 の段階 C）。

作成時の限界: 私は現行リポジトリ・CDL 仕様・niri のソースを直接読んでいない。根拠は U0・U2・U3 の各報告と、会話での決定。採用時に Claude Code が現物と突き合わせる（§10）。

## 1. 原理

1. **Contrast は「差を測る実験装置」** [確定]。名前の「差」は二重の意味を持つ。(a) 画面上の差（明度＝注意、空間＝記憶、時間＝因果）。(b) 設定間の差（niri 互換の基準線と変種を、同じ core で CDL だけ替えて比べる）
2. **niri は完成した土壌** [確定]。最終目標は、niri の挙動との完全一致（基準線）を CDL で実現し、足りない所を core で拡張すること。見た目は基準線から破壊的に変えてよい。
3. **可変は無限、ただし理論が守る境界の内側** [確定]。変えてよいもの（CDL）と、変えてはいけないもの（core の不変条件）を分ける。
4. **研究優先** [確定]。失敗は学びで、安定性より検証を優先する。ただし、実験の結論は 1 人の自己実験なので探索的なもの（§7）。
5. 理論（前注意的処理、同時対比、空間記憶、アニメーションの一貫性など）は、統計グラフ・Windows 7・メニュー操作などの研究からの**当てはめで、タイリング型コンポジタでは未検証の仮説**。設計の根拠ではなく、§7 の検証対象。

## 2. 層と責任

| 層 | 内容 | 編集する人 |
|---|---|---|
| L0 土壌 | niri 互換の挙動。基準線は `examples/niri.cdl`（減光なし） | 仕様は人間、実装は Claude Code |
| L1 不変条件 | core が保証し、`tests/spec`（人間）で検証する | 人間（仕様テスト・`core-spec.md`）。実装は Claude Code |
| L2 可変 | CDL で宣言する見た目・動き・プロファイル。検査は Contrast の変換 | Claude Code が実装。スキーマ方針は人間 |
| L3 観測 | 互換 IPC と記録。**動作に影響しない** | Claude Code |

規則: core は CDL を知らない。CDL は `cdl-host` の公開 API だけを使う。CDL リポジトリは別プロジェクトで、変更が要るときは `docs/cdl-requests.md` の形で報告する。

## 3. 不変条件（規範）

| ID | 内容 | 守る場所 | 検証 | 状態 |
|---|---|---|---|---|
| INV-S1 | 列の相対順序・幅・間隔は、列を変える命令（挿入・削除・移動・幅変更を起こす命令全般。クライアント起因の `WindowMapped`・`WindowUnmapped` を含む。分類は U0 報告 §4）以外で変化しない。後続列の変位は一括平行移動のみ。**例外**: `OutputChanged`・`OutputInsetsChanged` による `Proportion` 幅の再計算（順序と一括性は保たれる）。間隔は core では 0（`gap`・`outerGap` はコンポジタ側）。ビューのジェスチャーと `view_x` のクランプは一括平行移動で反しない | core | proptest（人間） | [確定]。niri との差の確認は [未決]（§10） |
| INV-S2 | ビュー変換は平行移動と一様拡縮のみ（リフローなし）。現状は平行移動のみで自明に成立 | core | proptest | [確定]（拡縮は U5） |
| INV-F1 | アクティブな出力（`active_output()`）の表示中ワークスペースに窓が 1 枚でもあれば、フォーカスは Some。`windowOpen` の `focus:false` は、直前にフォーカス窓があるときだけ有効で、None のときは無視して新しい窓へフォーカスを渡す | core | 仕様テスト（人間） | [確定]（DU-10）。非アクティブ出力の場合は [暫定]（DU-13） |
| INV-R1 | 窓の矩形は、減光・枠・影の有無や強さで変わらない（枠は窓の外側に描く） | コンポジタ | 描画テスト（GL） | [実装済]（GL テストはスキップなし）。幾何テストは未実装 |
| INV-A1 | `F − max(N) > max(N) − min(N)`（狭義）。`F` はフォーカス段階、`N` は非フォーカス段階の `l`。差は OKLCH の L の差。`Tier` が 0 個のときは自明に成立 | スキーマ検証（変換） | 診断（段階の `l` の prop の位置。複数段階にまたがる検査は要素の位置） | [実装済]（DU-9・12・20） |
| INV-T1 | Motion の長さ（easing 系の `duration`）は、定数 `MAX_MOTION_MS = 400` 以下。設定で上げられない。**`niriSpring` は長さを持たず対象外**（niri のバネの収束は手計算で約 0.4〜0.7 秒で、対象に入れると基準線が自分で違反するため） | スキーマ検証（変換） | 診断（`duration` prop の位置。CDL 0.2.x の prop 位置） | [暫定]（DU-21・22） |
| INV-M1 | 動きは 6 種別（Scroll・Resize・Workspace・Focus・Open・Close）。**命令単位で種別が 1 つに決まる**。矩形に掛かる種別は同時に 1 つ。Focus は明度のチャンネルで、矩形の動きと同時に走ってよい（明示した例外）。割り込みは「置き換え」で、キューに積まない | コンポジタ | 例示テスト（人間） | [暫定]（DU-23）。命令→種別の表は人間のレビュー待ち（§5） |
| INV-Z1 | ズーム段階は離散（2〜3）。段階間で窓の相対位置が保たれる。丸めの許容: 矩形の各辺を同じ写像で別々に写すので、**順序の保存と隣接の厳密な保存**（隙間・重なりが生まれない）、**大きさ・間隔の誤差は 1 論理 px 未満**、等倍は恒等、合成（1/2 を 2 回 ≈ 1/4）は辺ごとに ±1 px 以内。GPU の丸めとの差は ±1 論理 px | **policy（純粋関数）**＋スキーマ（core は変えない最小案）| proptest | [確定]（DU-40〜48。U5）
| INV-K1 | キー層（mode）から必ず抜けられる | コンポジタ | 例示テスト | [暫定]（U4） |
| **INV-L1** | **観測は動作を変えない**: IPC・記録を有効にしても無効にしても、core の状態列と描画結果は同じ | コンポジタ | 差分テスト（有効/無効で同じ命令列→同じ `Effect` 列） | [暫定]（新規） |

## 4. CDL 設定スキーマ

### 4.1 Appearance と Tier [実装済]

- `Tiers`（`Contrast` の一意の子、省略可）、`FocusTier`（`l`、`ringColor?`。`Tiers` の中で一意）、`Tier`（`l`、`ringColor?`、`name?`）
- `Tier` は 0 個または 1 個。0 個は減光なし（乗数 1.0）。2 個以上は CT064 [確定]（DU-15・20）
- 省略時の組み込み既定: `FocusTier l=1.0` + `Tier l=0.8`（乗数は仮置き: `k = (L_tier/L_focus)^(3/2.2)`。グレーでだけ正確な近似。周囲に対する知覚コントラストの保証ではない）[確定]（DU-18・19）
- 旧 `focusColor`・`borderColor` は廃止（未知 prop として診断）。`borderWidth` は省略可（0 = 枠なし）。`background`・`shadowColor` は残す [実装済]（DU-8・11）
- 診断コード: CT060（`l` 不正）、CT061（`FocusTier` なし）、CT062（単調でない）、CT063（INV-A1 違反）、CT064（`Tier` 2 個以上）、CT065（`name` 重複・空）
- 減光は窓のテクスチャシェーダに `dim` を乗せる方式。`screenshot-window` は常に `dim = 1.0` [実装済]（DU-17）

### 4.2 Motion [暫定]（U3）

```
<Motion>                               // Contrast の一意の子。省略 = 全種別が即時
  <Scroll    curve? duration? />       // 平行移動（スクロール・窓の移動・列の並べ替え）
  <Resize    curve? duration? />
  <Workspace curve? duration? />       // 縦移動
  <Focus     curve? duration? />       // 明度遷移（減光の補間）
  <Open      curve? duration? />       // 見た目は透明度＋拡大で固定
  <Close     curve? duration? />
</Motion>
```

- 種別ごとに高々 1 つ。省略した種別は即時。種別は要素名で固定
- 曲線: `easeOutCubic`（既定）、`easeOutQuad`、`easeOutExpo`、`easeInOutCubic`、`niriSpring`（互換専用。Scroll・Resize・Workspace・Open・Close のみ。Focus には不可。長さを持たない）。`linear` は置かない
- 既定の曲線: 全種別 `easeOutCubic` [実装済]（`<Motion>` を使うと Open・Close は従来の expo・quad ではなくこれになる。従来の見た目は `<Open curve="easeOutExpo"/>`・`<Close curve="easeOutQuad"/>` で再現できる）
- 既定の長さ（仮置き）: Scroll・Resize・Workspace 150ms、Focus 120ms（新規・実機で決める）、Open・Close 150ms
- 診断（実装済）: CT070（easing の長さが 0 未満・有限でない・400ms 超）、CT071（未知の曲線）、CT072（`niriSpring` と `duration` の併記、または `Focus` に `niriSpring`）、CT073（同種別の重複）。コードの空きは実装時に確認
- 長さの型: `duration: Duration`、書式は `ms(150)`（CDL にリテラルは無い。`sec(0.15)`=150ms も通るが、整数ミリ秒にならない小数は E5009）。`ms(-5)` は CDL を通るので、**範囲（0 以上、400ms 以下）は Contrast の変換で検査する** [確定]（CDL 0.2.1 の実測。現在の policy crate には Motion も `duration` も未実装で、`Layout.animationMs`（`Number`）がある。U3 で Motion へ移す）
- 旧 `Layout.animation`・`animationMs` は**廃止済み**（U3 段階 (d)、2026-11-14）。書くと CDL が未知 prop（E4003、その prop の位置）として拒否し、リロードは旧設定を保持する。`<Motion>` なし = 全種別即時（旧既定も即時だったので組み込みの見た目は不変）。CT074 は削除。`examples/niri.cdl` は `<Motion>` に `niriSpring` の 5 種別（Focus なし）を書く。移行の書き方は `docs/cdl-contract.md`
- 対象外: オーバービューの開閉、ワークスペースのジェスチャー追従、スクリーンショット UI（物理・指追従・システム UI）

### 4.3 プロファイルと文脈 [実装済: DU-30〜39]（U4。2026-11-15 承認・実装。段階 (a)(b)(c) 完了、(d) core は未着手）

- **`Env`**（読み取り専用・純粋・決定的。型は `import type { Env } from "host:contrast-env"`。`host:contrast` には既に要素 `Env`（`Environment` の子）があり、同じモジュールで要素と型の名前が重なれないため別モジュール）: `{ profile: string, mode: string, output: { name }, workspace: { index, name?, windowCount }, focus?: { appId, isDialog, state: "tiled"|"floating"|"fullscreen" } }`。`title` は入れない（変化が頻繁なため）。出力ごとの `Env` は対象外（`Settings` は全体で 1 つ）
- **ルート**: 関数形（`export default function App(env: Env)`）を許す。`check` は本体を評価しないので、`load` の後に代表の `Env` で `render` し、`Settings` への変換まで通す（CT081）。実行時の `render`・変換の失敗は、直前の設定を保持して警告（CT080）。静的ルートの設定（`examples/niri.cdl`、`config/`）は従来どおり。`Env` が前回と等しければ再 `render` しない。`Env` 由来の「適用中の設定」は `ConfigStore` の世代とは別の層に持つ
- **Profile**: 新しい要素は足さない。`env.profile`（1 つの文字列。既定 "default"）と条件（`c && <A/>`）で書く。同じ要素を複数の条件が出すと CDL の一意の子の検査（E4006）で衝突が禁止される。`profile` の入力は `~/.config/contrast/profile`（1 行）または `CONTRAST_PROFILE`、反映は `reload-config`（A/B の差が 1 か所に集まる）。切り替える対象は Tiers・Motion・Appearance・Keybind。`Layout.mode`・`Workspaces` は切り替えない
- **名前付きワークスペース（H4）**: 当面は **core を変えない**。`<Workspaces>` の子 `<Workspace name>`（並び = 位置。`<Motion>` の中の種別 `<Workspace>` と同名。どちらで使えるかは変換が検査）の名前表を policy が持ち、`workspace-name` が既存の `SwitchWorkspace` に写る。対象は `Fixed` のみ（`Dynamic` で名前を使うと CT083）。IPC の `name`・`FocusWorkspaceName` もこの表から。core に名前を持たせる案（C。空でも残る名前）は H4 の所見の後で、規則 5 の例外許可・`core-spec.md` の追記・`tests/spec` が先
- **シーン**: `<Workspace>` の子に `<Launch command/>`。宣言は純粋、起動は手動の命令 `scene`（`OpenScene`。先に切り替えてから spawn）。自動起動は入れない
- **キー層（INV-K1）**: 層は `<Mode name fallthrough?>` で宣言。`Keybind.mode?`（属する層。既定 "normal"）、`Keybind.name`（`enter-mode`・`workspace-name`・`scene` の対象）、`enter-mode`・`exit-mode`。同一性は `(mode, 修飾キー, キー)`（CT031 の拡張）。保証は 2 重: (i) `normal` 以外の層に抜けるキーが無ければ CT082、(ii) 層の中では修飾キーなしの `Escape` が常に `exit-mode`（宣言で上書き不可）。オーバービュー・スクリーンショット UI は層より先にキーを奪う順序を保つ。タイムアウトは入れない
- **観測の記録**: コンポジタ内蔵の JSONL（`CONTRAST_RECORD` で有効。既定は無効。単調時計 + 壁時計。フォーカス・ワークスペース・窓の出現消滅・設定の読み込み（`profile` つき）・層の遷移。`title` は記録しない）。IPC と同じ差分から作り、状態へ戻さない（INV-L1）
- **Env 由来の減光**: `Tier` の `l` が `Env` の変化で変わるときは**補間する**。リロードだけ即時（Q10 の理由は `Env` に当てはまらない）
- 診断（実装済。CT083 は名前の問題全般 = ワークスペース名・層名の重複・空・`Dynamic`・`count` 超過）: CT080 実行時 `render` 失敗、CT081 読み込み時の代表 `Env` の失敗、CT082 抜けられない層、CT083 名前の重複・`Dynamic` での名前、CT084 未知の層・名前、CT085 未知のシーン
- `Env` を `Layout`・`Workspaces` の出し分けに使っても、代表の `Env` では検出されず、実行時に CT050 の警告で無視される（静的には禁止していない）。CDL の型の制約: `&&` の左辺は `boolean`、`T | undefined` のオブジェクトは `?.` で読む、`c && <A/>` は配列に入れず子の位置に直接書く
- 診断コード・決定 DU-30〜39 の割り当て: 30 名前表（core 不変）/31 `Fixed` のみ/32 `Env` 最小/33 Profile は条件/34 Env 由来の減光は補間/35 キー層の 2 重の保証/36 シーンは手動/37 JSONL 内蔵/38 profile は 1 入力/39 診断コード

### 4.5 ズーム [確定: DU-40〜48]（U5。2026-11-15 承認、U5 計画の推奨どおり）

- **意味**: アクティブな出力の表示中ワークスペースのビュー全体（窓・枠・影・開閉の絵）を、作業領域の中心を基準に**描画だけ**縮める。窓の実サイズ（`configure`）は等倍のまま、矩形・順序・幅・間隔・フォーカスは変えない（INV-S1・S2・F1・R1）。実サイズも縮める案は INV-S2・R1 と衝突するので却下（ぼやけが許容できない場合に人間が INV の改訂から決める）
- **core は変えない**（最小案）: 状態は compositor（`Contrast.zoom_level`）、段階の定義は policy の設定、幾何は policy の純粋関数 `zoom`（辺ごとの写像 `pivot + round_half_away((v − pivot)/divisor)`、点の逆写像 `pivot + (q − pivot)·divisor`）。core に置く案（C）は、`view_x` の clamp を直したい等の要求が出てから、規則 5 の承認・`core-spec.md` の追記・`tests/spec` が先
- **CDL**: `<Zoom>`（`Contrast` の一意の子。省略 = 等倍のみ）の中に `<Step divisor={2〜8}/>` を 0〜2 個（厳密に増える順）。`Keybind action="zoom-in"|"zoom-out"|"zoom-reset"`（`<Zoom>` が無いのに書くと CT088）。診断 CT086（`Step`）、CT088。基準線 `examples/niri.cdl` は `<Zoom>` を書かない。既定の例は等倍と 1/2 の 2 段
- **操作**: ピンチはクライアントへ転送しているのでズームに使わない（キーのみ）。ズーム中は 3 本指のビュー操作・DnD の端スクロール・4 本指のオーバービュー・スクリーンショット UI を無効。ワークスペース切替ではズームを維持、オーバービューは等倍に戻してから開く。`screenshot-screen` は見えているとおり、`screenshot-window` は変更なし
- **ポインタ**: 画面の点を逆写像で等倍の点にして、窓の選択（`visible_key_at`・クリック・ホバー）に使う。クライアントへの面内座標は、`pointer.motion` の `location`（画面の位置）と面の原点の組み合わせで両立させる案を試す。解けなければズーム中のポインタ入力を無効にして止める
- **動き**: 遷移は即時（種別 `Scale` は実機で必要と分かってから。要素名 `Zoom` と衝突させない）。`Env.zoom` は足さない（必要が出てから）
- **ヒント減衰は保留**: 定義が無い（`ui-breaking-update.md:20` の「ヒントは呼出時のみ、習熟で減衰」だけ）。人間が定義を書いてから。測定（H1〜H4）が先で、n=1・習熟の交絡があるため、効果の検証は難しい
- 既知の負債: ズーム中は `view_x` の clamp が等倍の意味なので、端に余白が出る（P4）。窓の枠・影・角丸・減光（`StyledSurface`）を縮める経路が無く、(b) の中心の作業（P6）

### 4.4 破壊的移行の規則 [確定]（DU-8）

旧 prop は一度に壊す。壊すのは**最後の段階**にして、それまでは「足す」だけにする。旧 prop は未知 prop として診断され、リロードは失敗時に旧設定を保持する。`config/`・`examples/` は同時に書き換える。

## 5. 動きの仕様（INV-M1 の対応表）[暫定]

規則: `submit(command)` が、その命令で起きる矩形の変化の種別を 1 つ決め、その命令の `Configure` 全部がその種別の曲線を使う。

| 種別 | 命令 |
|---|---|
| Scroll | `FocusDirection`、`FocusColumnFirst/Last`、`CenterColumn`、`MoveWindow`、`MoveColumnToFirst/Last`、`ConsumeWindowIntoColumn`、`ExpelWindowFromColumn`、`ConsumeOrExpelWindowLeft/Right`、`ViewGesture*`、クライアント起因の `WindowMapped`・`WindowUnmapped` による隣の列の動き |
| Resize | `SwitchPresetColumnWidth`、`MaximizeColumn`、`SetColumnWidth`、`SetWindowHeight`、`SwitchPresetWindowHeight`、`ResetWindowHeight`、`ToggleFloating`、`ToggleFullscreen`（復帰） |
| Workspace | `SwitchWorkspace`、`MoveToWorkspace`、`MoveColumnToWorkspace`、`MoveColumnToWorkspaceNoFocus`、`MoveWorkspaceUp/Down` |
| Focus | 命令ではなく状態（フォーカス窓が変わったこと）。どの命令でも、変われば起きる |
| Open / Close | 窓の出現・消滅 |
| （U3 実装で追加・暫定） | `CloseFocused` = Scroll（隣の列が詰める）、`FocusMonitor` = Workspace（窓は動かさないので矩形の補間は起きない。Focus の明度遷移のみ見える）、`MoveColumnToMonitor` = Workspace（移動先が別の出力で始点が無い。実機で確認）、`ToggleFullscreen` = Resize（全画面にするときは従来どおり補間しない）。命令でない入力（`WindowMapped`・`WindowUnmapped`・ジェスチャー）と未知の `Action` は Scroll |
| 補間なし | 出力の大きさの変更、`OutputChanged`・`OutputInsetsChanged`、全画面にするとき |

- 要レビュー（人間）: `FocusMonitor` は窓を動かさずフォーカスだけが変わるので、Workspace ではなく Focus のみかもしれない。`MoveColumnToMonitor` は移動先が別の出力で、矩形の補間に始点が無い可能性がある。`MoveToWorkspace`・`ToggleFloating` の見た目も確認する
- `Action` は `#[non_exhaustive]`。コンポジタの対応で未知の variant は Scroll にする
- 割り込み: 新しい目標は動き中のものを置き換える。始点は今表示している値。`niriSpring` は速度を引き継ぎ、easing は引き継がない
- Focus の補間: 乗数 `k` を線形に補間する（時間 → 進行度 → 曲線 → `k`）。履歴が無い窓は補間せず目標から始める。閉じる窓は、画面で見えていた乗数で止まる。補間の値は 1 か所（`displayed`）で決め、描画の 2 か所がそれを読む

## 6. 観測仕様（L3）[暫定]

- **互換 IPC**: niri の IPC（`niri msg event-stream` 相当）のイベント購読と互換にする。理由は、最終目標が niri との完全一致であることと、実機の ironbar などの既存構成を再利用する方針のため。niri の IPC の仕様は**未読**。実装の前に、仕様を調べて、サブセットを決める [未決]
- **記録**: IPC のイベント列に時刻を付けて残す。記録は L3 で、core の公開 API を変えない（命令と `Effect` の列がそのまま材料）
- INV-L1（§3）: 記録は動作に影響しない
- 指標の定義（仮説。妥当性は検証が要る）: 再フォーカス遅延（直前の窓へ戻るまでの時間）、逆向きフォーカス移動の連続回数（見失いの代理）、ワークスペース切替の所要時間、窓を探している間のフォーカス往復

## 7. 実験仕様

| 仮説 | つまみ | 基準線 | 変種 | 測るもの |
|---|---|---|---|---|
| H1 注意: 明度差で示すと、見失いが減る | Tier | `niri.cdl`（減光なし） | 減光あり（既定 `l=0.8`、他の値） | 見失いの指標、再フォーカス遅延 |
| H2 記憶: リフローしなければ位置が保てる | INV-S1・S2（core 保証） | niri の挙動 | リフローを起こす変種（負の対照）。大きな作業なので当面は保留 | 窓を探す時間 |
| H3 因果: 動きを操作ごとに固定すると迷子が減る | Motion | 即時、niri の spring | 固定の動き | 迷子の指標 |
| H4 文脈: 目的で呼べるワークスペースが切替を減らす | 名前・シーン | 番号 | TopicSpace 型 | 切替の所要時間 |

手順: 同じ作業環境で、設定だけを替えて、数日ずつ行い、ログを比べる。**限界を明記する**: 利用者は 1 人、順序効果・習熟・作業内容の違いが交絡し、結論は探索的で、統計的な検証ではない。比較の順序を入れ替える（ABBA）、各期間の作業内容を記録する、などで緩和する。

## 8. 破壊的変更の一覧

| 層 | ID | 内容 | 状態 |
|---|---|---|---|
| core | CB-1 | INV-F1 の実現（`window_mapped.rs` の条件、`Fixed` の `OutputAdded`・`OutputRemoved` で D12 に従いフォーカスを与える）。公開 API のシグネチャは変えない | [確定]。実装は U1 の仕様テストの後（ゲート G2） |
| core | CB-2 | `core-spec.md` の追記（D102〜D104。INV-F1、D7・D21 の改訂）。D74 に Q13（列が空になる場合）を反映 | 人間 |
| core | CB-3 | ズームの型（平行移動＋拡縮、段階、拡縮関数）。公開 API の追加 | [未決]（U5。規則 5 の例外の許可が先） |
| core | CB-4 | 名前付きワークスペース（`StateView` への読み取りなど）。公開 API の追加 | [未決]（U4。同上） |
| コンポジタ | CP-1・2 | 減光、枠・影・角丸を任意に | [実装済] |
| コンポジタ | CP-3 | Motion（種別ごとの曲線・長さ）、`DimAnimator`（Focus 遷移）、`window_dim` の 1 か所化 | [暫定]（U3） |
| コンポジタ | CP-4 | キー層の照合・一覧 | [暫定]（U4） |
| コンポジタ | CP-5 | 互換 IPC と記録 | [暫定] |
| 設定 | CC-1・3 | Appearance → Tier | [実装済] |
| 設定 | CC-2 | `Layout.animation*` → Motion | [実装済] |
| 設定 | CC-5 | Keybind の同一性、`Env`・Profile | [暫定] |
| CDL リポジトリ | — | **段階 C は完了（2026-11-14）**: CDL 0.2.1（タグ `v0.2.1` = `cbae4e8`）。破壊的変更は作らず、(A) prop の位置情報（`Node::prop_span`・`prop_error`）と (B) `Literal` の再公開（列挙を `Ty::union` で宣言）だけ。T8（同名コンポーネントの取り違え）も修正。Contrast の移行も完了（`modKey`・`Layout.mode`・`Layout.animation`・`Startup.ipc`・`Workspaces.mode` が Literal。範囲外は E4004、位置は値）。詳細は `docs/cdl-requests.md` | 完了 |

## 9. ロードマップ（再編後）

ゲート付きで、止められる点を明記する。

| 段階 | 内容 | 担当 | 前提 |
|---|---|---|---|
| 0 | **実機で U2 を使う**: 減光あり/なし、`niri.cdl`（減光なし）の見た目。実機確認項目は U2 報告の C1 と U3 計画 §10 | 人間 | なし |
| 1 | **U1**: 仕様テスト（INV-F1・INV-S1・INV-R1）、`core-spec.md` の §17 追記、niri との差の確認 | 人間 | niri の実挙動の確認 |
| 2 | **A**: core の CB-1 | Claude Code | 段階 1（ゲート G2） |
| 3 | **記録と互換 IPC**（CP-5） | Claude Code | niri IPC 仕様の調査（先に調査プロンプト） |
| 4 | **H1 の計測**: 減光あり/なしを数日ずつ | 人間 | 段階 0・3 |
| C ✅完了 | **CDL の更新**（破壊的変更は結局不要で、非破壊の 2 件 + T8 修正）。CDL リポジトリで、計画 → 承認 → 実装 → Contrast 側の移行の順。Contrast は、着手前に現行の rev を固定（git tag）する | Claude Code（CDL リポジトリ） | 計画の承認。段階 5 の前に完了 |
| 5 | **U3**: Focus 遷移を先に（`Motion` スキーマの追加と `DimAnimator`）。種別ごとの曲線設定の全面配線と、旧 prop の廃止は、計測の結果を見てから | Claude Code | 段階 4 の所見。`tests/spec` の準備（人間） |
| 6 | **U4**: 名前付きワークスペース、`Env`、Profile、キー層 | Claude Code | 規則 5 の例外許可、CDL 側の確認 |
| 7 | **U5**: ズーム、ヒント減衰 | Claude Code | CB-3 の許可 |

人間の作業が前提になる箇所: `tests/spec` の追加（`crates/contrast-policy-cdl/tests/spec/` は未作成）、`core-spec.md`、`CLAUDE.md`、各 `Cargo.toml`、規則 5 の例外の許可、実機確認。

## 10. 決定の一覧と未決

### 決定（承認済み）
DU-1〜9（v0.2 のとおり。DU-1 は DU-10 で訂正）、DU-10〜20（U2 の判断。DU-13 は暫定）。

### 暫定（推奨案。承認待ち）
- DU-21: INV-T1 の 400ms は定数で、設定で上げられない（Q1）
- DU-22: `niriSpring` は INV-T1 の対象外と明記（Q2）
- DU-23: INV-M1 は 6 種別。Open・Close を Motion に含める（Q8・Q9）
- DU-24: 併存期間は `<Motion>` が旧 prop に優先し、警告（Q7）
- DU-25: 仕様テストは `contrast-policy-cdl/tests/spec/` に置く。純粋な補間は `contrast-policy` の公開モジュール（Q6）
- DU-26: リロードによる `Tier` の `l` の変更は補間せず即時（Q10）
- DU-27: Focus と矩形の動きは同時に走ってよい。実機で見て最終決定（Q5）
- DU-28: IPC は niri 互換を優先する
- DU-29: U3 は Focus 遷移を先に出す

### 未決
- niri との差: 列の挿入位置、挿入・削除時の `view_x`、削除後に動く列、広い列のスクロール、`Proportion` の再計算、DU-13 の場合（人間が実機で確認）
- 設定の型注釈: Literal 化した prop に `string` 型の式は渡せない（E4004）。手元の `~/.config` などの `.cdl` は人間が確認する
- 命令→種別の表のレビュー（`FocusMonitor`、`MoveColumnToMonitor`、`MoveToWorkspace`、`ToggleFloating`）
- niri の IPC のサブセット
- ワークスペース種別の出所（名前付きワークスペースの core への入れ方）
- 指標の定義と、比較の手順
- ヒント減衰の定義（保留。人間が書く）
- ドキュメントの整合: `DECISIONS.md` の `focusColor?` や CD3、`cdl-contract.md` の `Keybind` の列漏れ（U0 報告 §5-A）

### 採用時の突き合わせ
この仕様は、報告からの転記で、現行リポジトリと直接照合していない。採用するときは、Claude Code が `docs/ui-breaking-update.md`、`docs/cdl-contract.md`、`docs/core-spec.md` との食い違いを一覧にし、人間が確定する。