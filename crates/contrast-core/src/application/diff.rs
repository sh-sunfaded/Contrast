//! 新旧の状態の差分から Effect 列を組み立てる。
//!
//! handler は Registered と RequestClose だけを返し、ShowWorkspace・Configure・SetFocus・Damage は
//! ここで差分から求める。これで Effect 列の順序（仕様 5 節）と D15 を 1 か所で守る。
//! 同じ (旧状態, 新状態, handler の Effect) からは同じ列を返す（I8）。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::domain::geometry::Rect;
use crate::domain::id::{OutputId, WindowId, WorkspaceIndex};
use crate::domain::state::State;
use crate::domain::window::WindowState;

/// 出力の表示内容（D15 の比較対象）: 論理矩形、表示ワークスペース、窓の並び・矩形・状態、そこにあるフォーカス。
type Display = (
    Rect,
    WorkspaceIndex,
    Vec<(WindowId, Option<Rect>, WindowState)>,
    Option<WindowId>,
);

/// 仕様の順序で Effect 列を作る。
pub fn effects(old: &State, new: &State, from_handler: Vec<Effect>) -> Vec<Effect> {
    let (close, mut out): (Vec<Effect>, Vec<Effect>) = from_handler
        .into_iter()
        .partition(|e| matches!(e, Effect::RequestClose(_)));

    for o in new.outputs.values() {
        // D69: 位置ではなく、ワークスペースの id で比べる（`Dynamic` では増減で位置がずれる）。
        let before = old
            .outputs
            .get(&o.id)
            .and_then(|p| old.workspace(p.shown))
            .map(|ws| ws.id);
        let now = new.workspace(o.shown).map(|ws| ws.id);
        if before.is_some() && before == now {
            continue;
        }
        if let Some(ws) = new.workspace(o.shown) {
            out.push(Effect::ShowWorkspace {
                output: o.id,
                workspace: ws.id,
            });
        }
    }

    for w in new.windows.values() {
        let Some(rect) = w.rect else { continue };
        if !new.is_window_visible(w.id) {
            continue;
        }
        let before = old.windows.get(&w.id).map(|p| (p.rect, p.state));
        if before != Some((Some(rect), w.state)) {
            out.push(Effect::Configure {
                window: w.id,
                rect,
                state: w.state,
            });
        }
    }

    if old.focus != new.focus {
        out.push(Effect::SetFocus(new.focus));
    }

    out.extend(close);

    for id in new.outputs.keys() {
        if display(old, *id) != display(new, *id) {
            out.push(Effect::Damage(*id));
        }
    }
    out
}

/// 出力 `id` の表示内容。出力が無ければ `None`。
fn display(state: &State, id: OutputId) -> Option<Display> {
    let output = state.outputs.get(&id)?;
    let ws = state.workspace(output.shown)?;
    let windows = ws
        .windows
        .iter()
        .filter_map(|wid| state.windows.get(wid))
        .map(|w| (w.id, w.rect, w.state))
        .collect();
    let focus = state.focus.filter(|f| ws.windows.contains(f));
    Some((output.geometry, output.shown, windows, focus))
}
