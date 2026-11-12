#!/usr/bin/env bash
# 使い方: scripts/verify.sh [--full]   非破壊: 検査のみ
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
full=0; [[ "${1:-}" == "--full" ]] && full=1

# 失敗したら、どのステップで止まったかを最後に出す。
step="tools"
trap 'echo "verify: 失敗: $step" >&2' ERR

# 道具が無いときは、cargo の「no such command」ではなく何が足りないかを出して止まる。
missing=0
need() { # $1=表示名  $2...=有無を確かめるコマンド
  local name="$1"; shift
  if ! "$@" >/dev/null 2>&1; then
    echo "verify: 道具が無い: $name（確認: $*）" >&2; missing=1
  fi
}
echo "== $step"
need rustfmt cargo fmt --version
need clippy cargo clippy --version
need "x86_64-unknown-none の core/alloc" \
  test -d "$(rustc --print sysroot)/lib/rustlib/x86_64-unknown-none"
if (( full )); then
  need cargo-mutants cargo mutants --version
fi
if (( missing )); then
  echo "verify: 失敗: $step（上の道具を入れてから再実行。rust-toolchain.toml）" >&2
  exit 1
fi

step=fmt;      echo "== $step"; cargo fmt --all -- --check
step=clippy;   echo "== $step"; cargo clippy -p contrast-core --all-targets --all-features -- -D warnings
step=no_std;   echo "== $step"; cargo build -p contrast-core --target x86_64-unknown-none
step=layering; echo "== $step"; scripts/check-layering.sh

if (( full )); then
  step="test (10000)"; echo "== $step"; PROPTEST_CASES=10000 cargo test -p contrast-core --all-features
  step="mutants (diff)"; echo "== $step"
  diff_file="$(mktemp)"
  git diff main...HEAD > "$diff_file"
  if [[ -s "$diff_file" ]]; then
    cargo mutants -p contrast-core --in-diff "$diff_file"
  else
    echo "main との差分なし: mutants を省略"
  fi
else
  step="test";         echo "== $step"; PROPTEST_CASES=256 cargo test -p contrast-core --all-features
fi
echo "verify: OK"
