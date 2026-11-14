#!/usr/bin/env bash
# Contrast の実機（DRM）確認を 1 回で行うためのスクリプト。
# 別の VT（Ctrl+Alt+F3 など）にログインして、そこで実行する。ホストの niri は止めなくてよい。
# 実行場所は問わない。全出力は ~/contrast-realhw-run-<日時>.txt に残り、
# ログの要点は ~/contrast-realhw-<日時>.txt に書かれる（TTY ではコピーできないため、ファイルに出す）。
set -u

REPO="${CONTRAST_REPO:-$HOME/RustroverProjects/contrast-wm}"
CONFIG="$REPO/config/config.cdl"
STAMP="$(date +%Y%m%d-%H%M%S)"
DIGEST="$HOME/contrast-realhw-$STAMP.txt"
RUNLOG="$HOME/contrast-realhw-run-$STAMP.txt"

# 以後の出力を画面とファイルの両方に出す
exec > >(tee "$RUNLOG") 2>&1

die() { echo "contrast-realhw: $*" >&2; exit 1; }

# --- 前提の確認 ---------------------------------------------------------
if [ "${XDG_SESSION_TYPE:-}" != "tty" ]; then
  die "VT のログインシェルではありません（XDG_SESSION_TYPE=${XDG_SESSION_TYPE:-未設定} VT=${XDG_VTNR:-?}）。Ctrl+Alt+F3 などでログインして実行してください。"
fi
# tty のセッションなので、環境に残ったホストの値は外す（子がホストにつながらないように）
unset WAYLAND_DISPLAY DISPLAY

[ -d "$REPO" ] || die "リポジトリが見つかりません: $REPO（CONTRAST_REPO で指定できます）"
[ -f "$CONFIG" ] || die "設定が見つかりません: $CONFIG"

GO=""
for candidate in "$HOME/contrast-go.sh" "$REPO/contrast-go.sh" "$REPO/scripts/contrast-go.sh"; do
  if [ -f "$candidate" ]; then GO="$candidate"; break; fi
done
[ -n "$GO" ] || die "contrast-go.sh が見つかりません（$HOME、$REPO、$REPO/scripts を探しました）。"
echo "contrast-go.sh: $GO"

# --- ビルド（メモリ上限つき。この PC は 13Gi で、過去に固まっている）------------
cd "$REPO" || die "cd に失敗しました: $REPO"
echo "== build =="
systemd-run --user --scope -p MemoryMax=6G -p MemorySwapMax=0 \
  nix develop -c cargo build -p contrast || die "ビルドに失敗しました。"

# --- 起動 ---------------------------------------------------------------
START_EPOCH="$(date +%s)"
echo "== start (DRM) =="
echo "終了は Mod+Shift+E（Quit）。確認項目は報告の 2.4。"
CONTRAST_BACKEND=drm CONTRAST_CONFIG="$CONFIG" CONTRAST_EXIT_AFTER=0 bash "$GO"
STATUS=$?
echo "== exited: status=$STATUS =="

# --- ログの要点をファイルに書く（読み取りだけ。何も消さない）-------------------
LOG="$(find "$HOME" -maxdepth 1 -name 'contrast-drm-*.log' -newermt "@$START_EPOCH" 2>/dev/null | sort | tail -n 1)"
[ -n "$LOG" ] || LOG="$(ls -t "$HOME"/contrast-drm-*.log 2>/dev/null | head -n 1)"
IRONBAR_LOG="$(ls -t "$HOME"/.local/state/contrast/spawn/ironbar-*.log 2>/dev/null | head -n 1)"

{
  echo "# contrast-realhw $STAMP  (exit status: $STATUS)"
  echo "# compositor log: ${LOG:-（見つかりません）}"
  echo "# ironbar log:    ${IRONBAR_LOG:-（見つかりません）}"
  echo
  echo "## compositor: 要点（先頭 120 行）"
  if [ -n "$LOG" ]; then
    grep -nE 'loaded=|generation=|drm: |xkb: |input: (mouse|touchpad)|disable_while_trackpointing|xwayland: |screenshot: |spawn failed|WARN|ERROR|panic' "$LOG" | head -n 120
  fi
  echo
  echo "## ironbar: workspaces（末尾 30 行）"
  if [ -n "$IRONBAR_LOG" ]; then
    grep -nE 'Getting workspace client|WorkspaceActivated|WorkspacesChanged|Init\(|unsupported' "$IRONBAR_LOG" | tail -n 30
  fi
} > "$DIGEST" 2>&1

cat "$DIGEST"
echo
echo "要点: $DIGEST"
echo "全出力: $RUNLOG"
echo "コンポジタのログ全文: ${LOG:-（なし）}"
