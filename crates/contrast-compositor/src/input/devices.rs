//! libinput のデバイスへの設定の適用（タッチパッドのタップ・自然なスクロール・加速・タイピング中の無効化）。
//!
//! デバイスが追加されたとき（起動時の既存デバイスを含む）と、設定のリロードのときに適用する。
//! デバイスが使えない項目はログに出して続ける。

use smithay::reexports::input::{Device, DeviceCapability, DeviceConfigResult};

use contrast_policy::InputSettings;

use crate::state::Contrast;

/// 1 項目の結果をログ用の文字列にする。
fn outcome(result: DeviceConfigResult) -> &'static str {
    match result {
        Ok(()) => "ok",
        Err(_) => "rejected",
    }
}

/// タッチパッドの dwtp（トラックポイント操作中の無効化）。niri の既定に合わせて off に固定する。
const DISABLE_WHILE_TRACKPOINTING: bool = false;

/// タッチパッドに `settings` を適用する。タッチパッドでなければ何もしない。適用した項目をログに出す。
pub fn apply(device: &mut Device, settings: &InputSettings) {
    // タップのできるポインタデバイスをタッチパッドとみなす（マウスには触らない）。
    if !device.has_capability(DeviceCapability::Pointer) || device.config_tap_finger_count() == 0 {
        return;
    }
    let tap = outcome(device.config_tap_set_enabled(settings.tap_to_click));
    let scroll = if device.config_scroll_has_natural_scroll() {
        outcome(device.config_scroll_set_natural_scroll_enabled(settings.natural_scroll))
    } else {
        "unsupported"
    };
    let accel = if device.config_accel_is_available() {
        outcome(device.config_accel_set_speed(settings.accel_speed))
    } else {
        "unsupported"
    };
    let dwt = if device.config_dwt_is_available() {
        outcome(device.config_dwt_set_enabled(settings.disable_while_typing))
    } else {
        "unsupported"
    };
    // niri はタッチパッドの dwtp（トラックポイント操作中の無効化）を毎回 off に設定する（`dwtp` の設定の既定が off。
    // 作者の config.kdl は書いていない）。libinput の既定に任せない。
    let dwtp = if device.config_dwtp_is_available() {
        outcome(device.config_dwtp_set_enabled(DISABLE_WHILE_TRACKPOINTING))
    } else {
        "unsupported"
    };
    tracing::info!(
        device = device.name(),
        tap,
        natural_scroll = scroll,
        accel,
        disable_while_typing = dwt,
        disable_while_trackpointing = dwtp,
        "input: touchpad configured"
    );
}

impl Contrast {
    /// libinput のデバイスが追加された: 設定を適用して、リロードで再適用できるよう覚える。
    pub fn input_device_added(&mut self, mut device: Device) {
        tracing::info!(device = device.name(), "input: device added");
        apply(&mut device, &self.policy.input_settings());
        self.input_devices.push(device);
    }

    /// libinput のデバイスが外れた。
    pub fn input_device_removed(&mut self, device: &Device) {
        tracing::info!(device = device.name(), "input: device removed");
        self.input_devices.retain(|d| d != device);
    }

    /// いまの設定を、覚えているデバイスすべてに適用し直す（リロードのとき）。
    pub fn reapply_input_settings(&mut self) {
        let settings = self.policy.input_settings();
        for device in &mut self.input_devices {
            apply(device, &settings);
        }
    }
}
