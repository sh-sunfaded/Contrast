//! OutputInsetsChanged: 出力の余白を置き換える（D31・D32。再配置は `Core::apply` が行う）。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::domain::id::ExternalKey;
use crate::domain::insets::Insets;
use crate::domain::state::State;
use crate::error::CoreError;

/// 出力の余白を `insets` に置き換える。同じ値なら何もしない。
///
/// # Errors
/// key が未登録なら `UnknownKey`、余白を引いた作業領域が作れなければ `Layout(AreaTooSmall)`。
pub fn handle(
    state: &mut State,
    key: ExternalKey,
    insets: Insets,
) -> Result<Vec<Effect>, CoreError> {
    let id = state.output_keys.id_of(key).ok_or(CoreError::UnknownKey)?;
    let output = state.outputs.get_mut(&id).ok_or(CoreError::UnknownKey)?;
    if output.insets == insets {
        return Ok(Vec::new());
    }
    output.insets = insets;
    output.work_area()?;
    Ok(Vec::new())
}
