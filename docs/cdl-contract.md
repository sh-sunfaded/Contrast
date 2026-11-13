# Contrast ⇔ CDL 契約

Contrast が設定ファイル（CDL）に約束する内容。実装は `crates/contrast-policy-cdl`（CDL に触れるのはここだけ）と
`crates/contrast-policy`（CDL を知らない設定の型とストア）。CDL の API は `cdl-host` の公開 API だけを使う。
CDL 側の仕様は CDL リポジトリの `docs/SPEC.md`（6・8・9 章）と `docs/DECISIONS.md`（E1〜E8）。

**固定している CDL**: `https://github.com/sh-sunfaded/cdl.git` の `rev = 0d252db2bc7d477f2e2bf1d85180dd20b0e09a74`（CDL の `main` の HEAD。
以前の `923136d`（その前は `bcff355`）から更新。`crates/contrast-policy-cdl/Cargo.toml` と `Cargo.lock` が同じ rev を指す）。
この rev の CDL に含まれる、Contrast に関わる変更は 6 節の末尾「rev 0d252db での変更」にまとめる。

## 1. スキーマ（モジュール `host:contrast`）

実装済みの機能だけを置く。ルート要素は `Contrast`。

| 要素 | props | 子 | 一意 |
| --- | --- | --- | --- |
| `Contrast` | `modKey?: "alt" \| "super"`（省略時は下記の優先順位で決まる） | `Appearance` \| `Tiers` \| `Workspaces` \| `Layout` \| `Cursor` \| `Input` \| `Startup` \| `DataControlClient` \| `Environment` \| `Keybind` | `Appearance`、`Tiers`、`Workspaces`、`Layout`、`Cursor`、`Input`、`Environment` |
| `Appearance` | `borderWidth?: Length`（省略 = 0 = 枠なし）、`background?: Color`、`radius?: Length`、`shadowBlur?: Length`、`shadowOffsetX?: Length`、`shadowOffsetY?: Length`、`shadowColor?: Color`、`inactiveBorderWidth?: Length` | なし | — |
| `Tiers` | なし | `FocusTier` \| `Tier` | `Contrast` の中で 1 つまで。`FocusTier` は `Tiers` の中で 1 つまで（`E4006`）。省略すると組み込みの段階（フォーカス `l=1`、フォーカス外 `l=0.8`、枠なし） |
| `FocusTier` | `l: number`（0 より大きく 1 以下）、`ringColor?: Color` | なし | `Tiers` の中でちょうど 1 つ（無いと CT061） |
| `Tier` | `l: number`（0 より大きく 1 以下）、`ringColor?: Color`、`name?: string`（診断用。空・重複は CT065） | なし | `Tiers` の中でちょうど 1 つ（この版。0 個・2 個以上は CT064） |
| `Workspaces` | `count: number`（整数、1〜32）、`mode?: "fixed" \| "dynamic"`（既定 `"fixed"`） | なし | — |
| `Layout` | `gap?: Length`、`outerGap?: Length`、`mode?: "equal" \| "scrolling"`（既定 `"equal"`）、`defaultColumnWidth?: number`、`presetColumnWidths?: string`、`windowHeightPresets?: string`、`animation?: "easing" \| "spring"`（既定 `"easing"`）、`animationMs?: number`（既定 0） | なし | — |
| `Cursor` | `theme?: string`、`size?: number`、`hideAfterInactiveMs?: number` | なし | — |
| `Screenshot` | `path?: string`（`~` と strftime の `%Y` などを展開。既定は niri と同じ `~/Pictures/Screenshots/Screenshot from %Y-%m-%d %H-%M-%S.png`。空文字列で保存しない） | なし | — |
| `HotkeyOverlay` | `skipAtStartup?: boolean`（起動時にオーバーレイを開かない。niri の `skip-at-startup`）、`hideNotBound?: boolean`（束縛の無い行を隠す。`hide-not-bound`） | なし | — |
| `Input` | `tapToClick?: boolean`（既定 true）、`naturalScroll?: boolean`（既定 false）、`accelSpeed?: number`（-1〜1、既定 0）、`disableWhileTyping?: boolean`（既定 true）、`focusFollowsMouse?: boolean`（既定 false） | なし | — |
| `Startup` | `command: string`、`ipc?: "niri"` | なし | —（複数可） |
| `DataControlClient` | `name: string` | なし | —（複数可） |
| `Environment` | なし | `Env` | 一意 |
| `Env` | `name: string`、`value: string` | なし | —（`Environment` の中だけ） |
| `Keybind` | `keys: string`、`action: string`、`command?: string`、`workspace?: number`、`width?: number`、`delta?: number`、`height?: number`、`heightPx?: number` | なし | — |

- `Layout.mode` はスクロール列レイアウト（core-spec 12 節）を選ぶ。起動時に決まり、リロードでは変わらない（変えるには再起動）。
  `defaultColumnWidth` は新しい列の幅（0 より大きく 1 以下の割合、既定 0.5）。`presetColumnWidths` はプリセットの列幅で、
  割合を空白かコンマで区切って 1〜8 個（既定 `0.33333 0.5 0.66667`）。割合は分母 1000 に丸める。範囲外・不正な書式は CT003。
- `Layout.animationMs` は窓のアニメーションの長さ（0〜2000 の整数ミリ秒。0 で無効、既定 0）。0 より大きいと、core が決めた窓の行き先の矩形へ、
  今の矩形から ease-out で近づける。範囲外・非整数は CT003。リロードで更新する。
  - 移動と大きさの変化: 補間した矩形を、毎フレーム窓に configure する（枠・影・隙間も補間した矩形に従う）。
    クライアントが追いつくまでの数フレームは、窓の中身と枠にずれが出ることがある。
  - ワークスペースの切り替え: 新しく見える窓が、番号が大きくなる向きなら下から、小さくなる向きなら上から滑り込む。
    出ていく窓は補間せず、すぐ消える。
  - 補間しない: 窓が最初に開くとき、全画面にするとき（全画面から戻るときは全画面の矩形から補間する）、出力の大きさが変わるとき。
- 見た目の追加 prop はすべて省略でき、省略時は枠なし・角丸 0・影なし・隙間 0（非フォーカスの枠の幅は `borderWidth`）。
  - `gap` は窓と窓の間、`outerGap` は出力の端と窓の間（px）。core の矩形は変えず、描画側で縮める。
  - `radius` は窓と枠の角丸の半径。`shadowBlur` が 0 のとき影は描かない。`shadowOffsetX/Y` は負も使える。
  - 枠（フォーカスリング）は窓の**外側**に描く（niri と同じ）。窓の大きさは枠の太さで変わらない（隙間 `gap` の中に枠が広がる）。`inactiveBorderWidth` はフォーカス外の窓の枠の太さ。
    枠を描くのは、その窓の段階に `ringColor` があり、かつ太さが 1 以上のときだけ。フォーカス窓の太さは `borderWidth`、フォーカス外は `inactiveBorderWidth`（省略時は `borderWidth`、`borderWidth` に収める）。
    `borderWidth` を省略（0）にすると、どの窓にも枠を描かない。
  - 長さの範囲は 0〜512 の整数 px（`shadowOffsetX/Y` は -512〜512）。外れると CT003。`Cursor.size` は 8〜256 の整数。
  - `Cursor` は環境変数 `XCURSOR_THEME` / `XCURSOR_SIZE` より優先する。リロードで更新する。
- `Startup` は niri の `spawn-at-startup`。コンポジタが Wayland のソケットを公開したあと、書かれた順に、起動時に一度だけ spawn する。
  リロードでは再実行しない。`command` は空白で区切った先頭がプログラム、残りが引数（引用符・シェル記法は使えない。要るなら `sh -c …`）。
  空（空白だけ）の `command` は CT003。起動に失敗しても残りを続け、ログに出す。
  `ipc="niri"` を付けた `command` の子には、環境変数 `NIRI_SOCKET`（niri 互換の IPC のソケットのパス。`$XDG_RUNTIME_DIR/contrast.<pid>.sock`）を足す。
  ironbar の workspaces のように `NIRI_SOCKET` で niri を見分けるクライアント向けで、ほかの子（ほかの `Startup` と `Keybind` の `spawn`）には渡さない。
  `ipc` は `"niri"` だけ。ほかの値は CT003。IPC が起動できなかったときは、`NIRI_SOCKET` を渡さない（ログに警告）。
- `DataControlClient` は、wlr-data-control（`zwlr_data_control_manager_v1`。フォーカスなしでクリップボードを読み書きできる）を使ってよいクライアントの、
  プログラム名（実行ファイルの名前。nix のラッパー `.name-wrapped` は `name` とみなす。取れなければ `comm`）。ここに無いクライアントには、そのグローバルを見せない。
  1 つも書かないと既定（`ironbar`・`wl-copy`・`wl-paste`・`cliphist`・`wl-clip-persist`）、1 つでも書くと、書いたものだけに置き換わる。
  空・空白や `/` を含む `name`・重複は CT003。リロードで更新する（以後に接続するクライアントから効く）。
- `Environment` は niri の `environment`。`Env` の `name` と `value` の組を、spawn する子（`Startup` と、`Keybind` の `spawn`）の環境変数に足す。
  コンポジタ自身の環境は変えない。`WAYLAND_DISPLAY` は常にコンポジタのソケットで上書きする。`name` は空でなく、`=` と NUL を含まない。
  同じ `name` の重複・不正な `name`/`value` は CT003。リロードで更新する（以後の spawn から効く）。
- `Cursor.hideAfterInactiveMs` は niri の `cursor.hide-after-inactive-ms`。入力（キー・ポインタ）がこの時間（1〜3,600,000 の整数ミリ秒）ないと、
  カーソルを隠し、入力があると出す。省略すると隠さない。範囲外・非整数は CT003。nested（winit）ではホストの窓のカーソルを隠す。
- `Input.focusFollowsMouse` は niri の `focus-follows-mouse`。true のとき、ポインタが窓に入る（窓の上で動く）とその窓にフォーカスを移す。
  ポインタをつかんでいる間（ドラッグ中）と、レイヤー面がキーボードを持っている間は移さない。
- `Input` はタッチパッドの設定。libinput のデバイスが追加されたとき（起動時の既存デバイスを含む）とリロードのときに、タッチパッドに適用する。
  デバイスが対応しない項目はログに出して無視する。`accelSpeed` が -1〜1 の外（または非数）なら CT003。
- `Appearance`、`Workspaces`、`Input` を省略したときは `Settings::builtin()` の値を使う。
- `modKey` と `action` は本来リテラルユニオン（`"alt" | "super"` など）だが、`cdl-host` は `Literal` を公開していないので
  スキーマ上は `string` にして、Contrast の変換で検査する（誤りは CT003 / CT020）。
- `borderWidth` は 0〜256 の整数（px。範囲外・非整数は CT003）。枠の色は段階の `ringColor`（`FocusTier` がフォーカス窓、`Tier` がフォーカス外の窓）。`background` は背景色（省略時は組み込みの色）。
- **旧 prop の廃止（DU-8）**: `Appearance` の `focusColor`・`borderColor` は廃止した。書くと CDL が「`<Appearance>` に props `…` はない」（`E4003`）で拒否する（位置はその prop）。
  起動時なら `Settings::builtin()` で起動、リロードなら旧版を保持する（3 節）。`focusColor` → `FocusTier.ringColor`、`borderColor` → `Tier.ringColor`。
- **注意の段階（Tiers）**: 明度 `l` の段階で「注意」を表す。フォーカス窓は `FocusTier`、フォーカス外の窓は `Tier`。検査（INV-A1）は変換で行う:
  1. 各 `l` は有限で 0 より大きく 1 以下（CT060。`l` が数でなければ CDL の型の診断）。
  2. `FocusTier` が無い（CT061）。
  3. 段階が単調減少でない: `F > n_1 > …`（フォーカス段階が最も明るく、書いた順に狭義の単調減少。CT062）。
  4. DU-12: `F − max(N) > max(N) − min(N)`（狭義。差は OKLCH の L の差。`F` はフォーカス段階、`N` はフォーカス外の段階の `l` の集合。CT063。`Tier` が 1 つなら `F > n_1` と同じ）。
  5. `Tier` の個数がちょうど 1 でない（この版は 1 つまで。CT064）。2 個以上のときは、4 も破れば CT063 も同時に出る。
  6. `name` が空・重複（CT065）。
  **減光**: フォーカス外の窓（Tiled / Floating。全画面の窓とフォーカス窓は除く）の画素に、プリマルチプライドの RGB へ乗数 `k = (l_tier / l_focus)^(3/2.2)` を掛ける（アルファ・矩形・枠・影は変えない。ポップアップ・サブサーフェスも同じ乗数）。
  **`k` と組み込みの `l = 0.8` は仮置き**（中立のグレーでだけ正確な近似で、目標値ではない）。`l` は「窓の画素を何倍に暗くするか」の指標で、周囲（壁紙・隣の窓）に対する知覚コントラストの保証ではない（未決。ui-breaking-update §7）。
  `screenshot-window` は減光しない（常に乗数 1）。`screenshot-screen` は画面に見えているとおり減光が入る。オーバービューは減光しない。

### `modKey` の決まり方

優先順位は、CDL の `modKey`（明示）> 環境変数 `CONTRAST_MOD_KEY=alt|super` > バックエンドの既定。
バックエンドの既定は、DRM（実機の TTY）が `super`、winit（nested）が `alt`（ホストのコンポジタが Super を奪うため）。
読み込み器（`CdlLoader::with_default_mod_key`）が「省略時の `mod`」を受け取り、`modKey` が無い設定にはそれを使う。
`modKey` が不正なら CT003 で、その設定は使わない。環境変数が不正なら警告して無視する。
組み込みの既定値（設定ファイルが無いとき）は、この規則に関わらず Alt のまま。

### action

`spawn` / `close` / `focus-left` / `focus-right` / `move-left` / `move-right` / `workspace` / `move-to-workspace` /
`workspace-next` / `workspace-prev` / `move-to-workspace-next` / `move-to-workspace-prev` / `move-column-to-workspace` / `move-column-to-workspace-next` / `move-column-to-workspace-prev` / `move-workspace-down` / `move-workspace-up` /
`toggle-floating` / `toggle-fullscreen` / `reload-config` / `quit` / `power-off-monitors`（画面の電源を切る。DRM のみ。どんな入力でも戻る）/ `screenshot-screen`（出力の絵を PNG で保存。保存先は `Screenshot.path`）/ `screenshot-window`（フォーカス窓だけの絵を、透明な背景の PNG で保存。枠とポインタは入れない）/ `screenshot`（範囲を選ぶ UI。Escape で閉じる、Space・Enter で保存、Ctrl+C でクリップボードだけ）/ `show-hotkey-overlay`（主なキーの一覧。起動時にも出る。キーなどで閉じる）/ `toggle-overview`（オーバービューを開く・閉じる。左上のホットコーナーと 4 本指のスワイプでも開く）/
`focus-up` / `focus-down` / `move-up` / `move-down` /
`focus-column-first` / `focus-column-last` / `move-column-first` / `move-column-last` /
`consume-window-into-column` / `expel-window-from-column` / `consume-or-expel-window-left` / `consume-or-expel-window-right` / `switch-preset-column-width` / `maximize-column` /
`set-column-width` / `adjust-column-width` /
`center-column` / `switch-preset-window-height` / `reset-window-height` / `set-window-height` / `adjust-window-height` /
`focus-monitor-left` / `focus-monitor-right` / `focus-monitor-up` / `focus-monitor-down` /
`move-column-to-monitor-left` / `move-column-to-monitor-right` / `move-column-to-monitor-up` / `move-column-to-monitor-down`。

- `move-column-to-workspace` に `focus={false}` を付けると、列を移すだけで表示は切り替えない（niri の `focus=false`。core の追補 19 節）。既定は true。
- 出力の操作（`focus-monitor-*`・`move-column-to-monitor-*`、niri と同じ名前）は、`Workspaces.mode` が `dynamic` のとき（出力ごとのワークスペースの列。core-spec の追補 17 節）。
  `fixed` のときは何もしない。`move-column-to-monitor-*` は、さらに `Layout.mode` が `scrolling` のときだけ。
  その方向に出力が無ければ何もしない。出力が変わったら、カーソルをその出力の中央へ動かす（niri と同じ）。

- 列の操作（`focus-column-*` 以降）と `focus-up/down`・`move-up/down` の列での意味は、`Layout.mode` が `scrolling` のときだけ。`equal` のとき、列の操作は何もしない
  （`focus-up/down`・`move-up/down` は従来の幾何・順序列の規則）。
- 窓の高さと列のセンタリング（`center-column` 以降の 5 つ。core-spec 14 節）も、`Layout.mode` が `scrolling` のときだけ。**`equal` のとき、これらは何もしない**（状態不変）。
  - `center-column` はフォーカス列の中心を作業領域の中心に合わせる。`switch-preset-window-height` はフォーカス窓の高さを、今の高さより高い最初のプリセットへ変える
    （なければ先頭）。`reset-window-height` は高さを自動に戻す。
  - `set-window-height` は、`height`（0 より大きく 1 以下の割合。`set-column-width` の `width` と同じ形）か `heightPx`（1〜16384 の整数 px）の**どちらか一方**が必須。
    両方・どちらもなし・範囲外は CT003。`adjust-window-height` は `delta`（`adjust-column-width` と同じ。0 以外で -100〜100 の整数のパーセント）が必須で、
    フォーカス窓の今の高さの割合（整数パーセント）に足して 1〜100 に収める（niri の `set-window-height "±10%"`）。
  - 窓の高さは、列の中の窓ごとに持つ。窓が別の列へ移ると自動に戻る。列の高さの配分は core-spec 14 節（D50）。
- `Layout.windowHeightPresets` は窓の高さのプリセット（`presetColumnWidths` と同じ書式。割合を空白かコンマで区切って 1〜8 個、分母 1000 に丸める。既定 `0.33333 0.5 0.66667`）。範囲外・不正な書式は CT003。起動時に決まり、リロードでは変わらない。
- `set-column-width` は `width`（0 より大きく 1 以下の割合）が必須。`adjust-column-width` は `delta`（0 以外で -100〜100 の整数のパーセント）が必須で、
  フォーカス列の今の幅の割合（整数パーセント）に足して 1〜100 に収める。どちらも不正なら CT003。

- `Workspaces.mode` は、ワークスペースの方式。`"fixed"` は `count` 個で固定。`"dynamic"` は niri のように、窓のあるものと末尾の空 1 つ（空の状態から始まる）。
  `dynamic` のとき `count` は `workspace` の番号の上限（CT022 の検査）にだけ使う。ほかの値は CT003。起動時に決まり、リロードでは変わらない（CT050）。
- `move-column-to-workspace`（`workspace` 必須）／`-next`／`-prev` は、フォーカス窓の**列ごと**移して、そこへ切り替える（`Layout.mode` が `scrolling` のときだけ。`equal` では何もしない。
  Floating・Fullscreen の窓は、その窓だけ）。`dynamic` では、存在しない番号は末尾の空のワークスペースになる。`move-workspace-down` / `-up` は、表示中のワークスペースを 1 つ下／上と入れ替える（`dynamic` のみ）。
- `workspace-next` / `workspace-prev` は、いま表示中の番号の ±1 へ切り替える。`move-to-workspace-next` / `-prev` は、
  フォーカス窓をその番号へ移す。端では何もしない（循環しない）。引数は取らない。

- `spawn` は `command` 必須（空白で区切った先頭がプログラム、残りが引数）。
- `workspace` と `move-to-workspace` は `workspace` 必須。CDL では 1 始まり。core の `WorkspaceIndex` は 0 始まりなので
  変換する。1 未満・非整数・`Workspaces.count` 超過は CT022。
- 上記以外の action は CT020。`command` が要る action に無ければ CT021。

### キー書式

`mod+Return`、`mod+shift+q` のように `+` で区切る。最後が xkb のキー名、それ以前が修飾キー。

- 修飾キー: `mod`（`modKey` に従って alt か super）/ `alt` / `shift` / `ctrl` / `super`。大文字小文字は区別しない。
- キー名: xkb のキー名（`Return`、`q`、`Left`、`1`）。大文字小文字は区別しない。
- 修飾キーの重複（`alt+mod+x` で `mod` が alt など）は 1 つにまとめる。
- 空・キー名なし・未知の修飾キーは CT030。同じ（修飾キーの集合, キー名）の重複は CT031（2 つ目を指す）。

## 2. ポリシーのフック

```ts
interface WindowInfo { appId: string; title?: string; isDialog: boolean; workspace: number }
interface Placement  { workspace?: number; floating: boolean; focus?: boolean; fullscreen?: boolean; columnWidth?: string }
interface Policy     { windowOpen?: (w: WindowInfo) => Placement }
```

- `windowOpen(w)` は新しい窓が最初に map されるときに 1 回呼ばれる。
- `appId`: 無いときは `""`。`title`: 無いときは省略。`workspace`: フォーカス中のワークスペース（1 始まり）。
- 戻り値 `workspace` を省略したときはフォーカス中のワークスペース。`focus` を省略したときは `true`。
  `focus: false` は、**既にフォーカス窓があるときだけ**有効で、フォーカスが None のときは無視して新しい窓にフォーカスを渡す（DU-10。表示中のワークスペースに窓があればフォーカスは Some）。
  **この動作の core 側は未実装**（実装は core-spec への追記と tests/spec の INV-F1 の検査が先。U2 の段階 A はスキップ）。現状は `focus: false` ならフォーカス None のまま。
- `fullscreen`（niri の `open-fullscreen`）を省略したときは `false`。`true` なら、配置のあとに全画面にする（コンポジタが core の `ToggleFullscreen` を送る）。
  `ToggleFullscreen` はフォーカス窓に効くので、窓がフォーカスを得たときだけ全画面になる（`focus: false` や、表示中でないワークスペースへの配置では、全画面にしない。ログに出す）。
- `columnWidth: "window"`（niri の窓ルールの `default-column-width {}`）なら、列の幅を窓に決めさせる（最初の configure の幅を 0 にし、窓が描いた幅＋隙間を列の幅にする。core の追補 20 節）。ほかの値は診断。
- フックを定義しないときは既定の配置（ダイアログは Floating、他は Tiled。フォーカスする）。

## 3. 失敗時の既定

| 状況 | 動作 |
| --- | --- |
| 起動時に読み込み・評価・変換のどれかが失敗 | `Settings::builtin()` と既定の配置で起動し、診断を error でログに出す。 |
| 設定ファイルがない | 既定値で起動する。エラーではない（info ログ）。 |
| 変換の誤りが 1 つでもある | その版を採用しない（半端な設定を作らない）。誤りは全部集めて報告する。 |
| リロードが失敗 | 旧版を保持し、診断を出す。世代は進めない。 |
| フックの上限超過・評価エラー・戻り値が不正（workspace が非整数・1 未満・範囲外など） | その呼び出しだけ既定の配置にし、診断を出す。同じコードは世代ごとに 1 回だけログに出す。 |
| リロードで `Workspaces.count` が変わった | core は起動後にワークスペース数を変えられない。新しい版は採用するが、数は起動時のまま。warning を出す（CT050）。 |

## 4. 診断コード（名前空間 CT。CDL の `E` とは別）

| コード | 意味 |
| --- | --- |
| CT001 | エントリの設定ファイルの状態を調べられない・パスを正準化できない（読めない・UTF-8 でない・サイズ超過は CDL の `E5001` で出る） |
| CT002 | 値木が期待と食い違う（未知の要素・ルートが違う。スキーマとの不一致で、通常は起きない） |
| CT003 | prop の値が不正（`modKey` が alt/super でない、`borderWidth` が負または非整数、`Startup` の `command` が空、`Env` の名前が不正・重複、`hideAfterInactiveMs` が範囲外、など） |
| CT010 | `Workspaces.count` が 1〜32 の整数でない |
| CT020 | 未知の action |
| CT021 | `command` が要る action に `command` がない（または空） |
| CT022 | `workspace` が要る action に `workspace` がない・1 未満・非整数・`count` 超過 |
| CT030 | キー書式の誤り |
| CT031 | 同じキーの重複 |
| CT040 | フックの評価エラー・上限超過 |
| CT041 | フックの戻り値の形が不正 |
| CT042 | フックの戻り値の `workspace` が不正（非整数・1 未満） |
| CT043 | フックの戻り値の `workspace` が `Workspaces.count` を超える |
| CT050 | リロードで反映できない変更（ワークスペース数） |
| CT060 | `FocusTier` / `Tier` の `l` が有限の数でない、または 0 以下か 1 超（位置はその要素） |
| CT061 | `Tiers` に `FocusTier` が無い（位置は `Tiers`。2 個以上は CDL の `E4006`） |
| CT062 | 段階が単調でない（`F > n_1 > …` を満たさない。位置は破った `Tier`） |
| CT063 | フォーカス段階との差が、フォーカス外の段階全体の幅以下（DU-12。位置は `FocusTier`。メッセージに F・max(N)・min(N)） |
| CT064 | `Tier` の個数がちょうど 1 でない（この版。位置は `Tiers`） |
| CT065 | `Tier` の `name` が空・重複（位置はその `Tier`） |

CDL 自身の診断（`E0001`〜）は、そのままのコードで `ConfigDiagnostic` に写す。位置は `file:line:col`。
値木の診断は要素単位の位置しか取れない（props に位置がない）ので、誤りのある prop を含む要素の位置を示し、メッセージに prop 名を書く。

## 5. 設定ファイルの場所

`CONTRAST_CONFIG` → `$XDG_CONFIG_HOME/contrast/config.cdl` → `~/.config/contrast/config.cdl`（空の環境変数は無いものとして扱う）。
`import "./x.cdl"` は、設定ファイルのあるディレクトリの外を拒否する（`canonicalize` の後で判定するので、symlink で出るものも拒否）。
読む前にファイルのサイズを確認し、`Limits::max_source_bytes` を超えるものは読まない。

## 6. CDL の上限と診断（上限は rev 923136d から変わらない）

Contrast は `cdl_host::Limits` の既定値（`Limits::default()`）をそのまま使う。上書きしない。

| `Limits` のフィールド | 既定値 | 超過したときの診断 |
| --- | --- | --- |
| `max_source_bytes` | 1 MiB（1,048,576） | `E5001` |
| `max_nesting`（構文の入れ子） | 64（rev bcff355 では 128） | `E1008` |
| `max_modules`（読み込むファイル数。エントリを含む） | 256（新規） | `E5013` |
| `max_steps`（評価ステップ） | 1,000,000 | `E5002` |
| `max_call_depth`（呼び出しの深さ） | 64 | `E5003` |
| `max_eval_depth`（式と要素の再帰の深さ。呼び出しをまたぐ） | 128（新規） | `E5012` |
| `max_string_bytes`（1 文字列の長さ） | 64 KiB | `E5004` |
| `max_collection_len`（1 配列・マップの要素数） | 65,536 | `E5005` |
| `max_nodes`（値木のノード数） | 1,000,000 | `E5006` |
| `max_value_depth`（値木の深さ。要素・配列・オブジェクトを 1 段） | 64（新規） | `E5011` |
| `max_hook_steps`（フック 1 回の評価ステップ） | 10,000 | `E5002` |

- 型の大きさ（深さ 128・ノード数 4,096、別名の連鎖も深さ 128。ホストは上書きできない）の超過は `E3013`。
- 文字列を作るとき、256 バイトごとに 1 ステップを使う。値木の深さの検査は、たどった値 1 つにつき 1 ステップを使う。
- E4008（変換の失敗）のメッセージには、値そのものではなく値の種類（`number`・`string`・`配列` など）を入れる。
  Contrast の診断は CDL の診断を `ConfigDiagnostic` に写すので、メッセージの文言は新しい版で変わる。

### CDL の上限・検査で新しく出る診断（rev 923136d）

| コード | 意味 |
| --- | --- |
| E3013 | 型が大きすぎる（深さ・ノード数・参照の深さの上限） |
| E5011 | 値木の深さの上限超過 |
| E5012 | 評価の再帰の深さの上限超過 |
| E5013 | 読み込むファイル数の上限超過（上限を超えた import の位置で出て、そのファイルは開かない） |

これらは CDL の `E` の名前のまま出る（CT の番号は付けない）。設定ファイル（`config/`・`examples/`）は、
この rev でも診断なしで読める（`impl_config.rs`・`impl_loader.rs` で確かめる）。

### rev 0d252db での変更（923136d から。未検証）

CDL の DECISIONS R1〜R4・U1〜U5 による。`Limits` の既定値は変わらない。

**cdl-host の公開 API（Contrast の対応）** — 対応は `crates/contrast-policy-cdl/src/{convert,loader}.rs`。

| 変更 | Contrast の対応 |
| --- | --- |
| `Program::root()` / `document()` が `Option`（関数の形のルートでは `None`） | 静的なルート（要素）だけを受け付ける。`None` は `CT003` の内部エラーにする（`Env` は宣言しない） |
| `Node::name()` → `component()`、`element()` 廃止 → `span()` | 置き換え |
| `Node::optional_prop::<T>` 廃止 → `prop::<Option<T>>` | 置き換え（意味は同じ: 無い・`undefined` は `None`） |
| `Node::expect` → `expect_component` | 置き換え |
| `Program::render`、`SchemaBuilder::env`、`ComponentDef::text_children`、`Node::key()`、`FromNode`、`Child` | 使わない（動作を変えない） |

**言語（CDL 側）の変更。いずれも書ける範囲が広がる。Contrast の `.cdl` は書き直していない。**
テキスト子（string を受け付けると宣言したコンポーネントだけ。Contrast のコンポーネントは宣言しないので、置けば `E3014`）、
`c && <A />`、`key`（string のみ。number は `E3017`）、引数の分割代入（1 階層・既定値つき）、
ルートの関数の形（`Env` を宣言しないホストでは `E4013`）。`key` は props ではない。同一性の規則に識別 prop が入ったが、
Contrast は識別 prop の宣言を足さない（7 節）。

**新しい診断コード**:

| コード | 意味 |
| --- | --- |
| E1012 | JSX のテキスト子の `&`（文字参照は使えない） |
| E3014 | テキスト子を受け付けないコンポーネントにテキストを置いた |
| E3015 | 省略可能でない prop への既定値（引数の分割代入） |
| E3016 | スロットに、型の違う要素を渡した |
| E3017 | `key` が string でない |
| E4011 | 同じ親の子の `key` の重複 |
| E4012 | ルート関数の引数が `Env` 1 つでない |
| E4013 | ホストが `Env` を宣言していないのに、ルートを関数の形で書いた |
| E4014 | ホストが渡した `Env` の値が宣言した型に合わない |

`E1009`（JSX のテキスト子）は欠番（出さない）。`E1007` は `key` を除く（`ref`・`on[A-Z]`・`use[A-Z0-9_]` は従来どおり）。
`Program::render` と `evaluate` の誤りは `Vec<Diagnostic>`（Contrast は `load` だけを使うので影響なし）。

この rev での設定ファイル（`config/`・`examples/`・tests の `.cdl`）の読み込みは、**評価していない（未検証）**。

## 7. 未実装

- 識別 prop（意味が不明なので実装しない）。
- ファイル監視、画面上の診断表示。窓の開閉（出ていく窓・開く窓）のアニメーションと、出ていく窓の補間。
