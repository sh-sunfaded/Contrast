# niri との差分表

Contrast の見た目と操作を niri に合わせるための調査。入力は `~/.config/niri/config.kdl`（作者の実際の設定）。
niri の設定の既定値は niri の既定（wiki / パッケージの既定設定）に従うものとして書いた（ネットワークからは取っていない）。

状態の凡例: **○** 実現済み / **△** コンポジタか CDL の変更で可（contrast-core は変えない）/ **×** contrast-core の変更が要る（または大きな新機能）。
規模: S 小（数時間）/ M 中 / L 大。
「使用」: 作者の config.kdl で有効に使っている（`/-` で無効化されているもの、コメントだけのものは「いいえ」）。

## 見た目

| niri の機能 | 使用 | Contrast の今 | 必要な変更 | 規模 |
|---|---|---|---|---|
| `layout.gaps 16` | はい | △ → **N1 で実装**（`Layout.gap`） | 描画側で core の矩形を縮める | S |
| `layout.struts` | 空 | △ → N1 は `Layout.outerGap`（四辺同じ値）。辺ごとの struts は未 | CDL に辺ごとの値 | S |
| `focus-ring`（width 4、active #7fc8ff、inactive #505050） | はい | ○ 近い（`Appearance.borderWidth`・`focusColor`・`borderColor`）。窓を枠の幅だけ縮める点が違う | N1 で非フォーカスの枠の幅（`inactiveBorderWidth`）を足した | S |
| `border`（off） | 無効 | △ focus-ring と同じ仕組みの 1 本だけ。2 本重ねは未 | — | S |
| `shadow` | いいえ | △ → **N1 で実装**（`shadowBlur` / `shadowOffsetX/Y` / `shadowColor`） | 影のシェーダ | M |
| `geometry-corner-radius` | いいえ | △ → **N1 で実装**（`Appearance.radius`） | 窓の角丸シェーダ（カスタムテクスチャシェーダ） | M |
| 背景 | はい（swaybg） | ○ 背景色 `Appearance.background`。壁紙は swaybg が layer-shell の背景面で動く | — | — |
| `animations`（既定のまま） | 既定 | ○ **N2 で実装**（`Layout.animation="spring"`＝niri の既定のバネ曲線、`Layout.animationMs` は無効化（0）。窓の移動・大きさ・ワークスペース切替の矩形の補間）。窓の開閉の補間は未 | 開閉（出ていく窓・開く窓）の補間 | M |
| `cursor`（xcursor-theme Cecilia、size 24） | はい | ○ `XCURSOR_THEME` / `XCURSOR_SIZE`。→ N1 で CDL の `Cursor` を足した | CDL > 環境変数 | S |
| `cursor.hide-after-inactive-ms` | いいえ | ○ **N2 で実装**（`Cursor.hideAfterInactiveMs`） | — | — |
| `hotkey-overlay` | いいえ | × 該当なし | オーバーレイ描画 | M |

## レイアウト

| niri の機能 | 使用 | Contrast の今 | 必要な変更 | 規模 |
|---|---|---|---|---|
| `preset-column-widths`（1/3・1/2・2/3） | はい | ○ **N3a で実装**（`Layout.presetColumnWidths`、`switch-preset-column-width`） | — | — |
| `default-column-width { proportion 0.5; }` | はい | ○ **N3a で実装**（`Layout.defaultColumnWidth`、`mode="scrolling"`） | — | — |
| `center-focused-column "never"` | はい | ○（スクロールしないので常に当たり前に満たす） | — | — |
| `switch-preset-column-width` / `maximize-column` / `set-column-width` | はい | ○ **N3a で実装**（`adjust-column-width` は ±N% の相対指定） | — | — |
| `center-column` | はい | ○ **N3b で実装**（`center-column`） | — | — |
| `focus-column-left/right`・`move-column-left/right` | はい | ○（`focus-left/right`・`move-left/right`） | — | — |
| `focus-column-first/last`・`move-column-to-first/last` | はい | ○ **N3a で実装** | — | — |
| `focus-window-up/down`・`move-window-up/down` | はい | ○ **N3a で実装**（列の中の上下） | — | — |
| `consume-window-into-column`・`expel-window-from-column` | はい | ○ **N3a で実装** | — | — |
| `consume-or-expel-window-left/right` | はい | × N3b | core | M |
| `switch-preset-window-height`・`reset-window-height`・`set-window-height` | はい | ○ **N3b で実装**（`switch-preset-window-height`・`reset-window-height`・`set-window-height`・`adjust-window-height` は ±N%。`Layout.windowHeightPresets`） | — | — |
| `fullscreen-window` | はい | ○（`toggle-fullscreen`） | — | — |
| `toggle-window-floating` | いいえ（既定の Mod+V） | ○（`toggle-floating`） | — | — |
| スクロール（ビューが列を追う） | 暗黙 | ○ **N3a で実装**（フォーカス列が見える最小のスクロール） | — | — |

## ワークスペース

| niri の機能 | 使用 | Contrast の今 | 必要な変更 | 規模 |
|---|---|---|---|---|
| `focus-workspace N`・`move-column-to-workspace N` | はい | ○（`workspace`・`move-to-workspace`。数は `Workspaces.count`） | — | — |
| `focus-workspace-down/up`・`move-column-to-workspace-down/up` | はい | ○（`workspace-next/prev`・`move-to-workspace-next/prev`。端で循環しない） | — | — |
| 動的な縦のワークスペース（空きが自動で増減） | 暗黙 | × 固定数 | core のワークスペースの増減 | L |
| `move-workspace-down/up` | はい | × 並べ替えが無い | core | M |
| 名前つきワークスペース（`workspace "name"`） | いいえ | × | core と CDL | M |
| `focus-monitor-*`・`move-column-to-monitor-*`・出力ごとのワークスペース | はい | × 出力 1 枚だけ（core は複数出力に対応しているが、コンポジタが 1 枚前提） | コンポジタの複数出力（ホットプラグ含む） | L |
| `output`（mode・scale・transform・position） | 無効 | × スケール 1 固定、modeset は preferred のみ | DRM のモード選択、スケール | L |
| `toggle-overview` | いいえ | × | 縮小表示の描画と操作 | L |

## 操作

| niri の機能 | 使用 | Contrast の今 | 必要な変更 | 規模 |
|---|---|---|---|---|
| `spawn` / `spawn-sh` | はい | ○（`spawn`。`spawn-sh` はシェル記法を使っていなければ同じ） | シェル記法が要る場合は `sh -c` | S |
| `spawn-at-startup`（fcitx5・ironbar・swaybg・mako・udiskie） | **はい（5 つ）** | ○ **N2 で実装**（`Startup`。ソケット公開後に一度だけ、順に spawn） | — | — |
| `environment`（QT_IM_MODULE など 4 つ） | **はい** | ○ **N2 で実装**（`Environment` / `Env`。spawn する子の環境に足す） | — | — |
| `close-window`・`quit` | はい | ○ | — | — |
| 音量などの `XF86Audio*`（`allow-when-locked`） | はい | ○（`spawn`。ロック画面は無い） | — | — |
| `Mod+ホイール`系 | はい | × ポインタ軸のバインドが無い | 軸イベントのバインド | M |
| `show-hotkey-overlay` | はい | × | オーバーレイ | M |
| `power-off-monitors` | はい | ○（DRM のみ。実機で確認） | — | — |
| `screenshot` / `screenshot-screen` / `screenshot-window`（`screenshot-path`） | はい | × screencopy も UI も無い | スクリーンショットの描画読み出しと保存、操作 UI | L |
| `Super+Alt+L`（swaylock） | はい | △ spawn は ○。swaylock には `ext-session-lock` が要る | session-lock プロトコル | M |

## 入力

| niri の機能 | 使用 | Contrast の今 | 必要な変更 | 規模 |
|---|---|---|---|---|
| `input.touchpad`（tap・natural-scroll） | はい | ○（`Input`。実機で確認待ち） | — | — |
| `input.keyboard.xkb` | 空 | ○（`CONTRAST_XKB_*`） | CDL にも足すなら S | S |
| `input.mouse` | 空 | ○ 既定のまま | — | — |
| `focus-follows-mouse` | いいえ | ○ **N2 で実装**（`Input.focusFollowsMouse`） | — | — |

## ウィンドウルール

| niri の機能 | 使用 | Contrast の今 | 必要な変更 | 規模 |
|---|---|---|---|---|
| `window-rule { match app-id … }` | はい（wezterm の `default-column-width {}` は実質何もしない。keepassxc のルールは無効化） | ○ `windowOpen` フック（app-id・title で判定） | — | — |
| `open-on-workspace` | いいえ | ○（フックの `workspace`） | — | — |
| `open-floating` | いいえ | ○（フックの `floating`） | — | — |
| `open-fullscreen` | いいえ | ○ **N2 で実装**（フックの `Placement.fullscreen`。配置後に `ToggleFullscreen`） | — | — |
| `default-column-width`（ルール） | はい（空） | × 列の幅が無い | N3 | L |
| `block-out-from "screen-capture"` | 無効 | × | screencopy が先 | L |

## 集計（上の表の行数）

| 区分 | ○ | △ | × |
|---|---|---|---|
| 見た目 | 5 | 5（うち gaps・struts・shadow・角丸の 4 つを N1 で実装） | 1 |
| レイアウト（N3b 後） | 13 | 0 | 1 |
| ワークスペース | 2 | 0 | 6 |
| 操作 | 5 | 1 | 3 |
| 入力 | 4 | 0 | 0 |
| ウィンドウルール | 4 | 0 | 2 |
| **合計（55 行）** | 33 | 6 | 16 |

×の主なもの: 列モデル（列の幅・プリセット・積み・ビューのスクロール）、動的・名前つきワークスペース、複数出力とスケール、
オーバービュー、スクリーンショット、ホイールのバインド。キー割り当てのうち、代用（△）で済ませているのは
`examples/niri.cdl` の J/K/Up/Down の 8 つ。

## 実装順の提案

使っているものを先に、core に触らないものを先にする。

- **N1（今回）**: 見た目。`gap`・`outerGap`、角丸、影、非フォーカスの枠の幅、`Cursor`。`examples/niri.cdl` を niri の config に寄せる。
- **N2（済み。窓の開閉の補間を除く）**: 使っているのに無いもの（core に触らない）。`spawn-at-startup` と `environment`（CDL の `Startup` / `Environment`）、`open-fullscreen`、`focus-follows-mouse`、`cursor.hide-after-inactive`。`focus-column-first/last` の代用は、N3a で core の action（`focus-column-first/last`）が入ったので不要。アニメーション（矩形の補間。窓の移動・サイズ・ワークスペース切替）は `Layout.animationMs` で実装した。窓の開閉の補間は未対応。
- **N3a（済み）**: core のスクロール列レイアウト（列幅・積み・ビュー）とコンポジタの配線。**N3b（窓の高さと `center-column`）も済み**。残りは `consume-or-expel-window-left/right` とビューのアニメーション。
- **N3（元の提案）**: core のレイアウト（列モデル）。列の幅（preset・default・set・maximize・center）、列への積み（consume/expel、focus/move-window-up/down、高さ）、ビューの横スクロール。これで `examples/niri.cdl` の「N3 待ち」が解ける。
- **N4**: ワークスペースとモニタ。動的な縦のワークスペース、`move-workspace-*`、名前つき、複数出力・ホットプラグ・スケール・モード指定、フォーカスとウィンドウのモニタ間移動。
- **N5**: オーバービュー、スクリーンショット（と screencopy）、ホットキーのオーバーレイ、ホイールのバインド、session-lock。

## 2026-11-07 niri-parity（`proto/niri-parity`）での変更

作者の config.kdl の項目ごとの差は `.cc-logs/report-niri-parity-status.md`（調査）と `.cc-logs/report-niri-parity.md`（結果）。niri の挙動は、インストール済みの niri（26.04、nixpkgs の `niri-spicy`）のソースと同梱ドキュメントで確かめた。

- ○ に変えたもの: ホイールの bind 12 個（`cooldown`）、`consume-or-expel-window-left/right`（core の P9、`core-spec.md` の 15 節）、キーリピート 600ms、xdg-decoration を公開しない・Tiled を送らない（`prefer-no-csd` が無効）、子の `XCURSOR_THEME`・`XCURSOR_SIZE`・`XDG_CURRENT_DESKTOP`・`XDG_SESSION_TYPE`、タッチパッドの dwt off、フォーカスリングを窓の外側に描く・フォーカス外の窓に枠を描かない。
- 差が残るもの（理由は報告）: 動的なワークスペース（常に 9 個）、`move-workspace-down/up`、複数モニタ、スクリーンショット、ホットキーのオーバーレイ、`power-off-monitors`、アニメーション（バネと窓の開閉）、ジェスチャーとホットコーナー、`dwtp`、xkb を localed から取ること、`move-column-to-workspace` が列ごとでないこと、`expel-window-from-column` の細部（`core-spec.md` の D61）。

## 2026-11-07 niri-dynws（`proto/niri-dynws`）での変更

動的なワークスペース（`Workspaces mode="dynamic"`、core-spec 16 節）、`move-column-to-workspace`（列ごと）、`move-workspace-down/up` を実装した。`config/` は `dynamic`。調査は `.cc-logs/report-niri-dynws-status.md`。

- ○ に変えたもの: ワークスペースの数（窓のあるものと末尾の空 1 つ。ironbar のボタンも同じ数）、`focus-workspace N` の範囲外（末尾の空へ）、離れた空のワークスペースの整理、`move-column-to-workspace`（列ごと）、`move-workspace-down/up` の 4 個。
- 差が残るもの: 離れた空のワークスペースの削除が、切り替えのアニメーションの終わりではなく直後（IPC は `WorkspaceActivated` → `WorkspacesChanged` の順で、niri と同じ）。複数モニタ（モニタごとのワークスペースの列）は未対応。
