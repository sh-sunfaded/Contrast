# Contrast — Claude Code 作業規約

## このリポジトリ
Rust + Smithay の Wayland コンポジタ Contrast。
現在の作業対象は `crates/contrast-core` だけ。他のクレートには触れない。
- 仕様: `docs/core-spec.md`（「暫定決定」節を含めて正とする）
- 仕様への疑問: `docs/questions.md`

CDL（Contrast Document Language）は別リポジトリ。contrast-core は CDL を一切知らない。

## 絶対規則（例外なし）
1. 次のパスは人間が書く仕様。読むだけで、編集・作成・削除しない。
   - `crates/contrast-core/tests/spec/`
   - `crates/contrast-core/src/verification/spec/`
   - `CLAUDE.md`, `.claude/`, `docs/core-spec.md`, 各 `Cargo.toml`
   - `scripts/`, `.github/`, `rust-toolchain.toml`, `flake.nix`, 各 `clippy.toml` / `rustfmt.toml` / `deny.toml`
2. 破壊的操作をしない: rm、ファイル削除、`git reset`、`git clean`、
   `git checkout`、`git restore`、`git push`、`git rebase`、`git stash drop`。
   必要に見えたら実行せず、報告に「提案」として書いて止まる。
3. 依存クレートを追加しない（dev-dependencies も含む）。
4. 本体コードで使用禁止: `unsafe`、`unwrap`、`expect`、`panic!`、`unreachable!`、
   `todo!`、`unimplemented!`、添字アクセス `a[i]`、オーバーフローしうる素の算術。
   `#[allow(...)]` で lint を黙らせない。
5. `docs/core-spec.md` の「公開 API」のシグネチャを変えない。
   `#[non_exhaustive]` な enum への variant 追加は、仕様の「エラー」節に
   列挙されたものに限る。それ以外が必要なら作業を止めて理由を報告する。
6. 層の依存方向を守る: domain ← layout, policy ← application。
   domain は crate 内の他モジュールを use しない。
7. 反復順が決まらないコレクション（HashMap / HashSet）を使わない。

## 作業の進め方
- 着手前に `docs/core-spec.md` と、そのフェーズの `tests/spec/` と
  `src/verification/spec/` を全部読む。
- 小さく書く → `scripts/verify.sh` → 直す、を繰り返す。
- 仕様が曖昧なら推測で埋めない。`docs/questions.md` に追記し、
  その項目に依存しない作業だけを続ける。依存する作業は止める。
- 完了前に `scripts/verify.sh --full` を実行し、末尾 30 行を報告に貼る。
- コミットは `core(pN): <要約>` 形式。小さい単位で複数回コミットしてよい。push はしない。

## コマンド
- `scripts/verify.sh`          fmt / clippy / no_std ビルド / 層検査 / test（256 ケース）
- `scripts/verify.sh --full`   上記 + test 10,000 ケース + 差分ミュータント
- `cargo test -p contrast-core --test spec <名前>`  仕様テスト（tests/spec/main.rs が入口。名前でフィルタ）

## 書き方
- 公開・非公開を問わず全項目に doc コメント。モジュール冒頭に役割と
  守る不変条件（I1〜I8 の番号）を書く。
- 状態を変える処理は `application/handlers/` に 1 ユースケース 1 ファイル。
  handler は複製された State を `&mut` で受け、`Result<Vec<Effect>, CoreError>`
  を返す。差し替えは `Core::apply` だけが行う。
- エラーは `CoreError` の variant で表す。文字列エラーは使わない。
- 自分で足すテストは `crates/contrast-core/tests/impl_<対象>.rs` か
  `src` 内の `#[cfg(test)]` に置く。

## 報告の形式
1. 何を実装したか（ファイル単位）
2. 追加したテストとハーネス
3. `verify.sh --full` の末尾 30 行
4. 仕様上の疑問（`docs/questions.md` に書いたもの）と、それで止めた作業
