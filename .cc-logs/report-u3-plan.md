# U3 計画（Motion）

作成: 2026-11-14 / 作業ツリー: `main`（clean）/ 変更したもの: この報告ファイルだけ（コード・設定・テスト・既存ドキュメントは未変更）
ビルド・テストは実行していない。以降の「確認した」は `ファイル:行` を読んだ、「未確認」は読んでいない・実行していない、の意味。「手計算」は計算したが実行していない。

## 0. 前提の問題（先に読む）

| 項目 | 内容 |
|---|---|
| 何が問題か | 指示で最初に読むことになっていた `.cc-logs/report-u0-ui-update.md` と `.cc-logs/report-u2-impl.md` が、この作業ツリーに無い（`.cc-logs/` は `.gitignore` 対象。ディレクトリ自体が無かった。`find / -name 'report-u*'` も 0 件） |
| 再現 | `ls /home/user/Contrast/.cc-logs` → No such file |
| 影響 | (1) アニメーション現状と C1（`StyledSurface`・`snapshot_dim`）は、コードを直接読んで確かめた（§2 の `ファイル:行`）。U0・U2 報告の記述との突き合わせはしていない。(2) U0 報告 §4 の「列を変える命令の分類」を読めていない。§4 の「命令 → 動き種別」の表は、`Action` の定義（`crates/contrast-core/src/application/command.rs:84-141`）から私が作った案で、人間のレビューが要る |
| 分かっていること / いないこと | ゲートの 2 項目（git clean、`CLAUDE.md` が policy・policy-cdl・compositor を「指示で明示されたときに触ってよい」と書いている）は満たしている。報告 2 件は、人間が別の場所から用意できるなら、§2・§4 を突き合わせてほしい |

## 1. 解釈（1 行）

「U3 計画」は `.cc-logs/report-u3-plan.md` を書くところまでで、実装・テスト作成・既存ドキュメントの更新はしない。INV-T1 の上限 400ms は設定で上げられない定数、と解釈した（根拠: 設定で上げられる上限は不変条件にならない。Q1 で確認する）。

## 2. アニメーション現状の表

共通: 時刻は `Animator`（`anim.rs`）が引数で受ける（時計を読まない）。開閉演出・スクリーンショット UI・減光周りは `Instant::now()` を直接読む（§11 の P6）。

| 種類 | 駆動する場所 | 曲線 | 長さの決まり方 | 設定の入口 | 割り込まれたとき |
|---|---|---|---|---|---|
| ビューのスクロール（キー・列のフォーカス移動・挿入/削除で動く列） | core が `view_x` を変え、`Effect::Configure` で窓ごとの矩形が来る → `shell.rs:725-751`（`configure`）→ `anim.rs:217`（`retarget`）→ 毎フレーム `shell.rs:790`（`tick_animations`）→ `anim.rs:273`（`tick`）。**スクロール専用の経路は無い**（窓の移動・リサイズと同じ経路） | `Layout.animation`: `easing` は ease-out 3 次（`anim.rs:159`）、`spring` は niri のバネ（成分ごと。`anim.rs:49`、`spring.rs` の `movement()`＝減衰比 1.0・stiffness 800） | easing: `animationMs`（0〜2000。`convert.rs:382-390`、`settings.rs:157`）。spring: 物理で決まる（`settle_time`。1ms 刻みで最大 3 秒まで探す。`spring.rs:100-110`）。**`animationMs` の値は spring では長さに使われず、0 かどうかだけが効く**（`shell.rs:757-768`） | `Layout.animation` / `Layout.animationMs`（`schema.rs:96-97`、`convert.rs:373-391`）。`Settings::layout.animation(_ms)`（`settings.rs:148-150`） | 今表示している矩形から新しい行き先へ張り替える（`anim.rs:217` 以降、`retarget`）。バネは今の速度を新しいバネの初速に引き継ぐ。easing は新しい長さを最初から数え直す。引き継ぎの単体テストあり（`anim.rs` の `retargeting_*`） |
| ビューのスクロール（タッチパッドのジェスチャー） | `gestures.rs:370`（`set_pending_velocity_x`）、`gestures.rs:421`（`gesture_command` が更新中だけ `suppress`）。更新中は補間せず指に追従し、離したあとの吸着だけ上の経路で動く | 吸着は spring のときだけ指の速度を初速にする（`anim.rs` の `pending_vx`）。easing では速度は使われない | 同上 | 同上 | 同上 |
| 窓の移動・リサイズ（`MoveWindow`、幅・高さの変更、consume/expel など） | 上と同じ `Configure` → `retarget` | 同上 | 同上 | 同上 | 同上。補間の途中の矩形は `shell.rs:259`（`displayed_frame`）→ `sync_space` で Space に置かれる |
| 窓が開く | `window_anim.rs:315-`（`prepare_window_animations`）が、初めて描く窓を `opening` に入れる。オフスクリーンに描いて、透明度と拡大率を掛けて重ねる | `ease_out_expo`（`window_anim.rs:38`）。透明度 = 進み、拡大率 = 進み/2 + 0.5（`open_look`） | 固定 150ms（`window_anim.rs:33` の `OPEN_DURATION`）。**有効かどうかは `animation_ms != 0`**（`window_anim.rs:319`） | 長さ・曲線の入口は無い。有効/無効だけ `Layout.animationMs`。検証用に環境変数 `CONTRAST_DEBUG_ANIM_SLOWDOWN`（`window_anim.rs:64`） | 開く途中の窓の再開は無い（`seen` に入ったら 1 回だけ。`opening` は時間で消える） |
| 窓が閉じる | `shell.rs:594-595` が `start_closing`（`window_anim.rs:146`）。最後のフレームの絵を縮めながら消す | `ease_out_quad`（`window_anim.rs:47`）。透明度 1−進み、拡大率 (1−進み)/5 + 0.8 | 固定 150ms（`window_anim.rs:35`）。有効かどうかは `animation_ms != 0`（`shell.rs:594`） | 同上 | 割り込みなし（絵は 1 回だけ描いて固定） |
| ワークスペース切替（キー） | `shell.rs:698`（`ShowWorkspace` で `workspace_shown`）→ 新しく見える窓が縦にずれた位置から `retarget`（`anim.rs:217`、`slide`）。出ていく窓は補間せず消える（`docs/cdl-contract.md:37` 以降） | easing か spring（spring は stiffness 1000。`slid` のとき `workspace_switch()`） | 同上（`animationMs`） | 同上 | 同上。`submit` の終わりで `end_slide`（`shell.rs:651`） |
| ワークスペース切替（ジェスチャー追従・離したあとの落ち着き） | `gestures.rs:386-395`、`gestures.rs:584-588`（`WsFollow::Settling`） | spring 固定（`workspace_switch()`） | 物理（`settle_time`） | **無し**（`Layout.animation` の影響を受けない） | 指に追従。離したあとは指の速度を初速にする |
| オーバービューの開閉 | `overview.rs:110-120`（`animate_to`） | spring 固定（`movement()`） | 物理 | **無し** | 今の進みから速度つきで張り替え |
| スクリーンショット UI の開く | `screenshot_ui_shell.rs:162-164` | `ease_out_quad` | 固定 200ms（`screenshot_ui.rs:12`） | **無し** | — |
| フォーカスの変化（枠） | 枠は `window_border` で毎フレーム即時に決まる（補間なし。`render/mod.rs` と `window_anim.rs` の `bands`） | — | — | — | — |
| **減光の変化** | `render/dim.rs:12`（`window_dim`）が、毎フレーム `focus_id == Some(id)` と全画面かどうかから乗数を返す。呼び出しは 2 か所（`window_anim.rs:358`、`render/mod.rs:297`）。乗数が変わると `DimIds::styled_id`（`render/rounded.rs:159`）が新しい `Id` を返して再描画させる | — | — | — | **現状は即時**（時間による補間は無い）。したがって INV-M1 の「フォーカス＝明度遷移」は**新規の動き**になる |

補足（確認した）:
- 窓の矩形の補間（スクロール・移動・リサイズ・ワークスペース）は、種類を区別せず 1 本の `Animator`・1 つの曲線設定で動く。
- `Settings::builtin` の既定は `animation_ms = 0`（補間なし。`docs/cdl-contract.md:23`）。`config/appearance.cdl:35-37` と `examples/niri.cdl:29` は spring ＋ 150。
- 開閉演出の曲線（expo・quad）は、`Motion::Easing`（ease-out 3 次）とは別物で、設定から選べない。

## 3. Motion のスキーマ案

### 3.1 要素

```
<Motion>                               // Contrast の子。一意。省略 = 動きなし（今の組み込み既定と同じ）
  <Scroll    curve? duration? />       // 平行移動（スクロール・窓の移動・列の並べ替え）
  <Resize    curve? duration? />       // 大きさの変化
  <Workspace curve? duration? />       // 縦移動
  <Focus     curve? duration? />       // 明度遷移（減光の変化）
  <Open      curve? duration? />       // 窓が開く（見た目は透明度＋拡大で固定）
  <Close     curve? duration? />       // 窓が閉じる（同上）
</Motion>
```

- 種別は要素名で固定（INV-M1 の「対応は固定」と同じ形。`Tiers` の `FocusTier`・`Tier` と同じ作り）。種別ごとに高々 1 つ。省略した種別は動きなし（即時）。
- `curve?: string`、`duration?: Duration`。`duration` は CDL の `Ty::Duration`（`cdl-sema/src/types.rs:15`、`cdl-host/src/convert.rs:84-95` の `Millis`）。Contrast のスキーマは今 `Duration` を使っていないので、**CDL の Duration リテラルの書式は未確認**（Q3）。書式が使いにくければ `ms?: Number`（今の `animationMs` と同じ流儀）に替える。
- `Settings` に `motion: MotionSettings`（種別ごとに `Option<MotionSpec { curve, duration }>`）。`LayoutSettings.animation` / `animation_ms` と `AnimationKind` は U3 の最終段階で消す。

### 3.2 曲線の固定集合

| 名前 | 式 | 出所 |
|---|---|---|
| `easeOutCubic`（既定） | `1 − (1−t)^3` | 既存（`anim.rs:159`） |
| `easeOutQuad` | `1 − (1−t)^2` | 既存（`window_anim.rs:47`。今の Close） |
| `easeOutExpo` | `1 − 2^(−10t)` | 既存（`window_anim.rs:38`。今の Open） |
| `easeInOutCubic` | 前半 `4t^3`、後半 `1 − (−2t+2)^3 / 2` | **新規**（slow-in / slow-out の素直な形） |
| `niriSpring` | niri のバネ | §5。Scroll・Resize・Workspace・Open・Close でだけ使え、長さを持たない |

easing 系 4 本は、`f(0) = 0`、`f(1) = 1`、単調非減少。これらの性質は §8 の性質テストで検査する。`linear` は置かない（設計原則「減速で止まる」に反するため）。

### 3.3 長さと既定値

| 種別 | 既定の長さ（仮置き） | 既定の曲線 | 根拠 |
|---|---|---|---|
| Scroll | 150ms | easeOutCubic | 今の `config/theme.cdl:33`（150ms）と同じ |
| Resize | 150ms | easeOutCubic | 同上 |
| Workspace | 150ms | easeOutCubic | 同上（今は Scroll と同じ値。別にしたければ人間が決める） |
| Focus | 120ms | easeOutQuad | **新規・仮置き**（DU-18 と同じ扱い。実機で決める） |
| Open | 150ms | easeOutExpo | 今の `window_anim.rs:33` と同じ |
| Close | 150ms | easeOutQuad | 今の `window_anim.rs:35` と同じ |

- `<Motion>` ごと省略したら、全種別が即時（今の組み込み既定 `animation_ms = 0` と同じ）。種別の要素を書いたら、省略した prop は上の既定になる。
- 上限は定数 `MAX_MOTION_MS = 400`（INV-T1）。`0` は「その種別は動きなし」。

### 3.4 診断コードの案

CT065 の次の空き `CT07x` を使う（`docs/cdl-contract.md:195-200` の表を読んで、CT066〜CT069 の使用は確認していないので、実装時に空きを確かめる）。

| コード（案） | 条件 | 位置 | 件数 |
|---|---|---|---|
| CT070 | INV-T1: easing 系の `duration` が 0 未満・有限でない・400ms 超 | 超えた種別の要素 | 超えた要素ごとに 1 件。複数あれば全部（`validate_tiers` と同じく、見つけた順に全部返す） |
| CT071 | 未知の曲線名 | その要素 | 要素ごとに 1 件 |
| CT072 | `niriSpring` と `duration` を同時に指定、または `Focus` に `niriSpring` | その要素 | 要素ごとに 1 件 |
| CT073 | 同じ種別の要素が 2 つ以上 | 2 つ目以降の要素 | 余分な要素ごとに 1 件 |
| （既存） | 未知の種別 = 未知の子要素 | `<Motion>` に置けない子 | 既存の CT002（`convert.rs:248` の `Tiers` と同じ扱い）。CDL 自身が先に拒否する場合の E コードは**未確認** |

- 値の診断は、CDL の制約で要素単位の位置しか取れない（`docs/cdl-contract.md` の「値木の診断は要素単位の位置」）。props の位置は取れない。
- エラーが 1 件でもあれば、リロードは旧設定を保持（既存の規則）。
- 各 `Scroll`〜`Close` の一意性を CDL の `unique_child` で守れるかは未確認（`Tiers` は 1 名しか指定していない。`schema.rs:68-72`）。守れなければ CT073 で守る。

### 3.5 `Settings` とポートの変更案

| 対象 | 変更 |
|---|---|
| `contrast-policy/src/settings.rs` | `MotionKind`（6 種）、`MotionCurve`（5 種）、`MotionSpec`、`MotionSettings`、`MAX_MOTION_MS = 400` を追加。最終段階で `AnimationKind`・`LayoutSettings.animation(_ms)`・`MAX_ANIMATION_MS` を削除 |
| `contrast-policy/src/port.rs` | `Policy` に `fn motion(&self) -> MotionSettings`（既定は全種別動きなし）。`layout_settings()` の `animation*` は最終段階で消える |
| 純粋な補間 | `contrast-policy` に新しい `motion` モジュール（`pub`）。曲線 `ease(curve, t)`、進行度 `progress(elapsed, duration)`、減光の補間 `dim_at(from, to, p)`、命令 → 種別 `kind_of(&Command)`。GL・時計に依存しない。前例: `tier.rs` の `dim_factor`（ポリシー側の純粋関数）。置き場所の理由は §8 |
| `contrast-policy-cdl` | `schema.rs` に `Motion` と 6 種の子を追加。`convert.rs` に `read_motion`（`read_tiers` と同じ形）。CT07x |
| `contrast-compositor` | `anim.rs` の内部型 `Motion`（`anim.rs:17`。`Easing`/`Spring`）は、CDL の `Motion` と名前が衝突する。実装時に改名（例: `Curve` は既に別の内部型があるので `Drive`）が要る |

## 4. INV-M1 の対象操作と対応表

### 4.1 選択肢

| 案 | 内容 | 長所 | 短所 |
|---|---|---|---|
| X（最小） | INV-M1 は Scroll・Focus・Workspace の 3 種だけ。移動・リサイズ・開閉は旧来のまま（固定または Scroll と同じ） | 変更が小さい | 窓の移動・リサイズ・consume/expel が Scroll と同じ曲線になり、「操作ごとに別の動き」にならない。開閉の 150ms 固定が残り、`niri.cdl` の基準線（開閉あり）を Motion で表せない |
| **Y（推奨）** | 6 種（Scroll・Resize・Workspace・Focus・Open・Close）。対応は命令単位で固定（§4.2） | 現状の全アニメーション（スクロール・移動・リサイズ・ワークスペース・開閉）が Motion に載る。`niri.cdl` の今の挙動を `niriSpring` で再現できる | 種別が 3 → 6。INV-M1 の本文（スクロール・フォーカス・ワークスペース）を人間が書き換える必要がある |
| Z（全部） | Y に加え、オーバービュー・ジェスチャー追従・スクリーンショット UI も Motion に載せる | 設定の入口が 1 つになる | ジェスチャー追従は指の動きそのもの（時間で決まらない）。オーバービューは別の機能。範囲が広がり、U3 を越える |

推奨は Y。オーバービュー・ワークスペースのジェスチャー追従・スクリーンショット UI（§2 の「設定の入口なし」の 3 つ）は、**U3 の対象外と明記**する（物理で動く／指に追従する／システム UI。INV-T1 の 400ms にも反しない: スクリーンショット UI は 200ms、バネは物理）。

### 4.2 命令 → 種別（案）。命令単位で 1 つに決める

**規則**: `submit(command)`（`shell.rs:645`）が、その命令で起きる矩形の変化の種別を 1 つ決め、その命令の `Configure` 全部がその種別の曲線を使う。窓ごと・差分ごとには決めない。

理由: 1 つの命令の中で別の種類の動きが重ならない。例えば「列の幅を変える」と、その後ろの列の平行移動は、まとめて Resize の曲線で動く。

| 種別 | 命令（`Action` と `Command`） |
|---|---|
| Scroll | `FocusDirection`、`FocusColumnFirst/Last`、`CenterColumn`、`MoveWindow`、`MoveColumnToFirst/Last`、`ConsumeWindowIntoColumn`、`ExpelWindowFromColumn`、`ConsumeOrExpelWindowLeft/Right`、`ViewGestureBegin/Update/End`、クライアント起因の `WindowMapped`・`WindowUnmapped` が起こす隣の列の動き |
| Resize | `SwitchPresetColumnWidth`、`MaximizeColumn`、`SetColumnWidth`、`SetWindowHeight`、`SwitchPresetWindowHeight`、`ResetWindowHeight`、`ToggleFloating`、`ToggleFullscreen`（全画面からの復帰） |
| Workspace | `SwitchWorkspace`、`MoveToWorkspace`、`MoveColumnToWorkspace`、`MoveColumnToWorkspaceNoFocus`、`MoveWorkspaceUp/Down`、`FocusMonitor`、`MoveColumnToMonitor` |
| Focus | 命令ではなく**状態**（フォーカス窓が変わったこと）。どの命令でも、フォーカスが変われば起きる |
| Open / Close | 窓そのものの出現・消滅（命令ではない） |
| 補間なし（既存のまま） | 出力の大きさの変更、`OutputChanged`/`OutputInsetsChanged`、全画面にするとき（`shell.rs:731` の `animate = state != Fullscreen`） |

- `Action` は `#[non_exhaustive]`（`command.rs:83`）。コンポジタ側の対応では、未知の variant は Scroll にする（ワイルドカードの腕。`CLAUDE.md` 規則 4 の禁止リストに無い）。
- 上の表は §0 のとおり U0 報告 §4 を読まずに作った案。`MoveToWorkspace` の窓が動く先の見た目、`ToggleFloating` の扱いは人間の判断（Q4）。
- `kind_of(&Command) -> Option<MotionKind>` は、`contrast-core` の型だけに依存する純粋関数にして `contrast-policy` に置く（§3.5）。これで INV-M1 の対応表を GL なしの例示テストで検査できる。

### 4.3 同時に複数の種類が動くときの規則

1. **矩形に動きが掛かる種別（Scroll・Resize・Workspace）は同時に 1 つ。** 命令単位で 1 つに決まるので、1 つの命令の中では重ならない。別の命令で動き中の窓に新しい命令が来たら、新しい種別・曲線・長さで張り替える（下の割り込み規則）。重ねない。
2. **Focus は別のチャンネル（明度）なので、矩形の動きと同時に走ってよい。** これが INV-M1「同時に別の種類の動きを重ねない」への**明示した例外**。`FocusDirection`（フォーカスの変化＋スクロール）や `SwitchWorkspace`（フォーカス移動＋縦移動）では、必ず同時に起きるため。
   - 代案（厳格案）: 矩形が動いている間は Focus を即時にする。問題: 同じ `FocusDirection` でも、スクロールが起きるときは減光が即時、起きないときは遷移、と見た目が不揃いになる。一貫性（Heer & Robertson 2007）に反するので推奨しない。
   - 推奨は例外ありだが、「動きを重ねない」原則とぶつかるので、実機で確かめ（§10 の 2）、Q5 で人間が決める。
3. **Open・Close は、その窓自身の出現・消滅の動き。** 同時に起きる隣の列の Scroll とは重なるが、窓が違う。窓ごとに同時に走る動きは「矩形の動き 1 つ」＋「Focus」＋「Open/Close」の最大 3 つ。
4. **割り込み規則（全種別共通）:**
   - 新しい目標は、動き中のものを置き換える（キューに積まない）。始まりは今表示している値（矩形なら `displayed`、減光なら表示中の乗数）。連続性は保つ（現状どおり。`anim.rs` の `retargeting_*`）。
   - 長さは、新しい種別の長さを最初から数える（距離に比例させない。簡単さを優先。実機で変えたければ §10 で）。
   - `niriSpring` は今の速度を引き継ぐ（現状どおり）。easing は速度を引き継がない。ジェスチャーの終わりの速度（`pending_vx`）が使われるのは `niriSpring` のときだけ。
   - 隠れた窓・閉じた窓は忘れる（現状どおり。`retain`・`forget`）。

### 4.4 開閉演出 150ms 固定の扱い

| 案 | 内容 |
|---|---|
| **A（推奨）: Motion に含める** | `Open`・`Close` を種別にする。長さ（上限 400ms）と曲線は設定でき、**見た目（透明度＋拡大率の式）は固定**。既定値は今と同じ（150ms、expo / quad）なので、`<Motion>` に `<Open />` と `<Close />` だけ書けば現状と同じ。有効/無効が `animationMs` と結びついている現状（P2）を解消できる |
| B: 固定のまま例外 | `window_anim.rs:33,35` を残す。有効/無効は `Motion` が 1 種別でもあるか、などの別規則が要る。`Layout.animationMs` を廃止すると、有効/無効の入口が消えるので結局何かが要る |

## 5. niri 互換の spring の置き方

### 5.1 比較

| 項目 | (A) `niriSpring` を Motion の曲線に置く | (B) `Layout.animation` を残す | (C) 互換用の別要素 |
|---|---|---|---|
| 形 | `<Scroll curve="niriSpring" />` | `Layout.animation="spring"` はそのまま。Motion は別系統 | `<Motion>` とは別に `<NiriSpring />` などを置く |
| CC-2（旧 prop の廃止）との整合 | ○ `Layout.animation*` を全部消せる | ✗ 残るので CC-2 に反する | ○ `Layout` からは消せる |
| 設定の入口 | 1 つ（Motion） | 2 つ（`Layout` と Motion）。同時に書いたときの優先順位が要る | 2 つ（Motion と互換用要素）。同上 |
| `examples/niri.cdl`（基準線）への影響 | `animation="spring" animationMs={150}` を `<Motion>` の 5 要素（Scroll・Resize・Workspace・Open・Close）に書き換える。今の挙動（移動・リサイズ・ワークスペースは spring、開閉は 150ms）を保てる | 変更なし | 1 要素の書き換え。ただし開閉の扱いが別に要る |
| INV-T1 の検査の複雑さ | 低〜中。「easing 系なら `duration ≤ 400`、`niriSpring` は長さを持たず対象外」の 1 分岐＋CT072 | 低。ただし spring が Motion の外にあるため、INV-T1 が守る範囲が曖昧になる | 低。Motion に spring が入らないので INV-T1 は単純。ただし Motion と互換用要素が重なったときの規則が要る |
| DU-5（spring は Motion の対象外）との整合 | 「Motion の長さの検査の対象外」と明記する。曲線の集合には入るが、INV-T1 の対象ではない、という言い方 | ○ | ○ |
| 種別との相性 | ○ バネの係数が種別に紐づく（Scroll・Resize は stiffness 800、Workspace は 1000。`spring.rs:41-51`）。今の実装と同じ | — | △ 種別ごとの係数を別要素で持つことになる |

### 5.2 推奨: (A)

- `niriSpring` は互換専用の曲線で、「設定で調整できるバネ」ではない（係数は niri の既定で固定）。名前に `niri` を入れて、互換専用であることを名前で示す。
- 使える種別は Scroll・Resize・Workspace・Open・Close。**Focus には使えない**（CT072。明度が 0〜1 を行き過ぎる）。
- **注意（INV-T1 との関係）**: niri のバネの収束時間は 400ms を超えうる。手計算（臨界減衰、`x = (1+u)e^{−u}`、`u = √k · t`、収束の許容は `epsilon = 0.0001`。実行していない）で、stiffness 800 では移動距離 1px のときの収束が約 0.42 秒、100px で約 0.6 秒、1000px で約 0.7 秒。視覚的にはもっと早く止まって見えるが、コードの `settle_time`（`spring.rs:102`）はこの値を使う。つまり `niriSpring` を INV-T1 の対象に入れると、niri 互換の基準線が自分で 400ms を超える。**対象外と明記するのが正直**（上限は easing 系の長さの上限、と INV-T1 を読み替える）。人間が INV-T1 の文言を決める（Q2）。
- 旧 prop の変換: §7.2。

## 6. フォーカスの明度遷移

### 6.1 補間

```
elapsed(t) ─ progress(elapsed, duration) ─ ease(curve, ·) ─ p ─ k = k_from + (k_to − k_from)·p
```

- `k` は今の乗数（`render/dim.rs:12` の `window_dim`。1.0 か `unfocused_dim`）。**補間は `k` そのもの（線形）**。
- 代案: OKLCH の `l` を補間して、各時刻で `dim_factor`（`tier.rs:266`）を通す。今の段階数（`l = 1.0` と `0.8`）では、手計算で、中間（`l = 0.9`）の乗数が約 0.866、`k` の線形補間の中間が約 0.869 で、差は約 0.003（実行していない）。見分けがつかないので、`l` を持ち回らずに済む `k` の線形補間を推奨する。Tier の段階が増える将来は見直す。
- 純粋関数 `dim_at(from, to, p) -> f32`: `p ∈ [0,1]` で、端点は厳密（`p = 0` で `from`、`p = 1` で `to`）、結果は `[min, max]` の中。時計に依存しない。

### 6.2 状態と配線

- 窓ごとに `{from, to, start, displayed}` を持つ `DimAnimator`（コンポジタ側。`WindowId` キー）を新設する。**フレームごとに 1 か所で `displayed` を決める**（`prepare_window_animations`。`drm.rs:388`、`winit.rs:218` の描画前）。
- 今は `window_dim` が 2 か所で独立に呼ばれる（`window_anim.rs:358`、`render/mod.rs:297`）。補間中に両者が別の値を返すと、画面と開閉の絵がずれるので、**両方とも `displayed` を読む**ように変える。
- 履歴が無い窓（新しく開いた窓・ワークスペースの切替で新しく見える窓）は、補間せず `to` から始める（`Animator` の `retarget` と同じ。新しい窓は明るく始まり、直前までフォーカスだった窓だけが 1.0 → `k` へ動く）。隠れた窓の履歴は捨てる（`retain` と同じ）。
- 終わったら `displayed` を `to` に厳密に合わせる（`Animator::tick` が終わりで丸めずに `to` を返すのと同じ）。

### 6.3 2 フレームのダメージ（C1 の `CommitCounter` / `Id`）との関係

- `StyledSurface` は `current_commit` を中身に任せており、乗数だけが変わっても Smithay 0.7.0 の `damage/mod.rs:563` は再描画しない（`render/rounded.rs:9-10` の説明）。そこで `DimIds::styled_id`（`render/rounded.rs:159`）が、乗数のビット列が変わったとき新しい `Id` を返す。
- 補間中は毎フレーム乗数が変わるので、毎フレーム新しい `Id` になり、その窓の面全体が毎フレーム再描画される（前のフレームの `Id` の領域と、新しい `Id` の領域の 2 つがダメージになる）。これは意図どおりで、補間が終われば（`displayed == to`）同じ `Id` が返り、ダメージは止まる。`Id::new()` の増加は、`end_frame` で使われなかった項目が捨てられるので、リークしない（`rounded.rs` の `entries.retain`）。
- 補間中は、次のフレームも描く必要がある。`tick_animations` の戻り値（`shell.rs:790-`）と `window_anims.is_running()` に、`DimAnimator` の実行中を足す。**呼び出し側（`drm.rs:359`、`winit.rs:209`）が戻り値をどう使って再描画を予約しているかは読み切っていない（未確認）**。実装前に確認が要る。
- 乗数が 1.0 に戻った最後のフレームで、`dimmed` が偽になり、要素が `StyledSurface` から素の面に戻る（今と同じ。`Id` が変わるので、もう 1 フレーム分のダメージ）。
- 描画コスト: 補間の間だけ窓全面の再描画。上限 400ms。DRM での見た目・コストは実機で確かめる（§10 の 6）。

### 6.4 閉じる窓の減光（DU-17）

- 閉じる窓の絵は `Snapshot.dim`（最後のフレームの乗数）で描く（`window_anim.rs` の `draw` → `snapshot_dim(Animation, s.dim)`）。**`s.dim` に入れる値を、`window_dim` の直接の値から `displayed`（画面で見えていた値）に変える**。こうすれば、補間の途中で閉じる窓も、見えていたとおりの乗数で止まる（一瞬明るくならない）。
- 閉じる窓の絵は静止画なので、Focus の補間は掛けない。閉じたあとにフォーカスを受ける隣の窓は、普通に `k → 1.0` へ動く。

### 6.5 `screenshot-window`（常に `dim = 1.0`）

- `snapshot_dim(ScreenshotWindow, _) = 1.0`（`render/dim.rs:30-35`）は、入力の乗数を使わない。補間の途中の乗数が入っても、結果は変わらない。この不変条件は保たれる（`dim.rs` の既存テスト `screenshot_window_never_dims`）。
- 開く窓の絵（`Animation` の用途）も `displayed` で描く。

### 6.6 その他

- 全画面の窓は `window_dim` が 1.0（`dim.rs:12`）。全画面に入る・出るときの乗数の変化も、同じ規則で補間される（全画面の窓は通常フォーカスされているので、実際の差は小さい）。
- 設定のリロードで `Tier` の `l`（目標の `k`）が変わったとき、同じ規則で補間される。即時にする方が自然かもしれない（人間の判断。未決の小さな点）。
- `Tier` が 0 個（DU-20）なら `unfocused_dim() = 1.0` で、補間する値が無い。`niri.cdl` は変化なし。

## 7. 移行計画

### 7.1 書き換えが要るファイル

| 区分 | ファイル | 内容 |
|---|---|---|
| `config/` | `config/appearance.cdl:5,35-37`（`ANIMATION_MS` の import と `animation="spring"`・`animationMs`） | `<Motion>` に置き換え。`<Layout>` から `animation`・`animationMs` を削除 |
| | `config/theme.cdl:33`（`ANIMATION_MS = 150`） | 種別ごとの定数へ置き換え（または削除） |
| | `config/unsupported.cdl:15`（コメント） | 「animations（既定）は移さない」を実態に合わせる |
| `examples/` | `examples/niri.cdl:13`（import 一覧）、`:29`（`animation="spring" animationMs={150}`）、`:199`（コメント） | `<Motion>`（`niriSpring` 5 要素）に。`Tier` は置かない（DU-20）ので減光は変わらない |
| policy-cdl のテスト | `crates/contrast-policy-cdl/tests/impl_config.rs:52-53`（`animation_ms == 150`、`animation == Spring`） | `settings.motion` の検査に |
| | `crates/contrast-policy-cdl/tests/impl_look.rs:254-296`（`animation_ms_defaults_to_off_and_accepts_the_edges`、`bad_animation_ms_is_ct003`、`animation_kind_defaults_to_easing_and_accepts_spring`）、`:318`（リロードのテストの `animation_ms == 150`） | 旧 prop を検査するので、最終段階で削除し、Motion 版へ。**これらは `impl_*`（自分で足したテスト）なので、規則 1 に触れない** |
| | `crates/contrast-policy-cdl/tests/impl_niri.rs` | `niri.cdl` を読んで検査する。`animation` の検査の記述は見つからなかった（grep）が、全文は読んでいない（未確認）。最終段階で通ることを確かめる |
| policy | `crates/contrast-policy/src/settings.rs:120-157`（`AnimationKind`、`LayoutSettings.animation_ms`・`animation`、`MAX_ANIMATION_MS`）、`crates/contrast-policy/src/lib.rs:32`（`AnimationKind` の再エクスポート） | 最終段階で削除し、`MotionSettings` 等を追加（追加は段階 (a)） |
| | `#[cfg(test)]` | `contrast-policy` の `src` で `animation` を含むのは `settings.rs` と `lib.rs` だけ（grep）。`#[cfg(test)]` に animation を検査するテストは見つからなかった |
| compositor | `shell.rs:594`（`animation_ms != 0` で閉じる演出の有効/無効）、`shell.rs:757-768`（`animation_motion`）、`window_anim.rs:319`（開く演出の有効/無効）、`anim.rs`（内部型 `Motion` の改名と、種別ごとの曲線の受け取り）、`state.rs:167`（コメント） | 段階 (b)(c)(d) |
| ドキュメント | `docs/cdl-contract.md:23,37-`（`Layout` の props と `animationMs` の説明）、`docs/config-migration.md:67`、`docs/niri-parity.md:21,117`、`docs/ui-breaking-update.md`（§4・§5・§6・§7 の更新） | 段階 (d) で。`docs/DECISIONS.md:21-31` の `Animation` は過去の設計の記録なので変えない |

保護されたパス（人間が編集）に触れる必要は、現時点では無い（各 `Cargo.toml` の変更は不要。`Duration` 型は `cdl-host` に既にある）。ただし §8 のとおり、compositor の仕様テストで proptest を使うなら `contrast-compositor/Cargo.toml` の `dev-dependencies` 追加が要り、これは人間の作業（規則 1・3）。

### 7.2 旧 prop → 新 prop の対応

| 旧 | 新 |
|---|---|
| `animation="spring"` かつ `animationMs > 0` | `<Motion><Scroll curve="niriSpring"/><Resize curve="niriSpring"/><Workspace curve="niriSpring"/><Open/><Close/></Motion>`（Open・Close は既定の 150ms。今と同じ） |
| `animation="easing"`（既定）かつ `animationMs = N`（1〜400） | Scroll・Resize・Workspace の `duration=N`、`curve="easeOutCubic"`。Open・Close は既定 |
| `animation="easing"` かつ `animationMs = N`（401〜2000） | **破壊的**: 旧設定は有効だったが、INV-T1 で CT070。400 以下に直す |
| `animationMs = 0`（または省略） | `<Motion>` を書かない。または各 `duration={0ms}` |
| （無し。開閉は `animationMs ≠ 0` に連動して 150ms） | `<Open/>`・`<Close/>`。`<Motion>` に書かなければ開閉の演出は無くなる（今の `animationMs = 0` と同じ） |
| （無し） | `<Focus/>`（新規。書かなければ減光は即時のまま） |

- 旧 prop は、スキーマから消した時点で CDL が「`<Layout>` に props `…` はない」（E4003 相当。`docs/cdl-contract.md:75` の `focusColor` と同じ扱い）で拒否する。リロードは旧設定を保持する（DU-8）。
- `MAX_ANIMATION_MS = 2000`（`settings.rs:157`）は削除し、`MAX_MOTION_MS = 400` に置き換える。`niriSpring` には長さの上限が無い（物理で決まる。`settle_time` が 3 秒で打ち切る）。上限は 1 つだけ（easing 系の `duration`）に整理される。

## 8. U3 仕様テストの候補（人間が書く）

### 8.1 置き場所の制約（確認した）

- `crates/contrast-compositor/tests/spec/` と `crates/contrast-policy-cdl/tests/spec/` は、DU-14 と `CLAUDE.md` 規則 1 が指定しているが、**どちらのディレクトリも現状は存在しない**（`ls` で確認。`crates/contrast-compositor` には `tests/` 自体が無い）。人間が作る。
- `proptest` は `contrast-policy-cdl` の `dev-dependencies` にだけある（`Cargo.toml:15-16`）。`contrast-compositor` と `contrast-policy` には無い。
- したがって、プロパティテストは `contrast-policy-cdl/tests/spec/` に置くのが自然。純粋な補間・種別の関数を `contrast-policy` の `pub` なモジュールに置く理由は、ここにある（`contrast-policy-cdl` から `contrast_policy::motion::*` を呼べる）。compositor の `mod anim;` は非公開（`lib.rs:6`）なので、統合テストからは届かない。
- compositor の `tests/spec/` に proptest が要るなら、`Cargo.toml` を人間が編集する（Q6）。例示テストだけなら不要。
- `contrast-compositor` の `Cargo.toml` に `autotests = false` 等があるかは未確認。

### 8.2 候補

| ID | 検査する内容 | 種類 | 置き場所 |
|---|---|---|---|
| T-T1-a | INV-T1 の境界: `duration` が 0 / 1 / 399 / 400 は通り、401 / 2000 は CT070。負・有限でない値も CT070 | 例示（境界値） | policy-cdl |
| T-T1-b | 診断の**位置と件数**: 超えた種別の要素ごとに 1 件、位置はその要素の `file:line:col`。2 つ超えれば 2 件。超えていない要素には出ない | 例示＋プロパティ | policy-cdl |
| T-T1-c | `niriSpring` は長さの検査の対象外（`duration` なしで通る）。`niriSpring` と `duration` の併記は CT072。`Focus` に `niriSpring` は CT072 | 例示 | policy-cdl |
| T-T1-d | 未知の曲線名は CT071、同種別の重複は CT073、`<Motion>` に未知の子は拒否。エラーがあればリロードは旧設定を保持 | 例示 | policy-cdl |
| T-T1-e | 旧 prop（`Layout.animation`・`animationMs`）は最終段階以降で拒否される | 例示（最終段階の後） | policy-cdl |
| T-M1-a | INV-M1 の対応表: `kind_of` を、`Action` の既知の全 variant に対して例示で検査（§4.2 の表そのもの）。未知の variant は Scroll | 例示（表駆動） | compositor または policy-cdl |
| T-M1-b | Scroll の補間は大きさを保つ（始まりと行き先が同じ大きさなら、途中の矩形も同じ大きさ）。Workspace は x が不変で y だけ動く | プロパティ（矩形の生成器） | policy-cdl（`lerp_rect` 相当の純粋関数を `motion` に置く場合）。`lerp_rect` は今 `anim.rs:171` にあり非公開 |
| T-M1-c | Focus の補間は矩形に触れない（`dim_at` は `f32` だけを返す）。型で保証される部分は検査不要 | — | — |
| T-P-a | 純粋性: `ease`・`progress`・`dim_at`・`kind_of` は、同じ入力で同じ出力（ビット列まで）。時計を読まない（引数に `elapsed` を取る） | プロパティ | policy-cdl |
| T-P-b | 性質: 長さが上限以下（有効な設定で全種別の `duration ≤ 400ms`）、`progress(0) = 0`、`progress(≥duration) = 1`、`t1 ≤ t2` なら `p(t1) ≤ p(t2)`（easing 4 本。`niriSpring` は別の関数なので除く） | プロパティ | policy-cdl |
| T-P-c | `dim_at(from, to, p)`: `p = 0` で `from`、`p = 1` で `to`、結果は `[min(from,to), max(from,to)]`、`p` について単調 | プロパティ | policy-cdl |
| T-P-d | `snapshot_dim(ScreenshotWindow, k) = 1.0` が任意の `k`（補間の途中の値を含む）で成り立つ | 既存（`dim.rs`）を保つ | 既存の `src` 内テスト |

### 8.3 生成器と参照モデル

- **CDL の生成器**: 構造化モデル（種別の部分集合、各 `curve`・`duration`、重複、未知名）から `<Motion>` のソース文字列を作る。`duration` は境界（0・399・400・401）に偏らせる。`proptest`。
- **参照モデル**: 実装と独立な `expected_diagnostics(model) -> Vec<(code, 種別の要素の位置)>`（仕様の規則だけから書く。実装の関数を呼ばない）。件数・コード・位置を比べる。
- **補間の参照モデル**: 曲線は閉じた式なので、式そのものを別に書いて比べる（`easeOutCubic`・`quad`・`expo`・`easeInOutCubic`）。`niriSpring` は、既存のテストが数値積分の参照モデルで検査している（`spring.rs` の `integrate`）ので、新しい参照は要らない。
- 前例: `crates/contrast-policy-cdl/tests/impl_tier_validate.rs` の流儀（読んでいない。未確認）。
- GL が要る検査（減光の画素）は `render/dim_tests.rs` にあり、**描画ノードが無い環境では `SKIPPED` で何も検証しない**（`dim_tests.rs:3-4`）。U3 の合格判定に使うなら、SKIPPED の数を数える。

## 9. U3 の段階案

| 段階 | 内容 | 完了条件（検証の方法） | 止められる点 |
|---|---|---|---|
| (a) 足すだけ | `contrast-policy`: `MotionSettings` 等・`MAX_MOTION_MS`・純粋な `motion` モジュール（`ease`・`progress`・`dim_at`・`kind_of`）。`contrast-policy-cdl`: スキーマ・`read_motion`・CT07x。`docs/cdl-contract.md` に追記。旧 prop は**触らない**。`Motion` と `Layout.animation*` が併存する（`Motion` があれば旧 prop は無視、のように優先順位を決める。Q7） | `scripts/verify.sh` は core 専用なので使わない（`CLAUDE.md`）。`cargo clippy -p contrast-policy -p contrast-policy-cdl --all-targets -- -D warnings`、`cargo test -p contrast-policy -p contrast-policy-cdl`（`-j 2`、`systemd-run` つき）。T-T1・T-P の仕様テスト（人間が先に書く）と、自分の `impl_motion.rs`・`#[cfg(test)]` | ここで止めても無害（何も `Motion` を使わない） |
| (b) 配線（併存） | compositor: `submit` で命令から種別を決める。`Animator` が種別ごとの曲線・長さを受け取る（旧 prop だけの設定では今までと同じ挙動）。`Open`・`Close` の長さ・曲線を設定から読む。内部型 `Motion` の改名 | `cargo clippy -p contrast-compositor ...`、`cargo test -p contrast-compositor`（`anim.rs` の既存テストが通る）。旧設定（`animation="spring"` のみ）で挙動が変わらないことを単体テストで確かめる。T-M1 の仕様テスト | 旧と新が併存したままで止められる |
| (c) 明度遷移 | `DimAnimator`、`window_dim` の 2 か所の呼び出しを `displayed` に、`Snapshot.dim` を `displayed` に、`tick_animations`・`window_anims.is_running()` に実行中を足す | `dim_at` の仕様テスト（T-P-c）、`dim_tests.rs`（GL。SKIPPED の数を確認）、自分の単体テスト。DRM の見た目は実機（§10） | `<Focus/>` を書かなければ挙動は変わらないので止められる |
| (d) 旧 prop を壊す | `Layout.animation`・`animationMs`・`AnimationKind`・`MAX_ANIMATION_MS` を削除。`config/`・`examples/`・policy-cdl のテスト・ドキュメントを書き換え。旧 prop を書くと E4003 相当で拒否（リロードは旧設定を保持） | `cargo test -p contrast-policy-cdl`（`impl_niri.rs` が `examples/niri.cdl` を読めること、`config/` がロードできること）、clippy。T-T1-e | **ここだけ元に戻しにくい**ので、(a)〜(c) を実機で確認してから着手する。(d) の前で止める |

- U3 は core に触らない。U2 の段階 A（core の CB-1。INV-F1 の追記と `tests/spec` が先）は未実施のままだが、U3 とは独立（`docs/ui-breaking-update.md:100`）。
- コミットの形式は `CLAUDE.md` のとおり `policy: ...`・`cdl: ...`・`compositor: ...`。push しない。
- 規則 4（`unwrap`・`expect`・`panic!`・添字アクセス・素の算術の禁止、`#[allow]` で黙らせない）は、新しく足すコードに適用する。補間の算術は `f64` と飽和・チェック付きの演算で書く。

## 10. 実機で人間が確かめる項目（1 回にまとめる）

`scripts/contrast-realhw.sh`（`docs/ui-breaking-update.md:116`）を使う。`CONTRAST_DEBUG_ANIM_SLOWDOWN`（`window_anim.rs:64`。開閉だけを遅くする）が使える。

1. **Scroll・Resize の動き**: キー連打（`focus-column-right` の連打）で動き中に割り込んだとき、窓が飛ばず、引っかからず、止まるときに減速するか。easing 既定（150ms）と `niriSpring` の見た目が、今の `niri.cdl` と同じか（基準線）。
2. **Focus の明度遷移**: 目立ちすぎない・遅れて感じない長さか（既定 120ms は仮置き）。**スクロールを伴うフォーカス移動で、Scroll と Focus が同時に動く見た目が許容できるか**（§4.3 の 2。許容できなければ厳格案を検討）。
3. **ワークスペース切替**: 縦移動と、フォーカス窓の明度遷移が同時に走る見た目。
4. **Open・Close**: 設定した長さ・曲線で動くか。閉じる窓が、補間の途中でも直前の明るさのまま縮むか（一瞬明るくならないか）。
5. **DRM での描画**: 明度遷移中に、ちらつき・テアリング・フレーム落ちが無いか（窓全面を毎フレーム再描画する経路。§6.3）。
6. **`screenshot-window`**: 明度遷移の途中で撮っても減光が入らないか。

## 11. 未決・要確認・問題の報告

### 問題

| ID | 何が問題か | 再現入力 | 影響 | 分かっていること / いないこと |
|---|---|---|---|---|
| P1 | U0・U2 報告が作業ツリーに無い（§0） | `ls .cc-logs` | §4.2 の命令の分類を U0 §4 と突き合わせていない | 現状の事実はコードから確認。U0・U2 の記述との差は未確認 |
| P2 | `animationMs` が 3 つの役割を兼ねている: easing の長さ、spring の有効/無効（値は使われない）、開閉演出の有効/無効 | `animation="spring" animationMs={1}`（spring が有効になり、開閉も 150ms で動く）。`shell.rs:594`、`window_anim.rs:319`、`shell.rs:757-768` | 移行（§7.2）で、旧設定の意味を失わずに写すには種別ごとの展開が要る | 確認した（コードを読んだ） |
| P3 | スクロール・移動・リサイズが同じ `Effect::Configure` → `retarget` を通り、原因の情報が無い（`shell.rs:725-751`）。INV-M1 の「操作 → 動き種別」を `retarget` の中では強制できない | `focus-column-right` と `set-column-width` が同じ経路 | 命令を知っている `submit`（`shell.rs:645`）で種別を決めて渡す設計が要る（§4.2） | 確認した |
| P4 | niri のバネの収束時間（手計算で約 0.4〜0.7 秒）が INV-T1 の 400ms を超えうる | §5.2 | INV-T1 の対象に `niriSpring` を入れられない | 手計算のみ。数値で実行していない（`python3` が環境に無かった） |
| P5 | `window_dim` が 2 か所で独立に呼ばれる（`window_anim.rs:358`、`render/mod.rs:297`） | — | 補間中に値がずれる恐れ。1 か所で決めた値を両方が読む設計にする（§6.2） | 確認した |
| P6 | 時計を内部で読む箇所がある（`window_anim.rs:154,170,190`、`render/mod.rs` の描画経路、`CONTRAST_DEBUG_ANIM_SLOWDOWN` のプロセス全体の静的変数） | — | 開閉演出は純粋関数のテストが書きにくい。新しい補間は `elapsed` を引数に取る純粋関数にして、時計は呼び出し側だけが読む | 確認した |
| P7 | compositor の内部型 `anim::Motion`（`anim.rs:17`）と、CDL の `Motion` の名前が衝突 | — | 実装時の改名が要る | 確認した |
| P8 | `contrast-compositor/tests/spec/` と `contrast-policy-cdl/tests/spec/` が存在しない。compositor に `proptest` が無い | `ls crates/contrast-compositor/tests` | 人間が仕様テストを書く前に、置き場所の準備が要る（§8.1） | 確認した |
| P9 | `tick_animations` の戻り値を、呼び出し側（`drm.rs:359`、`winit.rs:209`）が再描画の予約にどう使うか読み切っていない | — | 明度遷移の実行中を、次のフレームの描画に反映する経路を実装前に確かめる必要がある | 未確認 |

### 未確認（読んでいない・実行していない）

- CDL の `Duration` リテラルの書式（`15ms` のような形か）。`cdl-host/src/convert.rs:84-95` に読み出しの型があることだけ確認した。
- CDL が `<Motion>` の未知の子・重複する子をどの E コードで拒否するか。`unique_child` を複数名に使えるか。
- `CT066`〜`CT069` が使われているか（`docs/cdl-contract.md:195-200` の表では CT065 まで）。
- ビルド・テストは実行していない（今回の調査には不要）。

## 12. 質問（選択肢と推奨つき）

| # | 質問 | 選択肢 | 推奨 |
|---|---|---|---|
| Q1 | INV-T1 の上限 400ms は設定で変えられるか | (a) 定数（変えられない） / (b) `Motion` の prop で上げ下げできる | (a)。設定で上げられる上限は不変条件にならない。「既定 400ms」の「既定」をどう読むか確認したい |
| Q2 | INV-T1 の文言: `niriSpring` をどう扱うか | (a) 「easing 系の長さの上限」と読み替え、`niriSpring` は対象外と明記 / (b) `niriSpring` も 400ms で打ち切る（niri 互換の基準線が変わる） | (a)。P4 のとおり、基準線が自分で 400ms を超える |
| Q3 | `Motion` の長さの型 | (a) `duration: Duration`（CDL の型。書式は要確認） / (b) `ms: Number`（今の `animationMs` と同じ流儀） | (a)。書式が使いにくければ (b) |
| Q4 | §4.2 の命令 → 種別の表のレビュー（特に `MoveToWorkspace`、`ToggleFloating`、`FocusMonitor`） | 表のとおり / 人間が修正 | 人間が確認（U0 §4 の分類と突き合わせる） |
| Q5 | Focus と矩形の動きを同時に走らせてよいか | (a) 同時に走ってよい（明示した例外） / (b) 矩形が動く間は Focus を即時にする | (a)。ただし「動きを重ねない」原則と衝突するので、実機（§10 の 2）で見てから最終決定 |
| Q6 | 仕様テストの置き場所と `proptest` | (a) すべて `contrast-policy-cdl/tests/spec/`（proptest あり）に置く / (b) compositor の `tests/spec/` にも置く（`contrast-compositor/Cargo.toml` に `proptest` を人間が追加） | (a)。純粋関数を `contrast-policy` に置くので足りる |
| Q7 | 段階 (a)(b)(c) の間、`Motion` と旧 prop が併存するときの優先順位 | (a) `<Motion>` があれば旧 `Layout.animation*` は無視（警告の診断を出す） / (b) 種別ごとに `Motion` が優先し、書かれていない種別は旧 prop に従う | (a)。(b) は挙動の説明が複雑になる |
| Q8 | INV-M1 の対象範囲 | §4.1 の X / **Y** / Z | Y。オーバービュー・ジェスチャー追従・スクリーンショット UI は U3 の対象外と明記 |
| Q9 | Open・Close を Motion に含めるか | §4.4 の A / B | A |
| Q10 | リロードで `Tier` の `l` が変わったとき、減光を補間するか即時にするか | (a) 補間（同じ規則） / (b) 即時 | (b)。リロードは設定変更で、操作の結果ではないため。小さな点なので人間が決めてよい |
