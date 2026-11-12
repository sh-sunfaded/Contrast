# 実機バックエンド（DRM / libinput / session）計画

状態: 調査と計画のみ。コードは未着手。Smithay は `=0.7.0`。Kani は使わない。

## 1. 前提と現状

### 1.1 リポジトリの状態（調査時点）
- ブランチ `core/p3-spec`、作業ツリーはクリーン。
- Insets: **core に入っている**（`Command::OutputInsetsChanged`、`domain/insets.rs`、core-spec 11 節）。コンポジタ側（`shell.rs`）は未使用。
- layer-shell: **入っていない**（`crates/contrast-compositor/src` に実装なし）。`docs/architecture.md` と `README.md` にも DRM / layer / backend の記述はない。
  - 前提: フェーズ 3 の「レイヤー面」は、layer-shell の実装が先に必要。無ければ P3 は窓と枠だけで終える。

### 1.2 現在のコンポジタ構成（winit 依存かどうか）
| 場所 | winit 依存 | メモ |
|---|---|---|
| `backend/winit.rs` | 依存 | 出力の生成、リサイズ→`output_changed`、Redraw |
| `render/mod.rs` | 依存 | `WinitGraphicsBackend<GlesRenderer>` を引数に取る。`.expect` 多用 |
| `lib.rs` | 依存 | `init_winit` 固定 |
| `input/mod.rs` | ほぼ非依存 | `process_input_event<I: InputBackend>` は汎用。ただし `PointerMotionAbsolute` だけ処理。相対移動 `PointerMotion` が無い。出力 1 枚前提（`space.outputs().next()`） |
| `state.rs` | 非依存 | `seat.add_keyboard(Default::default(), ..)` は xkb 設定なし |
| `shell.rs` | 非依存 | core ブリッジ。`output_added` / `output_changed` のみ。`output_removed` / `output_insets_changed` は無い |
| `handler/`, `wm.rs`, `input/bindings.rs` | 非依存 | |

## 2. モジュール配置と抽象

- 同じ `contrast-compositor` クレート内に置く。`backend/winit.rs` は開発用に残す。
- 新設（ディレクトリ名は選択名に合わせて `drm`）:
  ```
  backend/drm/mod.rs       起動、イベントループへの登録、終了タイマー
  backend/drm/session.rs  LibSeatSession、pause / resume
  backend/drm/device.rs    UdevBackend、DrmDevice、GbmDevice、EGL、DrmOutputManager、connector 走査
  backend/drm/output.rs    Output 作成、モード、render / queue_frame / VBlank
  backend/drm/input.rs     Libinput、LibinputInputBackend
  ```
- 依存の向き: `backend/*` → `state` / `shell` → `contrast-core`。core は DRM を一切知らない。backend 同士は互いを use しない。
- 共通面:
  - 出力: backend は `Contrast` の `output_added` / `output_changed` / `output_removed`（新設）/ `output_insets_changed`（新設）だけを呼ぶ。
  - 入力: `process_input_event<I: InputBackend>` を共有する（`PointerMotion` を追加）。
  - 描画: `render_frame` を「要素の組み立て」と「提出」に分ける。提出は winit が `submit`、DRM が `render_frame` + `queue_frame`。
- 起動選択（決定済み）: `CONTRAST_BACKEND=winit|drm`。**DRM は `drm` を明示したときだけ有効**。未指定のとき、`WAYLAND_DISPLAY` または `DISPLAY` があれば winit。無ければ「DRM を使うなら CONTRAST_BACKEND=drm を指定する」というエラーで終了する（SSH やサービスからの誤起動で seat を奪わないため）。
- 本体コードの規約（unsafe / unwrap / expect / panic 禁止）は DRM 側にも適用する。Smithay の `Result` は `CoreError` ではなく backend 専用のエラー型で受け、ログに出して続行する。

## 3. Smithay 0.7.0 の要点

### 3.1 feature
- 現在: `backend_winit, wayland_frontend, desktop`。
- 追加が必要: `backend_drm, backend_gbm, backend_udev, backend_libinput, backend_session_libseat, backend_egl, renderer_gl`（`backend_winit` が `backend_egl` / `renderer_gl` を既に引き込む）。
- 根拠: `smithay-0.7.0/Cargo.toml` の `[features]`。`default` はこれら全部入り（x11 / vulkan / xwayland 含む）。必要分だけ明示する。
- Cargo.toml の編集は人間の領域（CLAUDE.md 絶対規則 1）。実装時は変更案を提示して止まる。

### 3.2 anvil（v0.7.0 `anvil/src/udev.rs`）の流れ
1. `LibSeatSession::new()` → (session, notifier)。
2. `primary_gpu(seat)` / `all_gpus`。`UdevBackend::new(seat)` と `device_list()` で DRM デバイスを列挙。
3. `device_added`: `session.open` → `DrmDevice::new(fd, true)` と `GbmDevice::new(fd)` → `EGLDisplay::new(gbm)` → `GbmAllocator`（RENDERING | SCANOUT）と `GbmFramebufferExporter` → `DrmOutputManager::new(..)`。DRM の notifier を登録し、`DrmEvent::VBlank` で `frame_finish`。
4. `device_changed`: `DrmScanner::scan_connectors` → `Connected` / `Disconnected` を `connector_connected` / `connector_disconnected` へ。
5. `connector_connected`: preferred mode を選び、`Output::new` + `create_global` + `set_preferred` + `change_current_state` + `space.map_output`。`drm_output_manager.initialize_output(crtc, mode, &[connector], &output, ..)` で `DrmOutput` を得る。
6. 描画: `render_surface` が `drm_output.render_frame(renderer, elements, clear_color, flags)` → 描けたら `queue_frame`。
7. VBlank: `frame_submitted()` → 次フレームを `Timer` で予約。
8. 入力: `Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>` + `udev_assign_seat` → `LibinputInputBackend` をイベントループへ。
9. session notifier: pause → libinput suspend、`drm_output_manager.pause()`。resume → libinput resume、`activate(false)`、再描画を予約。
10. ホットプラグ: udev の `Changed` で `device_changed` を再実行。`Removed` で `device_removed`。
- エラー処理（anvil）: `DeviceInactive` は再スケジュールしない（resume が担う）、`TestFailed` は状態リセット、`ContextLost` は回復不能。

### 3.3 Contrast での簡略化（初回）
- GPU は 1 枚（`card1`）のみ。multi-GPU（`GpuManager`）は使わない。`GlesRenderer` を直接使う。
- dmabuf / explicit sync / lease / 10bit は後回し。
- カーソルは最初は描かない（P4 で検討）。

## 4. NixOS・実機での試し方

### 4.1 実機の状態（調査で確認したこと）
- GPU: AMD Krackan（Radeon 840M / 860M、amdgpu）。`/dev/dri/card1` と `renderD128` のみ。
- 出力: `card1-eDP-1` が connected（1920x1200 と 1920x1080 のモード）。他の DP / HDMI は disconnected。
- 動作中のセッション: niri（logind セッション 7、tty1、seat0、Type=wayland、Active）。
- seat 管理: **logind あり、seatd なし**（`/run/seatd.sock` なし）。libseat は logind バックエンドを使う想定。
- `/dev/input/event*` は `root:input` で、ユーザー（groups: users, wheel, video, networkmanager, kvm, render）は直接読めない。libseat 経由で開く。
- sshd は inactive。`getty@tty2` は inactive。

### 4.2 niri を壊さずに別 TTY で試す
1. **niri の端末からは実行しない**。logind の TakeControl は自分のセッション（Active なもの）に対して行われる。niri 内から起動すると niri のセッションを奪いに行って失敗する（または niri と衝突する）。
2. Ctrl+Alt+F2 で tty2 へ切り替え、そこでログインする（新しい logind セッションができる）。
3. tty2 のシェルで、ビルド済みバイナリを実行する。開発シェルが要る場合は `nix develop -c ./target/debug/contrast`。
4. 最初の実機試験は `CONTRAST_EXIT_AFTER=15` から。
5. Ctrl+Alt+F1 で niri に戻れる（logind が VT 切り替えを処理）。
- 未確認: tty2 でのログインから `libseat` が logind セッションを掴めるまでの具体手順（`XDG_SESSION_ID` / `XDG_VTNR` の値）。P1 の実機試験で確認する。

### 4.3 必要なシステムライブラリと flake.nix の差分案
flake.nix は変更しない（人間の領域）。差分案:
```nix
packages = [
  # 既存に加えて
  pkgs.libseat      # LibSeatSession（logind / seatd）
  pkgs.libinput     # libinput バックエンド
  pkgs.libdrm       # DRM
  pkgs.mesa         # libgbm / libEGL / GL ドライバ（amdgpu は radeonsi）
  pkgs.systemdLibs  # libudev（属性名は要確認。無ければ pkgs.udev）
  pkgs.seatd        # 任意: seatd で試すとき用
];
LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
  pkgs.wayland pkgs.libxkbcommon pkgs.libGL
  pkgs.libseat pkgs.libinput pkgs.mesa pkgs.systemdLibs
];
```
- `pkg-config` は既にある。`gbm` / `input` / `libseat` / `udev` の各 sys クレートが pkg-config で探す。
- nixpkgs の属性名は未検証（今回 `nix eval` で確認しようとして属性が見つからなかった）。実装時に devShell に足して `cargo build` が通るかで確かめる。
- 実行時に dlopen される物（libEGL、GL ドライバ、libxkbcommon）は `LD_LIBRARY_PATH` で見えるようにする既存の方式を踏襲。NixOS では GL ドライバは `/run/opengl-driver` 経由でも見える。

## 5. 入力・出力の対応

### 5.1 core の Command との対応
| DRM 側のイベント | core の Command | 備考 |
|---|---|---|
| connector 接続（preferred mode） | `OutputAdded { key, name, geometry }` | key は connector のオブジェクト ID から作る。name は `eDP-1` など。geometry は論理矩形（拡大率 1、原点は配置規則から） |
| connector 切断 | `OutputRemoved(key)` | 窓は残る（core-spec D24）。同じ key の再追加は新しい OutputId |
| モード変更 | `OutputChanged { key, geometry }` | `Err(Layout(AreaTooSmall))` はログに出して続行（D17 / D32） |
| layer-shell の exclusive zone | `OutputInsetsChanged { key, insets }` | layer-shell 実装後。同じ値なら Effect なし（D31） |
- 複数出力の配置: 当面は横並び固定（左から接続順）。`Output` の `map_output` 位置と core の geometry を一致させる。
- `OutputAdded` はフォーカスを与えない（D21）。
- 要追加: `shell.rs` に `output_removed` / `output_insets_changed`。`Shell` の出力 key ↔ `Output` の対応表も更新する。

### 5.2 キーボード配列（xkb）
- `seat.add_keyboard(XkbConfig { layout, variant, options, .. }, delay, rate)` に変える。
- 設定の元: 環境変数 `CONTRAST_XKB_LAYOUT` / `CONTRAST_XKB_VARIANT` / `CONTRAST_XKB_OPTIONS`（無ければ既定）。
- 未指定のときは xkbcommon の既定を使う（決定済み）。参考: この機の `localectl` は X11 layout `us`、`~/.config/niri/config.kdl` の xkb は未設定（コメントのみ）なので、現在は US（ユーザーに確認済み）。既定の配列は US で足りる。

### 5.3 ポインタ
- libinput は相対移動（`PointerMotion`）を出す。`input/mod.rs` に追加し、座標は出力全体の矩形にクランプする。
- ボタン・スクロール（`PointerButton` / `PointerAxis`）は既存処理が流用できる。
- カーソル画像の描画は P4 の後半または後回し（見えないカーソルでもフォーカス移動は動く）。

## 6. 失敗したときの安全策

### 6.1 自動終了タイマー
- `CONTRAST_EXIT_AFTER=<秒>`。**DRM モードでは必須**。未指定なら既定 30 秒で自動終了する（無効化はしない）。起動時に calloop の `Timer` を登録し、満了で `loop_signal.stop()`。
- 最初の実機試験は 15 秒（`CONTRAST_EXIT_AFTER=15`）から始める。
- 終了時は `DrmOutputManager` と session を drop して、VT を元に戻す。

### 6.2 緊急終了キー
- P4 で、特定の組み合わせ（例: Ctrl+Alt+Backspace 相当）を backend 側で握って終了させる。`Policy` を経由させない。

### 6.3 復旧手順
1. 画面が黒 / 固まった: **Ctrl+Alt+F1** で niri に戻る（logind が VT を切り替える）。戻れたら tty2 のセッションを終了。
2. キー入力が効かない（libinput が掴んだまま）: 自動終了タイマーを待つ。
3. それでも戻れない: 別の端末から SSH して `pkill contrast`（`loginctl terminate-session <id>`）。**現状 sshd は無効なので、実機試験の前に有効化するか、別の手段（別の PC など）を用意する**（確認事項 2）。
4. 最後の手段: 電源長押しでの再起動（ファイルシステムへの書き込みは行わない設計なので破損リスクは低い）。
- niri のセッション（tty1）は触らない。

## 7. フェーズ

### P1: session と udev で出力を列挙してログに出す
- (a) 終了条件: tty2 で起動し、`eDP-1 1920x1200@60` 相当の行がログに出て、`CONTRAST_EXIT_AFTER` で正常終了する。画面は何も変えない（DRM master を取っても描かない、または取らない）。
- (b) 自動:
  - 純粋ロジック（connector 情報 → `OutputAdded` 用の name / geometry / key）の単体テスト。DRM 型に依存しない形で書く。
  - `nix develop -c cargo build` と `scripts/verify.sh`。
  - ヘッドレスで DRM を開く自動試験は **現状できない**（下記 8.2）。
- (c) 実機: Ctrl+Alt+F2 → ログイン → `CONTRAST_EXIT_AFTER=15 CONTRAST_BACKEND=drm contrast`。期待ログ: session 取得、device 追加（card1）、connector eDP-1 connected、モード一覧、終了。失敗時は Ctrl+Alt+F1。

### P2: 単色で塗って page flip
- (a) 終了条件: 画面全体が `policy.background()` の単色になり、60 秒間 VBlank が来続ける（フレームカウントがログに出る）。終了後に tty が元に戻る。
- (b) 自動: フレームスケジューラ（VBlank → 次の予約）の状態遷移を、DRM を使わない小さな型に切り出して単体テスト。実際の page flip は実機のみ。
- (c) 実機: 画面が単色になること。ちらつき・ティアリングがないこと。ログの VBlank 間隔が約 16.7ms（60Hz）または約 8.3ms（120Hz）。

### P3: 既存の描画（窓・枠・レイヤー面）を載せる
- (a) 終了条件: `foot` を起動して窓が出て、フォーカス枠が出て、Alt+1〜4 でワークスペースが切り替わる（入力は P4 の前なので、起動時に自動で `foot` を 1〜2 個立ち上げるデバッグ用フラグを使う）。
- (b) 自動: `render_frame` の要素組み立てを DRM 非依存の関数にして、`Space` に窓を置いたときの要素数・矩形をテスト（renderer_test など Smithay のテスト用 renderer の有無は未確認）。
- (c) 実機: 窓の位置が core の配置と一致する。layer-shell が入っていれば、バー等が上端に出て `OutputInsetsChanged` で作業領域が縮む。入っていなければ窓と枠のみ。

### P4: libinput でキーとポインタ
- (a) 終了条件: キーボードでバインドが動き、`foot` に文字が入力され、ポインタが動いてクリックでフォーカスが移る。緊急終了キーで終了できる。
- (b) 自動: 相対移動のクランプ計算の単体テスト。xkb 設定の読み込み（env → `XkbConfig`）の単体テスト。
- (c) 実機: 押すキー: 端末起動バインド、Alt+1〜4、Alt+Shift+Q 相当（close focused）。タッチパッド/マウスを動かし、クリックでフォーカスが移る。配列が期待通りか（記号キー）。

### P5: ホットプラグと VT 切り替え
- (a) 終了条件: 外部モニタの抜き差しで `OutputAdded` / `OutputRemoved` が出て、窓が残る。Ctrl+Alt+F1 → F2 の往復で描画と入力が復帰する。
- (b) 自動: `DrmScanner` の差分イベント → Command 列の変換を純粋関数でテスト（接続・切断・モード変更・同一 key 再追加）。
- (c) 実機: HDMI / USB-C のモニタを抜き差し。tty を往復しても画面が戻る。要外部モニタ（現状は eDP のみ接続）。

## 8. 自動検証の可否

### 8.1 できる
- 純粋ロジックの単体テスト（変換、クランプ、スケジューラ状態、env 読み込み）。
- `scripts/verify.sh`（fmt / clippy / no_std / 層検査 / test）。
- ログによる確認（`tracing`）。人間が実機ログを貼る。

### 8.2 できるか未確認
- QEMU + virtio-gpu: この環境の PATH に `qemu-system-x86_64` は無い。`/dev/kvm` はある。flake に `pkgs.qemu` を足し、NixOS の最小 VM（`nixos-rebuild build-vm` 相当）で virtio-gpu を出す方法は考えられるが、**未検証**。TTY / logind が VM 内に必要で、コストが高い。
- `vkms`（仮想 KMS カーネルモジュール）: ホストのカーネルに入っているか未確認。ロードにはルート権限が要る。
- Smithay の `renderer_test`: 0.7.0 の feature に存在する（`renderer_test = []`）。要素の組み立ての試験に使えるかは未検証。
- 結論: 自動検証の主軸は純粋ロジックとログ。DRM を実際に叩く部分は人間が実機で確かめる。

## 9. リスクと未確認

### 9.1 わかったこと
- Smithay 0.7.0 に DRM / GBM / libinput / libseat / udev の各バックエンドと `DrmOutputManager` がある。
- 実機は AMD 内蔵 GPU 1 枚、eDP-1 のみ接続、logind あり・seatd なし。
- `process_input_event` は `InputBackend` で汎用。core は DRM を知らずに済む（Command で足りる）。
- Insets は core に入っている。layer-shell は無い。

### 9.2 わからないこと
- nixpkgs の正確な属性名（libseat / libudev）。
- tty2 からの libseat（logind）の取得が実際に通るか。
- 自動 DRM 試験（QEMU / vkms）が成立するか。
- 外部モニタ接続時の挙動。複数出力でのカーソルとフォーカスの扱い。
- anvil は v0.7.0 のみ確認。docs.rs の 0.7.0 ページは未参照（ローカルのソースで代用）。
- amdgpu で `GbmAllocator` + `DrmOutputManager` の既定フォーマットで問題なく出るか（10bit を無効にしたほうがよいか）。
- ユーザーのキーボード配列。

### 9.3 リスク
- 画面が真っ黒 / 入力が奪われる。→ 6 章の安全策。
- niri との DRM master 競合。→ 別 VT・別セッションで実行（4.2）。
- `unwrap` / `expect` 禁止の規約と、既存コードの `.expect` の混在。→ 触る箇所から順に直す。
- `Cargo.toml` / `flake.nix` は人間の領域。→ 差分案を出して止まる。

## 10. 決定事項（確認事項への回答）

1. **layer-shell が先**。別タスクで先に作り、排他領域の反映（`OutputInsetsChanged`）が済んだらマージする。P1（出力の列挙だけ）はほとんど競合しないので、`proto/drm` ブランチで並行して始めてよい。layer-shell は P3（既存の描画を載せる）の前にマージする。
2. **復旧手段**: タイマーを必須にする（DRM モードの既定は 30 秒で自動終了）。最初の実機試験は 15 秒から。sshd は、別の端末（スマホなど）から入れるときだけ意味がある。入れるなら鍵認証のみで有効にする。別の端末がなければタイマーを主な安全策にする。
3. **QEMU の調査は続けない**。自動検証は純粋ロジックのテストとログ、DRM の実動作は実機。
4. **バックエンド選択**: `CONTRAST_BACKEND=winit|drm`。DRM は明示指定のときだけ（2 章）。
5. **キーボード配列**: `CONTRAST_XKB_LAYOUT` などで指定。未指定は xkbcommon の既定（5.2 章）。
6. **`render_frame` の共通化**: P2 の前に、動作を変えない小さなリファクタとして入れる（winit で確認できる）。

## 11. 確定事項

1. layer-shell は別タスクで先に作る（`proto/layer-shell` に入った。`proto/drm` はそこから切る）。
2. 復旧手段はタイマー必須。DRM モードの既定は 30 秒で自動終了し、環境変数 `CONTRAST_EXIT_AFTER`（秒）で変える。
3. QEMU による自動試験はしない。
4. `CONTRAST_BACKEND=winit|drm`。`drm` は明示指定のときだけ有効。未指定なら、`WAYLAND_DISPLAY` か `DISPLAY` があれば winit、なければ「`CONTRAST_BACKEND=drm` を指定する」というエラーで終了する。
5. キーボード配列は `CONTRAST_XKB_LAYOUT` / `CONTRAST_XKB_VARIANT` / `CONTRAST_XKB_MODEL` / `CONTRAST_XKB_OPTIONS` で指定し、未指定は xkbcommon の既定（フェーズ4で実装）。
6. `render_frame` の共通化は、フェーズ2の前に、動作を変えない小さなリファクタとして入れる。
