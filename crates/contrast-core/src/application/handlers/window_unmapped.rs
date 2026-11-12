//! WindowUnmapped: 窓を除く。
//!
//! フォーカス窓だった場合は、同じワークスペースの順序列で隣の窓（同じ位置に来た窓、
//! 末尾だったなら新しい末尾）へフォーカスを移す。表示中でなければ None（I4）。
//! `Scrolling` で列にあった窓なら D41（列に残る窓の下→上、列ごと消えるなら右の列→左の列）に従う。
//! 上の規則は仕様に無く、プロトタイプの挙動に合わせた暫定（docs/questions.md Q8）。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::scrolling;
use crate::domain::id::ExternalKey;
use crate::domain::state::State;
use crate::error::CoreError;

/// 窓を削除する。
///
/// # Errors
/// key が未登録なら `UnknownKey`。
pub fn handle(state: &mut State, key: ExternalKey) -> Result<Vec<Effect>, CoreError> {
    let id = state.window_keys.remove_by_key(key)?;
    let was_focused = state.focus == Some(id);
    // D41: Scrolling の列にある窓なら、削除前の列の構造から次のフォーカスを決める。
    // `Equal` では列が空なので `None` になり、既存の規則に従う。
    let from_columns = if was_focused {
        scrolling::focus_after_removal(state, id)
    } else {
        None
    };
    let Some((index, position)) = state.remove_window(id) else {
        return Err(CoreError::UnknownKey);
    };
    if was_focused {
        let next = if state.output_showing(index).is_some() {
            from_columns.or_else(|| {
                state.workspace(index).and_then(|ws| {
                    position
                        .and_then(|p| ws.windows.get(p))
                        .or_else(|| ws.windows.last())
                        .copied()
                })
            })
        } else {
            None
        };
        state.set_focus(next);
    }
    Ok(Vec::new())
}
