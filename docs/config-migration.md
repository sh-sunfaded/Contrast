# niri の設定から Contrast の設定への移行

`~/.config/niri/config.kdl`（作者の実機の設定）を、CDL で再編した設定が `config/` にある。`~/.config/contrast/` にコピーして使う
（`CONTRAST_CONFIG=<config/config.cdl のパス>` で別の場所も指せる。`import` は設定ファイルのあるディレクトリの中だけ）。

## 構成

| ファイル | 内容 |
| --- | --- |
| `config/config.cdl` | 入口。ほかのモジュールを import して `Contrast` の木を組み立てる |
| `config/theme.cdl` | 色・gap・幅・アニメーションの長さ・ワークスペースの数などの定数 |
| `config/appearance.cdl` | `Appearance`・`Layout`・`Cursor`・`Workspaces` |
| `config/input.cdl` | `Input`（touchpad の tap と natural-scroll） |
| `config/startup.cdl` | `Startup`（spawn-at-startup）と `Environment`（environment） |
| `config/keybinds.cdl` | キーバインド。方向・ワークスペースの繰り返しは、データから生成する |
| `config/unsupported.cdl` | 変換できない binds（元の KDL の行を `//>` で残す。コメントだけ） |

`mod` は `modKey`（実機の DRM は Super、nested の winit は Alt）に任せている。`Mod+Alt+J`・`Mod+Alt+E` は、`mod` が Alt のとき `Mod+J`・`Mod+E` と同じキーに
なって重複するので、`super+alt+j`・`super+alt+e` と書いている（実機の Super では同じ）。`Super+Alt+L` は元から Super と書かれている。

## binds 111 個の内訳

元の `binds { … }` は 111 行。**変換したもの 109、変換しなかったもの 2**（109 + 2 = 111）。
`keybinds.cdl` には、これに niri の既定の `Mod+V`（`toggle-floating`。作者の config.kdl の binds には無い）を足して 110 個ある。
`crates/contrast-policy-cdl/tests/impl_config.rs` が、この数（`keybinds.cdl` の 110 と、`unsupported.cdl` の `//>` の 2）と、キーの重複がないことを確かめる。
`impl_binds_parity.rs` が、`config/` と基準の `examples/niri.cdl` で、同じキーに同じ操作が割り当てられていること（`mod` が Super でも Alt でも）を確かめる。

### (a) 変換したもの（109 個。niri の既定の `Mod+V` を足して 110 個）

| 区分 | 個数 | niri の action → CDL の action |
| --- | --- | --- |
| プログラムの起動 | 12 | `spawn` / `spawn-sh` → `spawn`（`command`）。firefox-nightly・alacritty・fuzzel・fcitx5-remote -t・wpctl の 4 つ・jetbrains-toolbox（`env DISPLAY=:0 …`。XWayland が要る）・`strat fedora …` の 2 つ（Discord、Eclipse）・`swaylock`。`strat`（Bedrock Linux のコマンド）と `swaylock` は、この NixOS には無いので、押すと spawn が失敗してログに出るだけ |
| 窓を閉じる・終了 | 2 | `close-window` → `close`、`quit` → `quit` |
| フォーカスの移動 | 8 | `focus-column-left/right` → `focus-left/right`、`focus-window-up/down` → `focus-up/down`（矢印と hjkl の 2 通り × 4 方向） |
| 窓・列の移動 | 8 | `move-column-left/right` → `move-left/right`、`move-window-up/down` → `move-up/down`（同上） |
| 列の端 | 4 | `focus-column-first/last` → `focus-column-first/last`、`move-column-to-first/last` → `move-column-first/last` |
| ワークスペース N | 18 | `focus-workspace N` → `workspace`（`workspace`）、`move-column-to-workspace N` → `move-column-to-workspace`（**列ごと**移して切り替える。N = 1〜9）。動的なワークスペースでは、存在しない番号は末尾の空のワークスペースになる |
| ワークスペースの上下 | 8 | `focus-workspace-down/up` → `workspace-next/prev`、`move-column-to-workspace-down/up` → `move-column-to-workspace-next/prev`（Page_Down/Up と U/I の 2 通り） |
| 列への積み | 4 | `consume-window-into-column`、`expel-window-from-column`、`consume-or-expel-window-left/right`（同名）。`expel-window-from-column` は niri と細部が違う（下記） |
| ワークスペースの入れ替え | 4 | `move-workspace-down/up`（Mod+Shift+Page_Down/Up と U/I）。動的なワークスペースのみ |
| ホイール | 12 | `Mod+WheelScroll…` → `workspace-next` などと同じ action を、キー名 `WheelScrollDown/Up/Left/Right` に割り当てる。縦のワークスペースの 4 つは `cooldown={150}`（niri の `cooldown-ms=150`） |
| 幅・高さのプリセット | 3 | `switch-preset-column-width`（同名）、`switch-preset-window-height`（同名）、`reset-window-height`（同名） |
| 列・窓の状態 | 3 | `maximize-column`（同名）、`fullscreen-window` → `toggle-fullscreen`、`center-column`（同名） |
| （niri の既定） | 1 | `Mod+V` → `toggle-floating`。作者の binds には無いが、`examples/niri.cdl` が niri の既定どおり持つので合わせた |
| 複数モニタ | 16 | `focus-monitor-*`（8）と `move-column-to-monitor-*`（8）。同じ名前の action（`Mod+Shift+矢印/HJKL`、`+Ctrl` で列の移動） |
| スクリーンショット・画面 | 3 | `screenshot-screen`（Ctrl+Print）・`screenshot-window`（Alt+Print）・`power-off-monitors`（Mod+Shift+P）（同名） |
| 幅・高さの増減（±10%） | 4 | `set-column-width "±10%"` → `adjust-column-width`（`delta`）、`set-window-height "±10%"` → `adjust-window-height`（`delta`） |

- 音量キー（`allow-when-locked=true`）の `allow-when-locked` は移さない（Contrast にロック画面が無い）。
- キーは `Mod+Q` → `mod+q`、`Mod+Ctrl+Home` → `mod+ctrl+Home` のように、`+` で区切り、修飾キーを小文字にする（キー名の大文字小文字は問われない）。

### (b) 変換しなかったもの（2 個）

| 待っている機能 | 個数 | 内容 |
| --- | --- | --- |
| スクリーンショット | 1 | `screenshot`（範囲を選ぶ UI。`screenshot-screen`・`screenshot-window`・`screenshot-path` は移した） |
| オーバーレイ | 1 | `show-hotkey-overlay` |

元の KDL の行は `config/unsupported.cdl` に、待っている機能ごとに残してある。

## binds 以外の設定

| niri | CDL | 備考 |
| --- | --- | --- |
| `spawn-at-startup`（5 つ） | `Startup`（この順） | fcitx5・ironbar・swaybg・mako・udiskie。ironbar だけ `ipc="niri"`（下記） |
| `environment`（4 つ） | `Environment` / `Env` | QT_IM_MODULE・GTK_IM_MODULE・SDL_IM_MODULE（fcitx5）、MOZ_ENABLE_WAYLAND（1） |
| `layout { gaps 16 }` | `Layout.gap`・`outerGap` | |
| `default-column-width { proportion 0.5; }`、`preset-column-widths` | `Layout.defaultColumnWidth`・`presetColumnWidths` | 0.33333 0.5 0.66667 |
| スクロールするレイアウト | `Layout.mode="scrolling"` | |
| `animations`（既定） | `Layout.animation="spring"`（niri の既定のバネ: 減衰比 1.0、stiffness 800／ワークスペース切替は 1000）と `Layout.animationMs`（0 で無効） | 150 |
| `focus-ring`（width 4、active #7fc8ff、inactive #505050） | `Appearance.borderWidth`・`focusColor`・`borderColor` | |
| `cursor`（Cecilia、24） | `Cursor.theme`・`size` | |
| `input.touchpad`（tap、natural-scroll） | `Input.tapToClick`・`naturalScroll` | |
| `workspace`（Mod+1〜9） | `Workspaces.count` | 9 |
| `border`（off）、`output`（無効）、`window-rule`（wezterm は何もしない、keepassxc は無効） | 移さない | `screenshot-path` は `Screenshot.path` に移した |
| `input.keyboard`（xkb は空）、`input.mouse`（空）、`struts`（空）、`hotkey-overlay` | 移さない | 設定が空 |

## ironbar の workspaces（`ipc="niri"`）

ironbar の workspaces モジュールは、環境変数 `NIRI_SOCKET` があると niri だとみなし、そのソケットに niri の IPC で接続する。
Contrast は、そのうち ironbar が使う最小の部分（`EventStream`、`FocusWorkspace`、`Workspaces`）を、`$XDG_RUNTIME_DIR/contrast.<pid>.sock`（0600）で話す
（計画と範囲は `docs/ipc-plan.md`）。

- `config/startup.cdl` の ironbar の `Startup` に `ipc="niri"` を付けてある。付けた子にだけ、コンポジタが `NIRI_SOCKET=<そのソケットのパス>` を環境に足す。
  ほかの子（ほかの `Startup` と `Keybind` の `spawn`）には渡さない（ホストから継承した `NIRI_SOCKET` も外す）。niri 対応のほかのアプリが、Contrast を niri と誤認しないため。
- ワークスペースは動的（`Workspaces mode="dynamic"`）。`idx` は位置 + 1、`id` は作った順の通し番号で位置と無関係（消えても再利用しない）。名前は無い（ironbar は番号を出す）。バーのボタンは、窓のあるワークスペースと末尾の空 1 つ（niri と同じ）。`Super+1〜9` や `workspace-next` の切り替えは、ironbar に `WorkspaceActivated` として伝わる。
  ボタンのクリックは `FocusWorkspace` として届き、キーと同じ切り替えになる。
- 出力は 1 枚だけの想定。隠れたワークスペースの出力は主出力として伝える。
- IPC が起動できなかったとき（`XDG_RUNTIME_DIR` が無い・同じパスのソケットがある）は、エラーをログに出して IPC なしで続ける。このとき `ipc="niri"` の子にも `NIRI_SOCKET` は渡らず（警告が出る）、ironbar の workspaces は使えない。
- ironbar のクリップボード（clipboard）モジュールが使う wlr-data-control は、設定を書かなくても、既定の許可リスト（ironbar・wl-copy・wl-paste・cliphist・wl-clip-persist）のクライアントにだけ公開される（`DataControlClient` で変えられる）。

## 動的なワークスペース（`mode="dynamic"`）

`config/` は `<Workspaces count={9} mode="dynamic" />`。niri のように、ワークスペースは窓のあるものと末尾の空 1 つだけで、空の状態から始まる（`core-spec.md` の 16 節）。
`count` は、`workspace` の番号の上限（`Mod+1〜9` の検査）にだけ使う。ワークスペースの数そのものではない。

- 窓がいちばん下のワークスペースに入ると、その下に新しい空のワークスペースができる。
- ワークスペースを離れて切り替えると、窓も名前もない（末尾以外の）ワークスペースが消える。窓を閉じただけでは消えない（表示中のあいだは残る）。
- `focus-workspace N` で存在しない番号は、末尾の空のワークスペースになる。`move-column-to-workspace N` も同じ（列ごと移して、そこへ切り替える）。
- `move-workspace-down/up` は、表示中のワークスペースを 1 つ下／上と入れ替える。
- niri との差: niri は切り替えのアニメーションが終わるまで、離れた空のワークスペースを残す。Contrast は切り替えの直後に消す。IPC は、niri と同じ順序（`WorkspaceActivated` → `WorkspacesChanged`）で送る。
- 起動時の方式（`fixed` か `dynamic`）は、リロードでは変わらない（変えると警告 CT050）。

## 繰り返しの減らし方

`config/keybinds.cdl` は、次のデータ 1 組ずつから、複数の binds を生成する（CDL の `const`・関数・配列の `reduce`/`concat`・`map`・テンプレート文字列を使う）。

- 方向 4 組（矢印、hjkl、focus、move）→ 16 個
- ワークスペースの上下 2 組（Page_Down/Up、U/I、focus、move）→ 8 個
- ワークスペースの番号 1〜9 → 18 個

起動と、1 つずつの操作は、そのままの表（データ）で書く。元の binds は 111 行（変換するものだけで 72 行、`Mod+V` を足して 73 行）。`keybinds.cdl` は、コメントと空行を除いて 117 行
（import、型の宣言、データ、生成の関数、最後の `map` を含む）。ファイル全体は 155 行。
