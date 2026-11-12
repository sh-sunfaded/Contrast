//! CloseFocused: フォーカス窓に閉じる要求を出すだけ。窓の削除は `WindowUnmapped` で行う（D14）。

use alloc::vec;
use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::application::handlers::focused;
use crate::domain::state::State;
use crate::error::CoreError;

/// フォーカス窓に RequestClose を出す。状態は変えない。
///
/// # Errors
/// フォーカスが None なら `NoFocusedWindow`。
pub fn handle(state: &mut State) -> Result<Vec<Effect>, CoreError> {
    let id = focused(state)?;
    Ok(vec![Effect::RequestClose(id)])
}
