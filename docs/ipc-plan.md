# ironbar の workspaces を動かすための niri IPC の最小実装: 調査と計画

状態: 計画（コードは書いていない）。ブランチ `proto/ipc-plan`。
対象: Contrast のコンポジタ（`crates/contrast-compositor`）。core・policy の公開 API は、下記の「CDL の変更」を除いて変えない。

## 1. 調べたこと（根拠）

### 1.1 ironbar が niri に接続する方法

- 対象は、実機にある ironbar 0.19.1（`/nix/store/in0ah128qdnp1jb3kl776ms59c8lx3jw-ironbar-0.19.1`）。GitHub `JakeStanger/ironbar` のタグ `v0.19.1` のソースを読んだ。
- **`niri-ipc` クレートは使っていない**。`src/clients/compositor/niri/connection.rs` の先頭に「`niri_ipc` から必要な部分だけを抜き出した」とあり、型は ironbar が自前で持つ（`serde` + `serde_json`、feature `niri`）。
- **コンポジタの種類の判定は、環境変数の有無だけ**（`src/clients/compositor/mod.rs` の `get_current`）。順に `SWAYSOCK` → `HYPRLAND_INSTANCE_SIGNATURE` → `NIRI_SOCKET`。`XDG_CURRENT_DESKTOP` は見ない。どれも無ければ `Unsupported`（実機ログの `workspaces is unsupported by compositor` はこれ）。
- 接続: `NIRI_SOCKET` のパスに `tokio::net::UnixStream::connect`（`Connection::connect`）。
- 送り方（`Connection::send`）: `serde_json::to_string(&request)` を**改行なしで** `write_all` し、すぐ `shutdown()`（書き込み側だけ閉じる）。そのあと `read_line` で返事を 1 行読む。`Request::EventStream` のときは、同じ接続の読み側で、`read_line` を繰り返してイベントを読む。
- 使う Request（ironbar の `Request` は 2 つだけ）:
  | Request | JSON（serde の外部タグ） | 使う場所 |
  | --- | --- | --- |
  | `EventStream` | `"EventStream"` | `Client::new`。起動時に 1 接続（ワークスペースのモジュール 1 つにつき 1 つ） |
  | `Action(FocusWorkspace { reference: Id(u64) })` | `{"Action":{"FocusWorkspace":{"reference":{"Id":3}}}}` | ボタンのクリック。クリックのたびに別の接続を作って送る |
  - `WorkspaceReferenceArg::Name(String)` も型にはあるが、`focus` は `Id` しか送らない。
- 使う Reply: `Result<Response, String>`（`{"Ok":"Handled"}` または `{"Err":"..."}`）。`Response` は `Handled` と `Workspaces(Vec<Workspace>)` の 2 つだけ。ironbar は**どちらの返事の中身も使わない**（`EventStream` の返事は読み捨て、`FocusWorkspace` の返事は読むだけ）。ただし、返事の 1 行目が `Reply` として JSON にパースできないと、`send` がエラーになり、接続の処理が終わる（`?`）。
- 購読する Event（ironbar の `Event`）:
  | Event | JSON | 使い方 |
  | --- | --- | --- |
  | `WorkspacesChanged { workspaces }` | `{"WorkspacesChanged":{"workspaces":[...]}}` | 最初の 1 つで `Init`（id 昇順に並べ替える）。以降は、前の状態との差分から Add / Rename / Move / Remove を作る |
  | `WorkspaceActivated { id, focused }` | `{"WorkspaceActivated":{"id":2,"focused":true}}` | `focused:true` はフォーカスの移動（`Focus { old, new }`）。`false` は、その出力で表示中のワークスペースが変わっただけ |
  | `WorkspaceUrgencyChanged { id, urgent }` | `{"WorkspaceUrgencyChanged":{"id":2,"urgent":true}}` | `Urgent`（見た目のクラス） |
  | `Other` | （上記以外のすべて） | 行が `Event` にパースできなければ `Other` として捨てる（`unwrap_or(Event::Other)`）。**未知のイベントは送ってよい** |
- 使う型（ironbar 側の `Workspace`。`Serialize + Deserialize`、`deny_unknown_fields` なし）:
  `id: u64`、`idx: u8`、`name: Option<String>`、`output: Option<String>`、`is_active: bool`、`is_focused: bool`。niri 本物の `Workspace` には、さらに `is_urgent: bool` と `active_window_id: Option<u64>` があるが、ironbar は読まない（送っても無視される）。
  - ironbar 内部への変換: `name` が `None` なら `idx` の数字を名前にする。`monitor = output`（`None` なら空文字）。`is_active` なら `Visible { focused: is_focused }`、そうでなければ `Hidden`。
- 読み捨てる点（実装の制約になる）:
  - 受け側は、`read_line` が 0 バイト（EOF）を返しても、**エラーにせず `Event::Other` として次の読みに進む**（`unwrap_or(0)` と `unwrap_or(Event::Other)`）。つまり、サーバーが接続を閉じると、ironbar は**そのスレッドで空転する**（CPU を使い続ける）。コンポジタは、ironbar が生きているあいだ、イベントの接続を閉じてはいけない。
  - 送り側が書き込みを `shutdown` するので、サーバーは「書き込み側の EOF のあとも、イベントを書き続ける」必要がある。niri も同じ（`handle_client` は `read_until(b'\n')` が EOF で返っても、そのバッファを 1 行として処理する）。
- 利用者の設定（`~/.config/ironbar/niri-v8.toml`）の workspaces モジュール: `type = "workspaces"` と `all_monitors = false` だけ。`all_monitors = false` のとき、バーの出力の名前（Wayland の `wl_output` の name。実機は `eDP-1`）と、ワークスペースの `output` が**同じ文字列のもの**だけを表示する（`modules/workspaces/mod.rs` の `w.monitor == output_name`）。名前の対応表（`name_map`）などは設定していないので、既定の見た目: ボタンにワークスペースの名前（名前が無ければ idx の数字）を出し、`Visible` のボタンにアクティブの見た目、フォーカス中のものにフォーカスの見た目。
- 設定のうち、ほかに niri の IPC を使うモジュールは無い（`grep` で、`niri` が出るのは `clients/compositor/` だけ。`launcher` と `focused` は wlr-foreign-toplevel、`clipboard` は wlr-data-control を使う）。

### 1.2 niri の IPC（`niri-ipc`、niri のリポジトリ）

出典: `YaLTeR/niri` の `niri-ipc/src/lib.rs`・`niri-ipc/src/socket.rs`・`src/ipc/server.rs`（main）。

- ソケット: Unix ドメインのストリーム。パスは環境変数 `NIRI_SOCKET`。niri 本体は `$XDG_RUNTIME_DIR/niri.<WAYLAND_DISPLAY>.<pid>.sock` に作る（`IpcServer::start`）。パーミッションは明示せず、`$XDG_RUNTIME_DIR`（0700）に頼る。
- メッセージ: JSON を 1 行 1 メッセージ（改行区切り）。リクエストは「改行で終えて flush」か「flush して書き込み側を `shutdown`」のどちらでもよい（`lib.rs` の冒頭のドキュメント）。サーバーは `read_until(b'\n')` で読むので、EOF で終わる最後の 1 行も受け付ける。
- 返事: リクエスト 1 つにつき `Reply` が 1 行。`Reply` は `Result<Response, String>`（serde の外部タグで `{"Ok":...}` / `{"Err":"..."}`）。解析できない行や処理に失敗したときは `{"Err":"<メッセージ>"}`。
- `Request::EventStream`: 返事は `{"Ok":"Handled"}`。そのあと niri はそのソケットのリクエストの読み込みをやめ、**最初に現在の状態を全部イベントで送り**（ワークスペースは `WorkspacesChanged`。ドキュメントに「最初のワークスペース関連のイベントは `WorkspacesChanged` で完全な状態」とある）、以降は変化のたびに 1 行ずつ送る。
- イベントの送り先が遅いとき（バッファ 64 件）、niri はその接続を切る（`send_event`）。Contrast もこれに合わせる。
- niri の `Workspace`: `id`（変わらない一意の数。1 から始まるとは限らない）、`idx`（その出力の中の位置。1 始まり）、`name`、`output`、`is_urgent`、`is_active`（出力ごとに 1 つ）、`is_focused`（全体で 1 つ）、`active_window_id`。

### 1.3 ironbar の workspaces に必要な最小の一覧

- 受ける Request: `EventStream`、`Action::FocusWorkspace { reference: Id(u64) }`。それ以外のリクエストは、`{"Err":"unsupported request"}` を返す。
- 送る Reply: `{"Ok":"Handled"}`（`EventStream` と `FocusWorkspace` の両方）。失敗は `{"Err":"..."}`。
- 送る Event: `WorkspacesChanged`（接続の直後に完全な状態、以降は一覧が変わったとき）、`WorkspaceActivated`（表示中・フォーカス中のワークスペースが変わったとき）。`WorkspaceUrgencyChanged` は、Contrast に urgent の概念が無いので送らない。
- 型: `Workspace { id, idx, name, output, is_urgent, is_active, is_focused, active_window_id }`（ironbar が読むのは前半の 6 つ。残りは niri 互換のために固定値で送る: `is_urgent=false`、`active_window_id=null`）。`Window` の型・イベントは要らない。

### 1.4 Contrast 側で作れるか（core の公開 API で足りるか）

- core のワークスペースは**全出力で共有**され、数は増減しない（`domain/workspace.rs`、core-spec D1）。インデックスは 0 始まり（`WorkspaceIndex`）。コンポジタのログと CDL の `Keybind workspace` は 1 始まりで表示している。
- `StateView`（`application/view.rs`）で読めるもの: `workspace_count()`、`shown_workspace(output) -> Option<WorkspaceIndex>`、`workspace_windows(index)`、`focus()`、`window(id)`（`workspace` を持つ）、`output_id(key)`。
  - 一覧は `0..workspace_count()` から作れる。**`id` と `idx` は `index + 1` で足りる**（ironbar は id を一意の識別子とボタンの対応づけにだけ使い、`FocusWorkspace` で同じ id を返してくる。Contrast は `id - 1` で `WorkspaceIndex` に戻せる）。`WorkspaceId`（core の内部 ID）を出す必要はない。
  - `is_active`: `shown_workspace(output) == Some(index)` の出力があるか。`is_focused`: フォーカス出力が表示しているもの。現在の Contrast は出力が 1 つなので、その出力のものと同じ。
  - `output`: 表示している出力の名前（`Output::name()`。実機は `eDP-1`）。表示していない（隠れた）ワークスペースは、**主出力**（先頭の出力）の名前にする（共有モデルでは持ち主がいないため）。
  - `name`: Contrast にワークスペース名は無いので `None`（ironbar は `idx` の数字を出す）。
  - `active_window_id`: core はワークスペースごとのフォーカス履歴を持つが、`StateView` には出ていない。ironbar は読まないので `null`。要るなら、あとで `StateView` に足す（今回は足さない）。
- 結論: **core の公開 API の変更は要らない**。`StateView` と、コンポジタが持つ出力の名前で足りる。
- 切り替え: `FocusWorkspace { Id(n) }` は、キーバインドと同じ道（`core_action(contrast_core::Action::SwitchWorkspace(WorkspaceIndex::new(n - 1)))`、`input/bindings.rs`）で実行できる。範囲外は無視して `{"Err":...}` を返す（バインドは範囲外を無視する）。
- 状態の変化の集約点: `shell.rs` の `submit(Command)`。キー、IPC、窓の出入りのどれでも、core の状態の変化はここを通る。そこで `Effect::ShowWorkspace`（表示ワークスペースの切り替え）が出る。

## 2. 設計

### 2.1 置き場所と層

- `crates/contrast-compositor/src/ipc/`（新規モジュール）。コンポジタの中だけに置く。core・policy には依存を足さない（依存の向きは `compositor → policy → core` のまま）。
  - `ipc/protocol.rs`: **純粋**。リクエスト 1 行 → `Request`（`EventStream`、`FocusWorkspace(id)`、`Unsupported`）、`Reply`・`Event` → 1 行の JSON。`WorkspaceInfo`（id・idx・name・output・is_active・is_focused）の並びから `WorkspacesChanged` などを作る。I/O も Smithay も知らない。単体テストの中心。
  - `ipc/state.rs`: `Contrast` から `Vec<WorkspaceInfo>` を作る（`StateView` と出力の名前）。直前に送った一覧との差分を取り、送るべき `Event` を決める（一覧が変われば `WorkspacesChanged`、表示・フォーカスだけなら `WorkspaceActivated`）。純粋な差分の関数に切り出す。
  - `ipc/server.rs`: calloop の `Generic<UnixListener>` と、接続ごとの `Generic<UnixStream>`。行バッファ、書き込みキュー（ノンブロッキング）、切断の処理。
- 呼び出し: `submit` の最後（と、出力の追加・削除、リロードでワークスペース数が変わるとき）で `self.ipc_sync()` を呼ぶ 1 か所。`ipc_sync` は一覧を作り直して差分を配る。
- JSON の実装: serde + serde_json（下記の確認事項 1）。

### 2.2 ソケット

- パス: `$XDG_RUNTIME_DIR/contrast.<pid>.sock`（`XDG_RUNTIME_DIR` が無ければ IPC を起こさない。警告のログ）。DRM でも winit でも同じ。
- 作る前に、同じパスがあれば作らない（pid つきなので普通は無い）。`bind` のあとで `chmod 0600`。親のディレクトリ（`$XDG_RUNTIME_DIR`）は 0700。
- 終了時に `unlink`（`Drop`）。異常終了で残ったソケットは、次の起動で、**pid が生きていないものだけ**を（任意）掃除する。必須にはしない。
- `listening:` のログと同じ形で、`ipc: socket=<パス>` をログに出す。
- 受け付けるのは、**同じ UID だけ**（`SO_PEERCRED` は std で取れない。0600 と 0700 の親で足りるとみなす。今回は確認しない）。

### 2.3 `NIRI_SOCKET` を誰に渡すか

ironbar が使う判定は `NIRI_SOCKET` の有無だけなので、渡した子は、Contrast を niri だと思う。niri 対応のほかのアプリ（`niri msg`、waybar の niri モジュール、`niri-session` 系のスクリプトなど）が、全部の子に渡った `NIRI_SOCKET` を見て、Contrast に niri のリクエストを送る可能性がある。Contrast は上の最小のリクエストしか処理せず、それ以外には `{"Err":"unsupported request"}` を返す。

選択肢:
- A. すべての子に `NIRI_SOCKET` を渡す。実装は最小だが、誤認の範囲が最大。
- B（推奨）. すべての子に `CONTRAST_SOCKET`（同じパス）を渡し、`NIRI_SOCKET` は、**CDL で指定した子だけ**に渡す。`Startup` に省略できる prop を足す: `<Startup command="ironbar -c ... -t ..." ipc="niri" />`。`ipc` は今は `"niri"` だけ。そのコマンドの環境に `NIRI_SOCKET=<パス>` を足す（利用者が `Environment` に同名を書いていても、パスが実行のたびに変わる pid つきなので、コンポジタの値が勝つ）。
- C. CDL の `Environment` に書く。パスに pid が入るので、固定の値では書けない（`${...}` の展開は CDL の `Environment` には無い）。パスを pid なしの固定名（`contrast.sock`）にするなら可能だが、複数起動や古いソケットの扱いが難しくなる。

B は、CDL（`Startup`）と contrast-policy（`Settings.startup` を `Vec<String>` から、コマンドと `ipc` のフラグの組にする）の変更を伴う。変更は小さいが、`Policy::startup_commands` の型が変わる（公開 API の変更。確認事項 2）。
キーバインドの `spawn` には `NIRI_SOCKET` を渡さない（`CONTRAST_SOCKET` だけ）。

### 2.4 イベントの配り方

- 接続ごとに、`Request::EventStream` を受けたら: `{"Ok":"Handled"}` を書き、続けて現在の完全な状態（`WorkspacesChanged`）を書く。そのあと、その接続を「購読者」の一覧に入れる。購読者の接続は、**サーバーからは閉じない**（ironbar の読み側が EOF で空転するため。1.1）。リクエストの読み込みは止める（niri と同じ）。
- 状態の変化は `ipc_sync` が、前回送った一覧と比べて決める:
  - ワークスペースの一覧（数・id・idx・name・output）が変わった → `WorkspacesChanged`（完全な状態）。
  - 一覧は同じで、`is_active` / `is_focused` だけが変わった → `WorkspaceActivated { id, focused }`（表示が切り替わったワークスペースごとに 1 つ。`focused:true` のものを最後に）。
  - 何も変わらなければ何も送らない（窓の出入りやフォーカスの移動のたびに送らない）。
- 書き込みはノンブロッキング。送れなかった分は接続ごとのバッファに溜め、64 行を超えたら切断する（niri と同じ。ログに警告）。
- `Request::EventStream` 以外の通常のリクエストの接続は、1 件ずつ返事を書き、相手が閉じたら閉じる。

### 2.5 対応するリクエスト（最小）

| Request | 返事 |
| --- | --- |
| `EventStream` | `{"Ok":"Handled"}` のあとにイベント |
| `Action(FocusWorkspace { reference: Id(n) })` | 範囲内なら実行して `{"Ok":"Handled"}`。範囲外は `{"Err":"workspace not found"}` |
| `Action(FocusWorkspace { reference: Name(_) })` | `{"Err":"workspace names are not supported"}` |
| `Workspaces` | `{"Ok":{"Workspaces":[...]}}`（`niri msg workspaces` の確認用。安い） |
| それ以外（解析できないものを含む） | `{"Err":"unsupported request"}` |

## 3. フェーズ

### P1: プロトコル（純粋）
- 内容: `ipc/protocol.rs`。リクエストの解析、`Reply`・`Event` の整形、`WorkspaceInfo` からの JSON。
- 終了条件: 1.1 の表の JSON をそのまま入れたゴールデンテストが通る（ironbar が送る 2 つのリクエスト、ironbar が読む 3 つのイベントと `Workspace` の形。余分なフィールドを足しても ironbar の型で読める）。EOF で終わる改行なしの 1 行を扱える。
- 自動確認: `cargo test -p contrast-compositor ipc::`、clippy。
- 実機: なし。

### P2: 状態からの一覧と差分（純粋）
- 内容: `ipc/state.rs`。`StateView` と出力の名前から `Vec<WorkspaceInfo>`。差分の関数（前の一覧、新しい一覧 → 送るイベント）。
- 終了条件: 切り替え・ワークスペース数の変化・出力の追加で、期待のイベントが出る。変わらなければ何も出ない。隠れたワークスペースの `output` が主出力になる。
- 自動確認: core を組み立てる既存のテストの作り方（`shell.rs` の `core.apply(Command::OutputAdded …)` のテスト）に合わせた単体テスト。
- 実機: なし。

### P3: サーバー（calloop）
- 内容: `ipc/server.rs`。ソケット（0600）、接続ごとの行の読み込み、購読者への書き込み、バッファの上限、`Drop` で unlink。`ipc_sync` を `submit` に 1 行足す。`FocusWorkspace` を `core_action` に配線する。
- 終了条件: nested（winit）で、`python3`（標準）で書いた疑似クライアントが、ironbar と同じ手順（改行なしのリクエスト → `shutdown(SHUT_WR)` → 読む）で、返事と `WorkspacesChanged` を受け取る。キーでワークスペースを切り替えると `WorkspaceActivated` が届く。疑似クライアントの `FocusWorkspace` で、コンポジタのワークスペースが切り替わる。接続を閉じてもコンポジタが止まらない。パーミッションが 0600。
- 自動確認: 上の疑似クライアントのスクリプトを `scripts/` ではなく、`crates/contrast-compositor/tests/impl_ipc.rs` に Rust のテストとして置く（`UnixStream` の同期クライアントで足りる。ただしコンポジタの起動が要るので、サーバーの部分だけを calloop の `EventLoop` に載せて試す）。nested の実機に近い確認は、報告に結果を貼る手動の手順にする。
- 実機: なし（nested まで）。

### P4: 環境変数と CDL
- 内容: `CONTRAST_SOCKET` をすべての子に、`NIRI_SOCKET` を `Startup ipc="niri"` の子だけに渡す（`spawn.rs`）。CDL のスキーマ・変換・`docs/cdl-contract.md`・`config/startup.cdl`（ironbar の行に `ipc="niri"`）。
- 終了条件: 診断つきのテスト（不正な `ipc` の値は CT003）。`Startup` の環境のテスト（`ipc` なしの子には `NIRI_SOCKET` が無い）。
- 自動確認: policy・policy-cdl・compositor のテスト、clippy。nested で、`Startup` の `env` を使い、`ipc="niri"` のある子にだけ `NIRI_SOCKET` が出る。
- 実機: なし。

### P5: ironbar を実際に接続する
- 内容: 変更なし（結合の確認）。
- 終了条件: nested で、実機の設定（`niri-v8.toml`）の ironbar を、専用の D-Bus セッション（`dbus-run-session`。tty1 の niri セッションの ironbar と D-Bus 名が衝突するため）の中で起動し、`IRONBAR_LOG=debug` のログに `WorkspacesChanged:` が出て、`workspaces is unsupported by compositor` が出ない。Contrast のキーでワークスペースを切り替えると、ironbar のログに `WorkspaceActivated` が出る。疑似クライアントで ironbar の `FocusWorkspace` の送り方を再現し、切り替わる。
- 実機で見る（人間）: DRM で起動し、バーのワークスペースのボタンが 4 つ（設定の数）出て、現在のものが強調され、`Alt+<数字>`／`Super+<数字>` で強調が動く。ボタンをクリックして切り替わる。

## 4. 規模の見積もり（行数、テストを含む）

| 部分 | 本体 | テスト |
| --- | --- | --- |
| protocol.rs（serde_json 使用） | 約 150 | 約 150 |
| state.rs（一覧と差分） | 約 120 | 約 150 |
| server.rs（calloop、バッファ、切断） | 約 300 | 約 150 |
| 配線（`submit`、`lib.rs`、`spawn.rs`、`FocusWorkspace`） | 約 60 | 約 40 |
| CDL・policy（`Startup ipc`、契約、設定） | 約 80 | 約 80 |
| 合計 | 約 700 | 約 570 |

serde_json を使わず自前で書く場合は、protocol.rs が約 250 行に増える（リクエストの解析が主）。

## 5. リスク

- ironbar の読み側の空転（1.1）: 接続を閉じると CPU を使い続ける。コンポジタの終了のときは仕方ない（niri も同じ）。実行中に閉じない。
- ironbar は、`workspaces` のモジュール 1 つにつき 1 つの接続を作り、バーの数だけ増える（出力が増えても少数）。多くの接続を想定しない。
- D-Bus のセッションバスを共有すると、別の ironbar が動いている間は、バーが出ない（前回の調査）。IPC とは別の問題で、tty1 の niri を止めるか、専用のセッションで確かめる。
- 隠れたワークスペースの `output`: core は共有モデルなので、出力が複数のとき、ironbar の `all_monitors = false` では、ほかの出力のバーに隠れたワークスペースが出ない。プロトタイプ（出力 1 つ）では問題ない。複数出力は別の設計が要る。
- 出力の名前: `wl_output` の name と、IPC の `output` の文字列を、同じ `Output::name()` から作るので一致する（実機ログでは `eDP-1`、nested は `winit`）。
- ironbar のバージョンアップで、型や判定が変わる可能性（0.19.1 に固定して確認した）。
- `NIRI_SOCKET` を渡す子の誤認（2.3）。B で、渡す子を限る。
- 受け付けるリクエストを増やすと、niri との互換を期待されて、保守が増える。最小に保つ。

## 6. 確認事項（選択肢と推奨）

1. **JSON の実装**: (a) `serde` と `serde_json` を compositor の依存に足す（どちらも `Cargo.lock` に既にある。`serde_derive` も）。(b) 自前で書く（依存なし。リクエストの解析が約 100 行増え、誤りの余地が増える）。**推奨は (a)**（版を `=` で固定する）。依存の追加なので、確認する。
2. **`NIRI_SOCKET` の渡し方**: 2.3 の A / B / C。**推奨は B**。B は `Startup` の CDL と `contrast-policy` の `startup_commands` の型を変える（この作業の依頼の「policy は変えない」と食い違うので、実装の前に許可が要る）。許可できない場合の代わりは A（全部の子に渡す）か、`Environment` に固定名のソケット（C）。
3. **ワークスペースの `id` と `idx`**: どちらも 1 始まりの番号（`index + 1`）にする。**推奨はこのまま**（名前は `None`）。CDL でワークスペースに名前をつけたいなら、別の拡張になる。
4. **対応するリクエスト**: `EventStream`、`FocusWorkspace(Id)`、`Workspaces` だけ。`niri msg` の `version`・`outputs`・`windows` などは返さない。**推奨はこのまま**。
5. **複数出力のワークスペース**: プロトタイプは出力 1 つとみなし、隠れたワークスペースは主出力に置く。**推奨はこのまま**。
6. **実装の順序と範囲**: P1〜P5 をまとめて進めるか、P1〜P3（CDL の変更なし。ソケットのパスはログに出るので、`NIRI_SOCKET` を手で渡して ironbar を起動して試す）を先にするか。**推奨は P1〜P3 を先に**（ポリシーの変更の許可を待たずに進められる）。
