# contrast-core 仕様の追補案: 17 節 複数モニタ（P11）

> `docs/core-spec.md` は人間だけが編集する（`CLAUDE.md` 絶対規則 1、`.claude/settings.json` の deny）。
> この作業（proto/niri-complete）では編集が拒否されたので、追補する 17 節をここに置く。
> 内容は `docs/core-spec.md` の 16 節の後ろにそのまま足すことを想定している。人間が確認して移すまで、
> `tests/niri_parity/multi_monitor.rs` とその実装は、この文書を仕様として扱う。

## 17. 複数モニタ（P11。niri-complete。16 節の続き）

作者の niri の `focus-monitor-*`・`move-column-to-monitor-*` と、モニタごとのワークスペースの列に合わせる。
根拠は niri のソース `src/layout/mod.rs`（`add_output`・`remove_output`・`focus_output`・`move_column_to_output`・`move_to_output`）、`src/layout/monitor.rs`（`Monitor::new`・`append_workspaces`・`add_column`・`add_tile`・`clean_up_workspaces`）、`src/layout/workspace.rs`（`original_outputs`・`find_preferred_output`）、`src/niri.rs`（`output_left_of` ほか 4 方向）、`src/input/mod.rs`（`FocusMonitor*`・`MoveColumnToMonitor*`）。調査は `.cc-logs/report-niri-complete.md`。

- **D72 範囲**: `Dynamic` のときだけ、ワークスペースを出力ごとの列にする。`Fixed` は D1（全出力で共有）のままで、ここまでの動作・仕様テストは変わらない。16 節の「出力 1 枚を前提」は、この節で複数に広げる。出力 1 枚のときの動作は 16 節と同じ。
- **D73 参照と位置**: 窓の所属と出力の表示は、ワークスペースの id（`WorkspaceId`）で持つ。Action（`SwitchWorkspace`・`MoveToWorkspace`・`MoveColumnToWorkspace`）と `Placement.workspace`・`PlacementContext` の `WorkspaceIndex` は、`Dynamic` では**アクティブな出力の列の中の位置**（出力が 0 台なら、持ち主の無い列の中の位置）。`Fixed` では従来どおり全体の位置。StateView の既存の `workspace_count`・`workspace_id(index)`・`workspace_windows(index)`・`shown_workspace`・`WindowView.workspace` は**全体の並び**の位置で、`Dynamic` の全体の並びは「出力を OutputId 昇順に、各出力の列を順に」（出力が 0 台なら持ち主の無い列）。出力 1 枚なら、その出力の列と同じ。
- **D74 出力の列（`Dynamic`）**: 各ワークスペースは、持ち主の出力（出力が 0 台のときは無し）と、元の出力（出力の名前。無いこともある）を持つ。出力の列は、持ち主がその出力のワークスペースの並び。D64（末尾の空）と D65（整理）は出力ごとに行う: 各出力の列の末尾は常に空（すべての出力で、すべての命令の最後に補う）。整理は「末尾以外で、窓が無く、その出力が表示していない」ワークスペースを消し、niri と同じく、表示が変わった出力の列だけに行う: `SwitchWorkspace`・`MoveToWorkspace`・`MoveColumnToWorkspace`・`MoveWorkspaceDown/Up` はアクティブな出力、`OutputRemoved` は移し先、`MoveColumnToMonitor` で窓だけを移したとき（Floating・Fullscreen。niri の `move_to_output`）は元の出力。列ごとに移したとき（niri の `move_column_to_output`）は整理しない。出力が 0 台のときは、持ち主の無い列に対して同じ規則で行う。新しく作る空のワークスペースの元の出力は、持ち主の出力（持ち主が無ければ無し）。窓がワークスペースに入る（`WindowMapped`・`MoveToWorkspace`・`MoveColumnToWorkspace`・`MoveColumnToMonitor`）と、そのワークスペースの元の出力を、持ち主の出力の名前にする（持ち主が無ければ変えない）。
- **D75 アクティブな出力（`Dynamic`）**: 状態に「アクティブな出力」を持つ。出力が 1 台以上あれば、必ずそのどれか。`Dynamic` の D5 のフォーカス出力は、アクティブな出力。どの命令でも、フォーカスを窓に移すと、アクティブな出力はその窓を表示している出力になる。
- **D76 出力の追加（`Dynamic`）**:
  - 出力が 0 台だったとき: 持ち主の無いワークスペースのうち、窓のあるものと末尾の空を、この順のまま新しい出力の列にする（末尾以外の空は消す）。新しい出力がアクティブになり、主の出力（D77）になる。
  - 1 台以上あったとき: 既存の出力ごと（OutputId 昇順）に、列を末尾から先頭へ見て、元の出力が新しい出力の名前のワークスペースを、その列から外す。外した位置がその出力の表示中の位置以下なら、表示中の位置を 1 つ上げる（0 で止まる）。外したもののうち窓のあるものを、元の並びのまま新しい出力の列にし、末尾に新しい空を足す。アクティブな出力は変わらない。
  - どちらも、新しい出力の表示は、D77 で同じ名前に記録したワークスペースがその列にあればそれ、なければ先頭。最後に、フォーカスがアクティブな出力の表示中のワークスペースの窓でなければ（None を含む。niri ではフォーカスは常にアクティブなモニタのアクティブな窓）、そのワークスペースの、D12 の規則のフォーカス（履歴の最新、なければ順序列の先頭、なければ None）にする。D21（`OutputAdded` はフォーカスを与えない）は `Fixed` にだけ当てはまる。
- **D77 出力の削除（`Dynamic`）**: 削除する出力の名前に、その出力が表示していたワークスペースの id を記録する（D76）。その出力の列のうち窓のあるワークスペースを、元の出力の名前の出力が残っていればそこへ、なければ主の出力へ、元の並びのまま、移し先の末尾の空の直前に入れる（空のワークスペースは消える）。移し先が末尾の空を表示していたら、表示は末尾の空のまま。そのあと、移し先を整理する（D74）。
  - 主の出力は、最初に足した出力。主の出力とアクティブな出力は、出力を OutputId 昇順に並べたときの位置 `p` で持つとみなし、削除した出力の位置 `d` について `p >= d` なら `p − 1`（0 で止まる）にして、削除後の並びで指し直す。
  - 最後の出力を削除したら、窓のあるワークスペースをすべて持ち主の無い列にし、末尾に空を足す。
  - フォーカスは D76 の最後と同じ規則で直す（出力が 0 台なら None）。
- **D78 `FocusMonitor(Direction)`（新しい Action）**: アクティブな出力から見て、その方向にある出力をアクティブにする。候補は、矩形の中心（`x + 幅 / 2`、`y + 高さ / 2`。整数の切り捨て）がその方向にあり（`Left` なら中心の x が小さい）、直交する軸で矩形が重なる（端が接するだけは重ならない）出力。そのうち、その軸の中心の距離が最小のもの（同じなら OutputId が小さい方）。候補が無ければ状態不変で `Ok`。移したら、フォーカスはその出力の表示中のワークスペースの、D12 の規則のフォーカス。`Fixed` では状態不変で `Ok`。
- **D79 `MoveColumnToMonitor(Direction)`（新しい Action）**: `Scrolling` かつ `Dynamic` のときだけ動き、それ以外は状態不変で `Ok`。D78 と同じ規則で相手の出力を決め、無ければ状態不変で `Ok`。フォーカスが None なら `NoFocusedWindow`。フォーカス窓が Tiled（列に属する）なら列ごと（幅・窓ごとの高さ・最大化の状態を保つ）を、相手の表示中のワークスペースの「最後にフォーカスした列」の右隣（窓が無ければ先頭）へ入れる。Floating・Fullscreen は窓だけを移す（Fullscreen なら D9 で相手の全画面を戻す）。フォーカスは窓のままで、アクティブな出力は相手になる。元のワークスペースは、空になっても表示中なので残る。
- **D80 `Command::OutputFocusRequested(ExternalKey)`（新しい Command）**: 窓の無い所のクリックなど。`Dynamic` では、その出力をアクティブにし、フォーカスをその出力の表示中のワークスペースの、D12 の規則のフォーカスにする。すでにアクティブなら状態不変で `Ok`。未登録のキーは `UnknownKey`。`Fixed` では（キーの検査のあと）状態不変で `Ok`。
- **D81 ワークスペースの操作（`Dynamic`）**: `SwitchWorkspace`・`MoveToWorkspace`・`MoveColumnToWorkspace`・`MoveWorkspaceDown/Up` は、アクティブな出力の列の中で動く（D66 の丸めも、その列の個数で）。D12 の「別の出力が表示中なら入れ替える」は `Fixed` だけ（`Dynamic` では、列の中のワークスペースは他の出力に表示されない）。
- **D82 読み取り（StateView）**: `active_output() -> Option<OutputId>`（`Dynamic` はアクティブな出力、`Fixed` は D5）、`output_workspaces(OutputId) -> Vec<WorkspaceId>`（`Dynamic` はその出力の列、`Fixed` は全体の並び）、`workspace_windows_by_id(WorkspaceId)`、`workspace_output(WorkspaceId) -> Option<OutputId>`（`Dynamic` は持ち主、`Fixed` は表示している出力）、`WindowView.workspace_id` を足す。
- **D83 不変条件 I13（`Dynamic`）**: 出力が 1 台以上なら、すべてのワークスペースは存在する出力を持ち主に持ち、各出力の列は 1 個以上で末尾が空、各出力の表示は自分の列のワークスペース、アクティブな出力は存在する出力。出力が 0 台なら、どのワークスペースも持ち主を持たず、アクティブな出力は無い。フォーカスが Some なら、それはアクティブな出力の表示中のワークスペースの窓。
- **D84 niri との差（記録）**: ワークスペースの名前・`open-on-output`・`empty-workspace-above-first` は扱わない。出力の照合は名前だけ（niri は make・model・serial でも照合する）。niri の、ワークスペース切替のアニメーション中に出力が増減したときの扱い（切替を止めて整理する）は無い（core にアニメーションが無い。D71）。`WindowFocusRequested` は D16 のまま（niri の `activate_window` は、隠れたワークスペースの窓なら、そこへ切り替える）。`focus-monitor-previous/next` などの他の出力の Action は、作者の設定に無いので足さない。
  また、niri の `add_output` は、移したワークスペースが元の出力の列の末尾の空だったとき（`move-workspace-*` で末尾に来た、元の出力が別の空のワークスペース）、元の出力の列に新しい空を足さない。Contrast は D74 のとおり、すべての命令の最後に末尾の空を補う（proptest で見つけた。2026-11-10）。
