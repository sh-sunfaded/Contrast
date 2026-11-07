# contrast-core 仕様 v0.1

contrast-core は Contrast の純粋な中核クレート。Smithay・I/O・CDL のどれにも依存しない
決定的な状態遷移エンジンで、構成は「関数型コア／命令型シェル」。
Smithay 側（シェル）は Wayland イベントを `Command` に翻訳して渡し、
core が返す `Effect` を順に実行するだけにする。

公開 API の中心は全関数 `Core::apply(Command) -> Result<Applied, CoreError>` の 1 本。
不変条件は常に成り立ち、Err を返した Command は状態を一切変えない。core は決してパニックしない。

> 本書は Contrast 仕様 v0.6 と食い違う場合、v0.6 を正として本書を直す。

## 1. 責務と非責務

責務:
- 出力（モニタ）・ワークスペース・窓の所有関係の管理
- レイアウト計算（各窓の矩形の決定）
- フォーカスとナビゲーション
- 意味操作 `Action`（キーバインド解決後）の解釈
- ポリシーへの問い合わせ（初期配置、浮動判定など）と結果の適用

core に入れないもの: Wayland プロトコル、バッファ、描画、DRM / libinput（contrast-backend-smithay）、
CDL のパースと評価（contrast-policy-cdl）、キーシンボル→Action 変換、設定ファイル読み込み、
ログ出力、時刻、乱数。

## 2. クレート構成と依存方向

```text
contrast (bin) ──┬──> contrast-policy-cdl ──> cdl（別リポジトリ）
                 │            │
                 │            v
                 ├──────> contrast-core   （依存クレート 0, no_std）
                 │            ^
                 │            │
                 └──> contrast-backend-smithay ──> smithay
```

矢印は「依存する」向き。contrast-core はどこにも依存しない。

## 3. ドメインモデル

| 型 | 意味 | 主な属性 |
| --- | --- | --- |
| `Output` / `OutputId` | 物理または仮想モニタ | 論理矩形、作業領域、表示中ワークスペース |
| `Workspace` / `WorkspaceId` / `WorkspaceIndex` | 窓の集合と並び | インデックス、窓の順序列、フォーカス履歴 |
| `Window` / `WindowId` | トップレベル窓 | `WindowMeta`、状態（Tiled / Floating / Fullscreen）、浮動時の矩形 |
| `Point` / `Size` / `Rect` | 論理座標の幾何 | i32 座標、幅と高さは 1 以上 |
| `ExternalKey` | シェル側の識別子 | 不透明な u64。core は ID と対応づけるだけ |
| `EntityId` | 登録結果 | `Output(OutputId)` / `Window(WindowId)` |
| `Command` / `Action` | core への入力 | 5 節参照 |
| `Effect` | core からの出力 | 5 節参照 |

- ID は u64 の newtype。`IdGen` が単調増加で払い出し、再利用しない。上限到達は `Err(IdExhausted)`。
- コレクションは反復順が決定的な `BTreeMap` / `BTreeSet` / `Vec` だけを使う。
- `WindowMeta { app_id: Option<String>, title: Option<String>, is_dialog: bool, requested_size: Option<Size> }`
- `CoreConfig { workspace_count: u8 }`（1〜32、既定 9）。範囲外は `Core::new` が 1〜32 に丸める。

### ID と対応表（P2 で確定）

- `IdKind` トレイト（`from_raw` / `raw`）を 3 つの ID 型（OutputId / WindowId / WorkspaceId）が実装する。
- `IdGen<T>::new()` は raw = 1 から払い出す。`starting_at(n)` はテスト用。
  現在値が `u64::MAX` のとき `Err(IdExhausted)`（以後も同じ）。
- `KeyMap<T>` は key と id の双方向表。`insert` は key か id の重複で `Err(DuplicateKey)`、
  `remove_by_key` は未登録で `Err(UnknownKey)`。Err のとき表は変わらない。

## 4. 不変条件

`check_invariants()` が I1〜I6 を検査し、テストから呼ぶ。I7・I8 は状態機械テストで確認する。

- **I1** 窓はちょうど 1 つのワークスペースに属する。
- **I2** ワークスペースの窓順序列に重複がなく、列中の ID はすべて存在する窓を指す。
- **I3** 各出力は表示中ワークスペースをちょうど 1 つ持ち、1 つのワークスペースが 2 つの出力に同時表示されることはない。
- **I4** フォーカスは None か、いずれかの出力に表示中のワークスペース上の窓を指す。
- **I5** 表示中ワークスペースの Tiled 窓の矩形は、その出力の作業領域内に収まり、互いに重ならない。
- **I6** ID は単調増加で再利用しない。`ExternalKey` と ID の対応は全単射。
- **I7** `apply` が Err を返したとき、状態は呼び出し前と等しい（強い例外保証）。
- **I8** 同じ状態と同じ Command からは、同じ状態と同じ Effect 列が得られる（決定性）。

I7 は「状態を複製して新状態を組み立て、成功時だけ差し替える」実装で満たす。

## 5. 公開 API（v0.1）

```rust
pub struct Core<P: Policy, L: Layout> { /* private */ }

impl<P: Policy, L: Layout> Core<P, L> {
    pub fn new(config: CoreConfig, policy: P, layout: L) -> Self;
    pub fn apply(&mut self, cmd: Command) -> Result<Applied, CoreError>;
    pub fn view(&self) -> StateView<'_>; // 読み取り専用
    #[cfg(any(test, feature = "invariants"))]
    pub fn check_invariants(&self) -> Result<(), InvariantViolation>;
    #[cfg(any(test, feature = "invariants"))]
    pub fn snapshot(&self) -> StateSnapshot; // PartialEq + Debug。I7 の比較用
}

pub struct Applied { pub effects: Vec<Effect> }

#[non_exhaustive]
pub enum Command {
    OutputAdded { key: ExternalKey, name: OutputName, geometry: Rect },
    OutputRemoved(ExternalKey),
    OutputChanged { key: ExternalKey, geometry: Rect },
    WindowMapped { key: ExternalKey, meta: WindowMeta },
    WindowUnmapped(ExternalKey),
    WindowMetaChanged { key: ExternalKey, meta: WindowMeta },
    Action(Action),
}

#[non_exhaustive]
pub enum Action {
    FocusDirection(Direction),
    MoveWindow(Direction),
    SwitchWorkspace(WorkspaceIndex),
    MoveToWorkspace(WorkspaceIndex),
    ToggleFloating,
    ToggleFullscreen,
    CloseFocused,
}

pub enum Direction { Left, Right, Up, Down }

#[non_exhaustive]
pub enum Effect {
    Registered { key: ExternalKey, id: EntityId },
    ShowWorkspace { output: OutputId, workspace: WorkspaceId },
    Configure { window: WindowId, rect: Rect, state: WindowState },
    SetFocus(Option<WindowId>),
    RequestClose(WindowId),
    Damage(OutputId),
}

pub trait Policy {
    fn place(&self, meta: &WindowMeta, ctx: &PlacementContext<'_>) -> Placement;
}

pub struct Placement {
    pub workspace: Option<WorkspaceIndex>, // None = フォーカス出力の表示ワークスペース
    pub state: WindowState,
    pub focus: bool,
}

pub trait Layout {
    fn arrange(&self, area: Rect, windows: &[WindowId], out: &mut Vec<(WindowId, Rect)>)
        -> Result<(), LayoutError>;
}
```

`PlacementContext` は読み取り専用で、フォーカス中のワークスペースインデックス、ワークスペース数、出力数を持つ。

### Effect 列の順序（固定）

1. Registered
2. ShowWorkspace（OutputId 昇順）
3. Configure（WindowId 昇順）
4. SetFocus（フォーカスが変わったときだけ、最大 1 個）
5. RequestClose
6. Damage（OutputId 昇順、重複なし）

Configure は「表示中ワークスペースにあり、矩形か状態が変わった窓」にだけ出す。

## 6. エラー

`CoreError` は `#[non_exhaustive]` の enum。v0.1 で許す variant は次がすべて。

| variant | 発生条件 |
| --- | --- |
| `UnknownKey` | 登録されていない `ExternalKey` への操作 |
| `DuplicateKey` | 登録済み `ExternalKey` の再登録 |
| `NoFreeWorkspace` | 出力追加時に未表示のワークスペースが残っていない |
| `WorkspaceIndexOutOfRange` | `WorkspaceIndex` が `workspace_count` 以上 |
| `NoFocusedWindow` | 対象窓が必要な Action でフォーカスが None |
| `IdExhausted` | ID の上限到達 |
| `GeometryOverflow` | 幾何計算が i32 に収まらない |
| `Layout(LayoutError)` | レイアウト計算の失敗 |

`UnknownKey` はシェル側の競合（閉じかけの窓への操作など）で普通に起こる。シェルはログだけ出して続行する。
`LayoutError` は `AreaTooSmall` のみ。

## 7. 暫定決定（v0.6 との突き合わせまで有効）

実装者は推測で埋めず、ここに従う。変更は人間だけが行う。

- **D1** ワークスペースは全出力で共有するグローバル集合。`Core::new` が `workspace_count` 個を生成し、増減しない。
- **D2** 出力追加時、その出力は「未表示のワークスペースのうち最小インデックス」を表示する。なければ `NoFreeWorkspace`。
- **D3** 出力削除時、その出力が表示していたワークスペースは未表示になる。窓はワークスペースに残る。フォーカスがそこにあれば、残る出力のうち OutputId 最小の表示ワークスペースの順序列先頭の窓へ移り、なければ None。
- **D4** 作業領域は出力の論理矩形と同じ（layer-shell による予約は v0.1 では扱わない）。
- **D5** フォーカス出力 = フォーカス窓のある出力。フォーカスが None なら OutputId 最小の出力。出力 0 台ならなし。
- **D6** 出力 0 台でも `WindowMapped` は成功する。配置先は `Placement.workspace`、None ならワークスペース 0。表示されていないので Configure は出さない。
- **D7** 新しい窓は `Placement.focus` が true で、かつ配置先が表示中ならフォーカスを得る。順序列では末尾に追加する。
- **D8** Floating 窓は I5 の対象外（重なり可）。矩形は `requested_size`（なければ作業領域の半分）を作業領域中央に置き、出力の論理矩形と最低 1px 交差するよう切り詰める。
- **D9** Fullscreen 窓の矩形は出力の論理矩形全体。I5 の対象外。1 ワークスペースに Fullscreen は高々 1 枚で、2 枚目を全画面にすると 1 枚目は元の状態に戻る。
- **D10** `FocusDirection` は、フォーカス窓の矩形中心から見てその方向（Left なら中心 x が小さい側）にある表示中の窓のうち、中心間のマンハッタン距離が最小のもの。同距離は WindowId 最小。候補なしは状態不変で `Ok`（Effect 空）。
- **D11** `MoveWindow` は Tiled 窓の順序列での入れ替え。Left / Up は 1 つ前、Right / Down は 1 つ後と交換。端なら状態不変で `Ok`。Floating / Fullscreen では状態不変で `Ok`。
- **D12** `SwitchWorkspace(i)` はフォーカス出力の表示を i にする。i が別の出力で表示中なら 2 出力の表示を入れ替える。切替後のフォーカスは i のフォーカス履歴の最新、なければ順序列先頭、なければ None。
- **D13** `MoveToWorkspace(i)` はフォーカス窓を i の順序列末尾へ移す。フォーカスは D3 と同じ規則で元のワークスペース内で移る（窓に付いていかない）。
- **D14** `CloseFocused` は `RequestClose` を出すだけ。窓の削除はシェルが `WindowUnmapped` を送ったときに行う。
- **D15** `Damage` は、表示内容（窓の矩形・状態・表示ワークスペース・フォーカス）が変わった出力すべてに出す。

## 8. 依存とコード規約

- `#![no_std]` + `extern crate alloc`。
- `#![forbid(unsafe_code)]`。
- 実行時依存クレートは 0。dev-dependencies は `proptest` と `proptest-state-machine` のみ。
- 算術は `checked_*` / `saturating_*` だけ。clippy の `arithmetic_side_effects`、`unwrap_used`、
  `expect_used`、`panic`、`indexing_slicing`、`missing_docs_in_private_items` を deny。
- テストコードでは `clippy.toml` で `allow-unwrap-in-tests` と `allow-expect-in-tests` を true にする。
- 公開型はすべて `#[non_exhaustive]`（構造体は必要に応じてコンストラクタを用意）、公開項目はすべて doc コメント必須。

## 9. モジュール構成

依存は上から下への一方向だけ許す（domain は他のどの層も use しない）。

```text
crates/contrast-core/src/
├── lib.rs            公開APIの再エクスポートのみ
├── domain/           エンティティと値オブジェクト
│   ├── geometry.rs   Point, Size, Rect
│   ├── id.rs         各ID, IdGen, ExternalKey, KeyMap
│   ├── output.rs
│   ├── workspace.rs
│   ├── window.rs
│   └── state.rs      State（全エンティティの集約）
├── layout/           Layout trait（mod.rs）と参照実装 columns.rs
├── policy/           Policy trait（mod.rs）と DefaultPolicy
├── application/      Core, Command, Action, Effect
│   └── handlers/     1ファイル1ユースケース
├── invariant.rs      check_invariants, InvariantViolation
└── error.rs          CoreError, LayoutError
```

層の依存方向は `scripts/check-layering.sh` が CI で検査する。

## 10. P3 で確定した挙動（tests/spec/spec_core_*.rs が前提にする）

暫定決定 D1〜D15 を引き継ぎ、実装が決めていた点を仕様にする。違う意図があれば、この節とテストを一緒に直す。

- **D16 Command**: `WindowFocusRequested(ExternalKey)` を Command に加える。表示中のワークスペースの窓ならフォーカスを移す。非表示の窓は状態不変で Ok。未登録は `UnknownKey`。
- **D17 窓が多すぎるとき**: Tiled 窓の数が作業領域の幅（px）を超えると `Layout(AreaTooSmall)`。WindowMapped、OutputChanged（縮小）、ToggleFloating、MoveToWorkspace、SwitchWorkspace、OutputAdded のどれでも同じ。Err のとき状態は変わらない（OutputChanged が失敗したら、出力の矩形は古いまま）。シェルは Err をログに出して続行する。
- **D18 窓の削除とフォーカス**: フォーカス窓の WindowUnmapped は、同じワークスペースの順序列で右隣へ、末尾なら左隣へ、窓がなければ None にする（Floating・Fullscreen も対象）。ほかの窓の削除でフォーカスは変わらない。
- **D19 ワークスペースへの復帰**: 表示に戻った窓の矩形と状態が前回の Configure と同じなら、Configure は出さない（ShowWorkspace・SetFocus・Damage だけ）。シェルは、窓の最後の設定を保持する。
- **D20 浮動矩形（D8 を改める）**: 前回の浮動矩形が作業領域と交差していればそのまま保つ。なければ `requested_size`（なければ作業領域の半分）を作業領域に収まるよう縮め、中央に置く。
- **D21 OutputAdded**: フォーカスを与えない。
- **D22 Action の端**: 次はすべて状態不変で Ok（Effect なし）。Fullscreen の窓への ToggleFloating、表示中のワークスペースへの SwitchWorkspace、出力 0 台での SwitchWorkspace、現在のワークスペースへの MoveToWorkspace、Floating・Fullscreen の窓への MoveWindow、端での MoveWindow、候補のない FocusDirection。MoveWindow の前後は Tiled の窓だけを数える（Left・Up が前、Right・Down が後）。MoveToWorkspace は、インデックスの範囲、次にフォーカスの有無の順に検査する。
- **D23 WindowMetaChanged**: `meta` を差し替えるだけで、再配置・状態の変更・Effect はない。配置の判断は map のときだけ行う。
- **D24 出力の削除**: 窓は残り、表示されていたワークスペースは未表示になる。削除された出力への ShowWorkspace・Damage は出さない。同じキーの再追加は新しい OutputId になり、最小インデックスの未表示ワークスペースを表示する。
- **D25 FocusDirection の候補**: フォーカス窓以外の表示中の全窓（Floating・Fullscreen・他の出力の窓を含む）。フォーカス窓に矩形がなければ状態不変で Ok。
- **D26 不変条件 I9**: 同じワークスペースの Fullscreen の窓は高々 1 枚。`check_invariants` で検査する。
- **D27 I7・I8 の確認**: 任意の Command 列に対するプロパティテスト（`spec_core_properties.rs`）で確認する。確認項目は、毎ステップ I1〜I6 と I9、Err なら状態不変（I7）、同じ列を新しい Core に流すと結果と状態が一致（I8）、Effect の順序、Configure と SetFocus が適用後の状態と一致すること、SetFocus は変わったときだけ出ること。
- **D28 公開 API**: `StateView`・`WindowView`・`InvariantViolation`・`StateSnapshot` は現状の形で確定する。`Point` は公開フィールドと `new` だけを持つ（`x()`・`y()` は作らない）。「公開型は `#[non_exhaustive]`」の対象は、Command・Action・Effect・Placement・WindowMeta などのデータ型で、Core・StateView・IdGen・KeyMap などの振る舞いの型は対象外。`domain` は `crate::error` を use してよい。
- **D29 Damage（D15 の確認）**: フォーカスだけが変わったときも、その出力に Damage を出す。

## 11. 作業領域の余白（Insets。D4 を改める）

layer-shell のバーなどが確保する領域を、出力の作業領域から除く。余白が 0 のときは、D4（作業領域 = 出力の論理矩形）と同じ。

- **D30 Insets**: `Insets { top, right, bottom, left }`（i32、すべて 0 以上）。フィールドは非公開で、`Insets::new(top, right, bottom, left) -> Option<Insets>`（負の値があれば None）、`top()` `right()` `bottom()` `left()`、`Default`（すべて 0）を持つ。
- **D31 Command**: 出力ごとに insets を持つ。OutputAdded の時点では 0。`Command::OutputInsetsChanged { key: ExternalKey, insets: Insets }` で置き換える。未登録のキーは `UnknownKey`。同じ値なら、状態不変で Ok（Effect なし）。
- **D32 作業領域**: 出力の論理矩形から insets を引いた矩形。幅または高さが 1 未満になるときは `Layout(AreaTooSmall)` で、状態は変わらない。これは OutputInsetsChanged と、insets を持つ出力への OutputChanged の両方で検査する。窓が 1 枚もなくても検査する。
- **D33 使う場所**: Tiled の窓の配置、Floating の窓の配置（D8・D20）、不変条件 I5 は、作業領域を使う。Fullscreen の窓は insets を無視して、出力の論理矩形全体を使う（D9 のまま）。
- **D34 再配置**: insets が変わると、窓を並べ直す。Effect は通常の規則（矩形が変わった窓の Configure、Damage）に従う。

## 12. スクロール列レイアウト（N3a。niri に合わせる）

`LayoutMode::Scrolling` は、niri のように、列が横に並び、ビューが横にスクロールするレイアウト。既定は `Equal`（既存の等分。これまでの仕様のまま）。窓の高さの操作・`center-column`・ビューのアニメーションは N3b 以降（D48）。

- **D35 設定**: `CoreConfig` に `with_layout(LayoutMode)`（`LayoutMode::{Equal, Scrolling}`、既定 `Equal`）、`with_default_column_width(Ratio)`（既定 1/2）、`with_preset_column_widths(Vec<Ratio>)`（既定 1/3・1/2・2/3。空なら既定のまま）を足す。`Ratio::new(num, den) -> Option<Ratio>`（`1 <= num <= den`）と `num()` `den()`、`ColumnWidth::{Proportion(Ratio), Fixed(i32)}`（`ColumnWidth::fixed(px) -> Option<ColumnWidth>`、px は 1 以上）を持つ。`Scrolling` のとき、`Layout` 型（`Columns` など）は使わない。
- **D36 構造**: `Scrolling` のワークスペースは、列の並び（左から右）を持つ。列は、Tiled の窓の並び（上から下）、幅、「最後にフォーカスした窓」を持つ。Floating・Fullscreen の窓は列に属さない。
- **D37 幅**: `Proportion(r)` の列の幅は、作業領域の幅 × r（切り捨て、1 以上）。`Fixed(px)` は px。新しい列の幅は `default_column_width`。作業領域より広い列を許す。
- **D38 配置**: 列は、作業領域の x = 0 から隙間なく、左から右へ並ぶ。窓の矩形は `(作業領域.x + 列の x − view_x, 作業領域.y + 上の窓の高さの合計, 列の幅, 窓の高さ)`。列の中の窓の高さは等分（余りは上から 1 px ずつ）。ビューの外にある窓にも矩形を持ち、Configure を出す。
- **D39 ビュー**: ワークスペースごとに `view_x`（0 以上の整数）を持ち、ワークスペースを切り替えても保つ。フォーカス列が見える最小のスクロールにする: 列の左端 < `view_x` なら列の左端へ、列の右端 > `view_x` + 作業領域の幅 なら（右端 − 作業領域の幅）へ。列の幅が作業領域の幅以上なら、列の左端を合わせる。最後に、`view_x` を `0` 以上、`max(0, 総幅 − 作業領域の幅)` 以下に収める。列の追加・削除・移動・幅の変更・フォーカスの移動のたびに適用する。
- **D40 窓の追加**: Tiled の窓は、対象のワークスペースの、最後にフォーカスされた列の右隣に、新しい列として入る（列がなければ先頭）。フォーカスは新しい窓に移る（D7）。
- **D41 窓の削除**: 列の最後の窓なら、列も削除する。フォーカス窓の削除後のフォーカスは、列に窓が残るなら下の窓（なければ上）、列ごと消えたなら右の列（なければ左）の「最後にフォーカスした窓」。他の窓の削除では、フォーカスは変わらない。
- **D42 操作**: `Scrolling` では、`FocusDirection(Left/Right)` は隣の列の「最後にフォーカスした窓」へ、`FocusDirection(Up/Down)` は列の中の上下の窓へ移す。`MoveWindow(Left/Right)` はフォーカス列を隣の列と入れ替え、`MoveWindow(Up/Down)` は列の中で上下の窓と入れ替える。どれも、端では状態不変で Ok（Effect なし）。新しい Action `FocusColumnFirst`・`FocusColumnLast`・`MoveColumnToFirst`・`MoveColumnToLast` も、すでにその位置なら状態不変で Ok。フォーカスは、移した窓・列についていく。
- **D43 積み**: `ConsumeWindowIntoColumn` は、右隣の列の先頭の窓を、フォーカス列の最下段へ移す（右隣が空になれば削除する。右隣がなければ、状態不変で Ok）。`ExpelWindowFromColumn` は、フォーカス窓を、元の列の右隣の新しい列（既定の幅）へ出す（列に窓が 1 枚だけなら、状態不変で Ok）。どちらも、フォーカスは窓についていく。
- **D44 幅の操作**: `SetColumnWidth(w)` はフォーカス列の幅を `w` にする。`SwitchPresetColumnWidth` は、フォーカス列の現在の px 幅より広い最初のプリセットへ、なければ先頭のプリセットへ変える。`MaximizeColumn` は幅を `Proportion(1/1)` にし、すでに最大化されていれば、直前の幅に戻す。
- **D45 Equal**: `Scrolling` 専用の Action（`FocusColumnFirst`・`FocusColumnLast`・`MoveColumnToFirst`・`MoveColumnToLast`・`ConsumeWindowIntoColumn`・`ExpelWindowFromColumn`・`SwitchPresetColumnWidth`・`MaximizeColumn`・`SetColumnWidth`）は、`Equal` では、状態不変で Ok（Effect なし）。
- **D46 Floating・Fullscreen**: `Scrolling` でも、Floating（D8・D20）と Fullscreen（D9）は、列に属さない。Tiled に戻るときは、フォーカス列の右隣の新しい列として入る。
- **D47 不変条件**: I5 は `Scrolling` では「同じワークスペースの Tiled の窓の矩形は重ならず、縦は作業領域の範囲に収まる（横はビューの外に出てよい）」に改める。I10: `Scrolling` の各 Tiled の窓は、ちょうど 1 つの列に属し、空の列はなく、列の幅は 1 以上。I11: フォーカスが Tiled の窓にあるとき、その列は D39 の条件を満たす。
- **D48 N3a に含めないもの**: 窓の高さの操作（preset・reset・set）、`center-column`、ビューのアニメーション、`Scrolling` での `OutputChanged` による列幅の再計算の細部（`Proportion` の列は、新しい作業領域の幅で再計算する）。

## 13. 検証方針

- **守る主張は厳格 crate に限る**: `contrast-core` と `contrast-policy`（smithay に依存しない crate）が、パニックしない・決定的・`unsafe` なし、を守る。`smithay` に依存する crate（`contrast-compositor`・`contrast`）と、CDL との接続（`contrast-policy-cdl`）は、FFI や Smithay との境界を含むので、厳格 lint の対象外とする。
- **手段**: lint（`unsafe_code` の forbid、clippy の deny、`disallowed-types` / `disallowed-methods`）、設計（`HashMap` / `HashSet` を使わない、時刻・環境に触れない）、proptest（既存の `tests/spec`）。Kani と cargo-fuzz は使わない。
- **実行**: 日常は、変更した厳格 crate だけを `-p` で `clippy` と `test` にかける。区切り（マージの前）で、ワークスペース全体を検査する。
- **実機 TTY の確認**は、区切りで 1 回にまとめる。確認項目は少なくする。

## 14. 窓の高さと center-column（N3b。12 節の続き）

12 節（`Scrolling`）に、列の中の窓の高さと、列のセンタリングを足す。配分の規則は、niri の動作の完全な再現ではなく、決定的で単純な暫定の規則（使って合わなければ、ここを直す）。

- **D49 型と設定**: `WindowHeight::{Auto, Proportion(Ratio), Fixed(i32)}`（`WindowHeight::fixed(px) -> Option<WindowHeight>`、px は 1 以上）。`CoreConfig::with_preset_window_heights(Vec<Ratio>)`（既定 1/3・1/2・2/3。空なら既定のまま）。窓は高さを持つ（既定 `Auto`）。
- **D50 配分**: 列の窓を上から順に、列の高さ H（作業領域の高さ）を配る。Auto が 1 つもなければ、一番下の窓を Auto とみなす。`Proportion(r)` の要求は `H × r`（切り捨て、1 以上）、`Fixed(px)` は px。Auto でない窓は、残りの高さから「まだ高さを決めていない他の窓の数 × 1 px」を引いた値を上限に、要求の高さを取る。Auto の窓は、残りを等分する（余りは上から 1 px ずつ）。どの窓も高さは 1 以上で、合計は H になる（窓の数が H 以下のとき）。
- **D51 新しい Action**: `SetWindowHeight(WindowHeight)`（フォーカス窓の高さを設定。`Auto` はリセットと同じ）、`SwitchPresetWindowHeight`（フォーカス窓の現在の px の高さより高い最初のプリセットへ。なければ先頭へ）、`ResetWindowHeight`（`Auto` に戻す）、`CenterColumn`。
- **D52 窓の移動と高さ**: 窓が別の列へ移る（Consume・Expel など）とき、その窓の高さは `Auto` に戻る。同じ列の中での入れ替えでは、高さは窓についていく。
- **D53 CenterColumn**: フォーカス列の中心を作業領域の中心に合わせる `view_x` にする（`列の x + 列の幅 / 2 − 作業領域の幅 / 2`）。D39 のクランプ（0 以上、`max(0, 総幅 − 作業領域の幅)` 以下）を適用する。結果が今の `view_x` と同じなら、状態不変で Ok。ほかの操作では、D39 の最小のスクロールのまま。
- **D54 Equal**: 上の 4 つの Action は、`Equal` では、状態不変で Ok（Effect なし）。
- **D55 不変条件**: I10 に「列の中の窓の高さの合計は、作業領域の高さに等しい」を足す（窓の数が高さ以下のとき）。
- **D56 N3b に含めないもの**: `center-focused-column` の設定（always・on-overflow）、複数窓の高さの比の保存（niri の auto の重み）。

## 15. consume-or-expel-window-left/right（P9。niri-parity。14 節の続き）

作者の `~/.config/niri/config.kdl` の `Mod+BracketLeft`／`Mod+BracketRight`（`consume-or-expel-window-left`／`-right`）に合わせる。
niri のソース（`src/layout/scrolling.rs` の `consume_or_expel_window_left`・`consume_or_expel_window_right`、`add_tile_to_column`、`remove_tile_by_idx`）で確かめた動作。

- **D57 新しい Action**: `ConsumeOrExpelWindowLeft`・`ConsumeOrExpelWindowRight`。`Action` は `#[non_exhaustive]` なので、variant の追加は公開 API の破壊ではない。
- **D58 動作（左）**: `Scrolling` で、フォーカス窓（Tiled。列に属する）の列を `c`、列の窓の数を `n` とする。
  - `n == 1` のとき: `c` が先頭の列（0）なら、状態不変で `Ok`。そうでなければ、窓を左隣の列 `c − 1` の**最下段**へ移す。元の列は空になるので削除する。
  - `n >= 2` のとき: 窓を元の列から外し、元の列の**左隣**（インデックス `c`、元の列は `c + 1` になる）の新しい列へ出す。新しい列の幅は**元の列の幅**（niri は取り出した窓の列の幅を保つ）。
  - どちらも、フォーカスは窓についていく（窓がフォーカス窓だったので、移った先でもフォーカス窓）。窓の高さは `Auto` に戻る（D52）。
- **D59 動作（右）**: 左右を入れ替えたもの。`n == 1` のとき、`c` が最後の列なら状態不変で `Ok`、そうでなければ右隣の列 `c + 1` の最下段へ移し、元の列を削除する。`n >= 2` のとき、元の列の**右隣**（インデックス `c + 1`）の新しい列へ、元の列の幅で出す。
- **D60 範囲外**: フォーカス窓が Floating・Fullscreen（列に属さない）のとき、フォーカスが None のとき（`NoFocusedWindow`）は、既存の列の操作（`ExpelWindowFromColumn` など）と同じ扱いにする。`Equal` では、状態不変で `Ok`（Effect なし。D45 と同じ）。
- **D61 既存の D43 との違い（記録）**: niri の `expel-window-from-column` は、フォーカス窓ではなく**最下段**の窓を、列の幅を保って出し、フォーカスは動かない。Contrast の `ExpelWindowFromColumn`（D43）は、フォーカス窓を既定の幅の新しい列へ出し、フォーカスはついていく。D43 は `tests/spec` の前提なので変えない。この違いは `docs/` の差分表に残す。
