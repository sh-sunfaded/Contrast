//! WindowMetaChanged: 窓の性質を更新する。配置状態は変えない（再配置の判断は Policy の範囲外）。

use alloc::vec::Vec;

use crate::application::effect::Effect;
use crate::domain::id::ExternalKey;
use crate::domain::state::State;
use crate::domain::window::WindowMeta;
use crate::error::CoreError;

/// 窓の性質を変える。
///
/// # Errors
/// key が未登録なら `UnknownKey`。
pub fn handle(
    state: &mut State,
    key: ExternalKey,
    meta: WindowMeta,
) -> Result<Vec<Effect>, CoreError> {
    let id = state.window_keys.id_of(key).ok_or(CoreError::UnknownKey)?;
    let window = state.windows.get_mut(&id).ok_or(CoreError::UnknownKey)?;
    window.meta = meta;
    Ok(Vec::new())
}
