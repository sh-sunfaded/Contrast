# CDL 統合の点検表（proto/cdl）

「確認済み」は、自動テストか実行ログで実際に確かめたものだけ。見ていないものは「未確認」に置く。
測定の数値は `crates/contrast-policy-cdl/examples/measure.rs` で取った（release ビルド、この PC の 1 回の測定。再現性の幅は未評価）。

## 確認済み

| 項目 | 根拠 |
| --- | --- |
| `default.cdl` を読むと `Settings::builtin()` と一致する | テスト `default_example_equals_builtin` |
| `config.cdl` が読め、窓ルール（firefox → ws2、pavucontrol → Floating、ダイアログ → Floating）が効く | テスト `config_example_loads_and_window_rules_apply`、実クライアント（alacritty `--class`）でログ `window added … workspace=2` / `state=Floating` |
| 複数の誤りが全部、CT コードと `file:line:col` つきで出る | テスト `all_conversion_errors_are_reported_together`、壊した設定の起動ログ（CT020・CT021） |
| prop の値の誤り（CT003・CT010） | テスト `prop_value_errors_have_codes` |
| CDL の構文エラーが位置つきで出る | テスト `syntax_and_type_errors_come_from_cdl_with_positions` |
| import は設定ディレクトリの内側だけ（`..`・シンボリックリンクで外へ出るものを拒否） | テスト `import_outside_config_dir_is_rejected`、`symlink_import_pointing_outside_is_rejected` |
| フックの `workspace` が 0・負・非整数・範囲外のとき既定の配置になり CT042 / CT043 | テスト `invalid_hook_workspace_falls_back_to_default_placement`、`hook::tests::*` |
| キー書式のパーサが任意の文字列でパニックしない | proptest `keys::tests::never_panics`、`never_panics_on_plus_heavy_input` |
| 設定ファイルなしは `Absent`（エラーでなく既定値で起動） | テスト `missing_file_is_absent`、起動ログ `loaded=absent` |
| 失敗した設定は旧版を保持し、世代は進まない | テスト `store::tests::failed_reload_keeps_old_version`、SIGUSR1 のログ `reload failed … keeping generation=2` |
| 成功したリロードで世代が進む | テスト `replace_bumps_generation_and_keeps_workspace_count`、ログ `reloaded … generation=2` |
| SIGUSR1 でリロードできる | 実行ログ（`config: reloaded … generation=2`） |
| `reload-config` アクション（デバッグ stdin の `key alt+shift+r`）でリロードできる | 実行ログ（`action=ReloadConfig` → `reloaded`）。実キーボードでの入力ではない |
| 各アクションがコンポジタに届く（`MoveToWorkspace`・`ToggleFloating`・`ToggleFullscreen`・`Spawn`・`Quit` ほか） | デバッグ stdin のログ。窓がないときは `no focused window, ignored` までで、窓への効果は未確認 |
| 起動時に `config: path=… loaded=… generation=N bindings=M` が出る | 起動ログ 3 通り（なし / 正常 / 壊れ） |
| 巨大（1 MiB 超）・権限なし・UTF-8 でない・空・深い import（50 / 300 段）でパニックもハングもしない | テスト `impl_failure_paths.rs` の 6 件（30 秒のタイムアウト付き） |
| 重複キーの検査が線形時間に近い | 測定（20,000 件 138 ms、60,000 件 1.07 s）。修正前は 350 ms / 3.7 s（二乗） |
| `contrast-core` に変更がない | `git diff 5b697c5 HEAD -- crates/contrast-core` が空 |

## 未確認（手で見る必要がある）

| 項目 | 理由・見方 |
| --- | --- |
| 枠の色・太さ・背景色の見た目、フォーカス外の枠（`borderColor`）の描画 | 画面を見ていない。`config.cdl` で起動して目視 |
| 実キーボードでのキー入力（Alt / Super、Shift の組み合わせ、`raw_latin_sym` が返すキー名との一致） | デバッグ stdin は `resolve_key` を直接呼ぶだけ。実際の xkb のキー名との一致は未確認 |
| `modKey="super"` の動作 | テストは `KeySpec` の組み立てまで。ホストの Super キーが奪われる可能性もある |
| リロード直後の既存窓の再配置（枠の太さの変更が見た目に出るか） | `reapply_layout` を呼ぶ実装はあるが、窓を開いた状態でのリロードは見ていない |
| 窓があるときの各アクション（Move / Toggle / MoveToWorkspace）の効果 | 窓なしのログだけ |
| フックの診断を世代ごとに 1 回だけログに出す動作（`log_once`） | 専用のテストがない |
| フックの上限超過（`max_hook_steps`）の経路 | CT040 の経路をテストしていない |
| 起動時に SIGUSR1 が早く届いた場合 | シグナルは `EventLoop` 作成直後に登録するが、競合は見ていない |
| 設定ファイル自体がシンボリックリンクのとき | 正準化した先のディレクトリが「設定ディレクトリ」になる実装。意図どおりかは未判断（dotfiles 管理では先のディレクトリの import が使える） |
| 実際のユーザーの設定規模での性能 | 測定は合成した設定のみ |
| 評価ステップ数 | `cdl-host` が公開していないので数えられない（境界は N を変えて探した） |
| 要素数が上限（1,000,000）に近い設定の所要時間・メモリ | ソース 1 MiB の制限内では作れなかった。60,000 件までの外挿しかない |
| 同期リロードが画面操作を止める時間（実機） | 測ったのは `ConfigStore::reload` の所要時間。再配置と描画は含まない |

## 既知の制約

| 制約 | 内容 |
| --- | --- |
| `Literal` が `cdl-host` から公開されていない | `modKey` と `action` をリテラルユニオンにできず、`string` にして Contrast 側で検査している |
| props に位置がない | 診断の位置は要素単位（`file:line:col` は要素の先頭）。メッセージに prop 名を書く |
| `Workspaces.count` はリロードで変えられない | core が起動後に数を変えられない。新しい版は採用するが数は起動時のまま（CT050） |
| 識別 prop は未実装 | 意味が不明なので実装していない |
| 読めない・UTF-8 でない・サイズ超過の診断コードは CDL の `E2001` | 契約の CT001 は、エントリの状態を調べられないときだけ。契約を実装に合わせて直した |
| 空のファイルは `Absent` ではなく `Failed`（`E1003`: edition 宣言がない） | 空のファイルは「存在するが不正」として扱う。ファイルがない場合だけが `Absent` |
| スキーマが参照ホスト `host:contrast` と別物 | `cdl check --preset contrast` では今の設定を検査できない |
| `scripts/verify.sh` は `contrast-core` しか検査しない | 他のクレートは個別に `fmt` / `clippy` / `test` を実行している |
| 同期リロード | 重い設定ではイベントループが止まる。数値は報告（`.cc-logs/report-cdl-hardening.md`）参照。Limits を絞る案は未実装 |
| キーバインドの数に上限がない | 解決は線形探索。測定した範囲（60,000 件）では 1 回のリロードが約 1 秒 |
