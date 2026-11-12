# Contrast 側の決定記録（CDL 組み込み）

CDL（Contrast Document Language）を Contrast に組み込むにあたっての決定。1 項目 1 決定。
CDL 側の仕様・実装に問題を見つけたら、Contrast 側で回避せず「CDL への報告」節に書く。

## 決定

### CD1 接続の置き場所
CDL の接続は `contrast-policy` の `cdl_adapter` モジュールに置く。新しいクレートは作らない。
- 層: `contrast-core` は CDL を知らない（変更なし）。`contrast-compositor` も CDL を知らない。
- `contrast-policy` が `cdl-host` に依存する。`cdl-syntax` / `cdl-sema` / `cdl-eval` には直接触れない。
- 帰結: `cdl-host`（と推移的に `oxc_*`）が `contrast-policy` の下流すべてに入る。

### CD2 依存の仕方
- 開発中は `cdl-host` へのパス依存（`../../../cdl/crates/cdl-host`）。
- 固定するときは git 依存の `rev`（フル SHA）。CDL にタグはない。
- 確認時点の CDL の HEAD は `bcff355ff88ecb91013becf212c1617d3db13fec`（作業ツリーはクリーン）。

### CD3 スキーマ
Phase 3 までは `conformance/HOST.md` の参照ホスト `host:contrast` と同一にする。
- `Contrast`（ルート）の子は `Appearance` / `Animation` / `Keybind`。
- `Appearance { gap: Length, radius: Length, focusColor?: Color }`
- `Animation { duration: Duration, enabled?: boolean }`
- `Keybind { keys: string, action: string }`
- `Policy { windowOpen?: (WindowInfo) => Placement }`、`WindowInfo { appId: string, workspace: number }`、`Placement { workspace: number, floating: boolean }`
- 理由: `cdl check --preset contrast` で利用者が自分の設定を検査できる状態を保つ。拡張は CDL と調整してから。

### CD4 一意性と識別
CDL のビルダーには識別 prop がない（CDL C4）。
- スキーマの一意子は `Appearance` のみ（HOST.md どおり）。
- `Animation` の重複は Contrast 側の変換で診断する。
- `Keybind` は `keys` を自然キーとし、重複は変換で診断する。

### CD5 設定が壊れているとき
- 起動時: 全診断を error ログに出し、`StaticPolicy` で起動する（拒否しない）。
- リロード時: 旧版を保持し、全診断を error ログに出す。

### CD6 モジュールの解決
- import は設定ファイルのあるディレクトリの内側に限る。`canonicalize` した後で判定し、symlink で外へ出るものも拒否する。
- ファイルを読む前にサイズを確認し、`Limits::max_source_bytes` を超えたら読まない。

### CD7 リロード
- 契機は keybind から発火する `ReloadConfig` から始める。ファイル監視は依存が増えるので見送る。
- 新しい版の読み込み・変換が成功したときだけ、全体を原子的に差し替える。

### CD8 ワークスペース番号
CDL（`windowOpen` の引数と戻り値）では 1 始まり。`contrast-policy` / core へは 0 始まりに変換する。
範囲外・非整数・非有限の値は、丸めずにフックの失敗として扱う。

### CD9 フックの失敗
`Err`（評価エラー・上限超過・引数不一致）、戻り値の形の不正、workspace の不正は、すべて `StaticPolicy` と同じ配置に戻す。
診断は warn ログに出す。

### CD10 `contrast-policy` の内部型の変更
次の変更を許可する（core の公開 API ではない）。
- `LoadOutcome::Failed(String)` → `Failed(Vec<ConfigDiagnostic>)`
- `Policy::place_window` にフォーカス中のワークスペースを渡す引数を足す（P3）
- `ReloadablePolicy` の新設（P4）

### CD11 キーと action の文法（初期）
- `keys` は `mod+shift+Return` の形。`mod` は Alt（nested winit 前提）。
- `action` の語彙は、現在の policy の `Action` の範囲（`spawn <name>`、`close`、`focus next|prev`、`workspace <n>`、`quit`）。
- 未知の action は読み込み時に診断にする（キー押下時には失敗しない）。

## 改訂（契約 `docs/cdl-contract.md` による。上の CD1〜CD11 のうち、ここに書いたものはこちらが正）

### CD1' 接続の置き場所（CD1 を置き換え）
- 新しいクレート `contrast-policy-cdl` に CDL の接続を置く。`cdl-host` に依存するのはこのクレートだけ。
- `contrast-policy` は CDL を知らない（設定の型 `Settings`・`ConfigStore`・フックの口 `PlacementHook`・診断）。`cdl-host` の依存は外した。
- 旧 `crates/contrast-policy/src/cdl_adapter*`（未コミットだった試作）は、どこからも参照しないまま残してある。
  新クレートの `resolver.rs` / `diag.rs` はその流用。削除は人間が判断する。

### CD2' 依存の仕方（CD2 を置き換え）
- git 依存で `rev = bcff355ff88ecb91013becf212c1617d3db13fec`（`origin/main` の HEAD。`git ls-remote` で確認済み）。
- 手元の `~/RustroverProjects/cdl` への差し替えは、リポジトリに入れない `.cargo/config.toml` の `[patch]`（`.gitignore` 済み）。
  パッチを有効にすると `Cargo.lock` の cdl-* が path 扱いに変わることがあるので、そのロックはコミットしない（既定は無効）。

### CD3' スキーマ（CD3 を置き換え）
参照ホスト `host:contrast` とは別物にした（`cdl check --preset contrast` では検査できない）。
`Contrast`（`modKey?`）・`Appearance`・`Workspaces`・`Keybind` と、`Policy.windowOpen`（`WindowInfo` → `Placement`）。契約を参照。
`modKey` と `action` はリテラルユニオンにしたいが、`cdl-host` が `Literal` を公開していないので `string` にして Contrast 側で検査する。

### CD4' 一意性（CD4 を置き換え）
スキーマの一意子は `Appearance` と `Workspaces`。`Keybind` の重複は `(修飾キーの集合, キー名)` で変換時に検査する（CT031）。
識別 prop は意味が不明なので未実装。

### CD5' 設定が壊れているとき（CD5 を置き換え）
起動時: 設定が無ければ既定値で info、壊れていれば全診断を error で出して既定値で起動。リロード: 旧版を保持し、全診断を出す。

### CD7' リロード（CD7 を置き換え）
契機は `reload-config` アクションと SIGUSR1。ファイル監視はしない。リロードで `Workspaces.count` が変わっても
core は起動後に数を変えられないので、新しい版は採用するが数は起動時のまま（CT050 の警告）。

### CD8' / CD9' フック
CDL のワークスペース番号は 1 始まり、`contrast-policy` と core は 0 始まり。不正な値は丸めず、その呼び出しだけ既定の配置にして診断（CT040〜CT043）を出す。
診断は warn。同じコードは世代ごとに 1 回だけログに出す。

### CD11' キーと action（CD11 を置き換え）
キー書式は `mod+shift+Return`（`mod` は `modKey` に従い alt か super）。action の語彙は契約を参照。

### CD12 色の表現
内部の色は RGBA の `[f32; 4]`。CDL の 8 ビットの色を 255 で割って変換する。組み込みの既定色は
従来の `[0.12, 0.14, 0.20]` / `[0.40, 0.65, 1.0]` に最も近い 8 ビット（`#1f2433` / `#66a6ff`）に取り直した（`default.cdl` と完全に一致させるため）。

### CD13 `contrast-policy` の `tracing` 依存
フックの診断を世代ごとに 1 回だけログに出すため、`contrast-policy` が `tracing`（既存の固定バージョン）に依存する。

## CDL への報告
Contrast 側では回避しない。内容と影響のみ記す。
1. 参照ホストの定義が 3 か所で食い違う。`conformance/HOST.md` と `cdl-cli` の `contrast_schema()` は一意子が `Appearance` のみ。`examples/contrast.rs` は `Appearance` と `Animation` の両方。影響: Contrast は HOST.md に従い、`Animation` の重複を自前で検査する。
2. README は SPEC.md を正典としつつ、食い違いは DECISIONS.md に従うと書く。PROTOTYPE-REPORT は DECISIONS の多くが SPEC に未反映だと認めている。影響: 実質の正は DECISIONS.md。
3. 識別 prop に当たる機能がない。影響: リロードの差分は Contrast 側で `keys` を自然キーにして取る。
4. HANDOFF.md は「作業ツリーに未コミット 89 ファイル」と書くが、確認時点（`bcff355`）の作業ツリーはクリーン。影響: HANDOFF.md が古い可能性がある。
