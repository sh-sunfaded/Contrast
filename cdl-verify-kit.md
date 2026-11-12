# CDL 検証方針の改訂キット

このファイルは Claude Code への引き渡し用です（コミットしない）。内容は 2 つです。

- A. `docs/SPEC.md` への仕様改訂（該当の章を探して、下の文面に置き換える）
- B. 設定ファイル（`Cargo.toml` と `clippy.toml`）

出典は Claude Doc「CDL 仕様書」の改訂版（2026-11-06）。clippy の lint 名と `clippy.toml` のキーは、実機の clippy で未確認。受け付けられない名前があれば、直さずに報告する。

---

## A. docs/SPEC.md の改訂

### A1. 第 7 章「リソース上限」の表に 2 行を追加

| 項目 | 既定値 | 対象 |
| --- | --- | --- |
| 構文ネスト深さ | 64 | 1 ファイル。超過は E1xxx（パースの再帰を有界にする） |
| 値木の深さ | 64 | 評価結果全体（型検査・評価・ホスト変換の再帰を有界にする） |

### A2. 第 10 章「コード帯」の E1xxx の例

「期待したトークンがない」を「期待したトークンがない、構文ネスト深さの超過」に直す。

### A3. 第 12 章「検証とテスト方針」: 表を次に置き換える

| 主張 | 守る手段 |
| --- | --- |
| 任意入力でパニックも abort もしない | clippy の deny（unwrap・expect・panic・添字・算術・縮小キャスト）、ネスト深さの上限、proptest（任意の文字列を入力） |
| パーサ・評価器は必ず停止する | 設計（呼び出しグラフは DAG、反復は有界な組み込みのみ、評価ステップ上限）、上限ちょうどと上限 + 1 の境界テスト、proptest（任意入力で完了する） |
| 整数演算にラップ・未定義動作がない | `unsafe_code = "forbid"`、clippy の `arithmetic_side_effects` を deny（checked 演算のみ） |
| `fmt(parse(x))` は冪等で、意味を変えない | proptest（任意の AST から生成） |
| 型検査を通った式は評価で型エラーにならない（健全性） | proptest（型付き式ジェネレータ）と、参照モデルとの差分テスト |
| 診断のコード・位置が回帰しない | ゴールデンテスト（`.cdl` と期待 JSON） |
| 同じ入力は常に同じ出力 | 実行を 2 回行う差分テスト、clippy の `disallowed_types` / `disallowed_methods`（HashMap・時刻・環境・乱数を禁止） |
| リソース上限が効く | 各上限（第 7 章）の境界テスト（ちょうどなら成功、+ 1 なら診断） |

### A4. 同章「実装ルール」を次に置き換える

- コアのクレートは `unsafe` を禁止する（ワークスペースの lint 設定 `unsafe_code = "forbid"`）。
- ワークスペースの lint で、clippy の `unwrap_used` `expect_used` `panic` `todo` `unimplemented` `unreachable` `indexing_slicing` `arithmetic_side_effects` と、縮小・符号変更のキャスト（`cast_possible_truncation` `cast_possible_wrap` `cast_sign_loss`）を deny にする。テストコードだけは clippy.toml で許す。
- 決定性のため、clippy.toml の `disallowed-types` で `HashMap` / `HashSet` を、`disallowed-methods` で時刻・環境変数・乱数の取得を禁止する。
- パーサ・型検査・評価器の再帰は、ネスト深さの上限（第 7 章）の内側でだけ行う。パーサの繰り返しは、入力を 1 トークン以上進めるか、終了する。
- 公開 API は `Result` のみを返し、診断は常に値として扱う。
- 言語仕様の変更は、先に適合テスト（`.cdl` と期待結果の組）を足してから実装する。

### A5. 同章に、実装ルールの直後（「適合テストスイート」の前）へ 2 節を追加

#### 採らない手段

この仕様の検証には、Kani と cargo-fuzz を使いません（開発環境で実行できないため）。cargo-mutants と形式証明系は、この段階では採りません。Miri は、`unsafe` を禁止しているので不要です。したがって、上の表の主張は証明されたものではなく、静的な lint と設計上の制約とテストで守るものです。

#### 検証の実行段階

- 日常: 変更した crate だけを `cargo test -p` / `cargo clippy -p` にかけ、`cargo fmt --check` を行う。proptest の回数は少なめにする（64）。
- 区切り（マージ・リリースの前）: ワークスペース全体の test と clippy。proptest は既定の回数（256）で行う。
- 重い実行は、メモリ上限をかけて行う。

### A6. 付録 C「決定事項」にチェック済みを 2 件追加

- [x] 検証手段: lint・設計の制約・proptest・境界テストで守る。Kani と cargo-fuzz は採用しない
- [x] ネスト深さの上限: 構文 64（E1xxx）・値木 64（暫定、ホストが上書き可）

---

## B. 設定ファイル

### B1. ワークスペースの `Cargo.toml` に追加

```toml
[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
todo = "deny"
unimplemented = "deny"
unreachable = "deny"
indexing_slicing = "deny"
arithmetic_side_effects = "deny"
cast_possible_truncation = "deny"
cast_possible_wrap = "deny"
cast_sign_loss = "deny"

# リンク時のメモリを減らす
[profile.dev]
debug = "line-tables-only"

[profile.dev.package."*"]
debug = false
```

各クレートの `Cargo.toml` に `[lints] workspace = true` を書く。`cdl-cli` には、`unsafe_code` の forbid だけを適用してよい（clippy の deny は、コアのクレートに限る）。

### B2. リポジトリ直下に `clippy.toml`

```toml
allow-unwrap-in-tests = true
allow-expect-in-tests = true
allow-panic-in-tests = true
allow-indexing-slicing-in-tests = true

disallowed-types = [
  { path = "std::collections::HashMap", reason = "決定性: 辞書順の BTreeMap を使う" },
  { path = "std::collections::HashSet", reason = "決定性: BTreeSet を使う" },
]

disallowed-methods = [
  { path = "std::time::SystemTime::now", reason = "時刻に依存しない" },
  { path = "std::time::Instant::now", reason = "時刻に依存しない" },
  { path = "std::env::var", reason = "環境に依存しない" },
  { path = "std::env::vars", reason = "環境に依存しない" },
]
```