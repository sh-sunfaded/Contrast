//! libinput のデバイスへの設定の適用（タッチパッドのタップ・自然なスクロール・加速・タイピング中の無効化、マウスの既定値）。
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

/// マウスに niri の `mouse {}`（作者の設定は空）の既定を適用する（niri の `apply_libinput_settings` の mouse の部分）:
/// 有効、natural-scroll なし、加速 0、左利きなし、中ボタンの模倣なし、加速の型とスクロールの方法はデバイスの既定。
fn apply_mouse(device: &mut Device) {
    use smithay::reexports::input::{ScrollButtonLockState, ScrollMethod, SendEventsMode};
    let enabled = outcome(device.config_send_events_set_mode(SendEventsMode::ENABLED));
    let natural = outcome(device.config_scroll_set_natural_scroll_enabled(false));
    let accel = outcome(device.config_accel_set_speed(0.0));
    let left = outcome(device.config_left_handed_set(false));
    let middle = outcome(device.config_middle_emulation_set_enabled(false));
    let profile = device
        .config_accel_default_profile()
        .map_or("none", |p| outcome(device.config_accel_set_profile(p)));
    let scroll = match device.config_scroll_default_method() {
        Some(m) => {
            let r = outcome(device.config_scroll_set_method(m));
            if m == ScrollMethod::OnButtonDown {
                let _ = device.config_scroll_set_button_lock(ScrollButtonLockState::Disabled);
            }
            r
        }
        None => "none",
    };
    tracing::info!(
        device = device.name(),
        enabled,
        natural_scroll = natural,
        accel,
        left_handed = left,
        middle_emulation = middle,
        accel_profile = profile,
        scroll_method = scroll,
        "input: mouse configured (niri defaults)"
    );
}

/// トラックボールかトラックポイントか（niri・Mutter と同じく udev の `ID_INPUT_TRACKBALL`・`ID_INPUT_POINTINGSTICK`）。
fn is_trackball_or_trackpoint(device: &Device) -> bool {
    // SAFETY: `udev_device` は libinput の udev デバイスへの参照を増やして返すだけで、ここでは読み取りにしか使わない
    // （libinput のコンテキストは `device` が生きている間は有効）。
    let udev = unsafe { device.udev_device() };
    udev.is_some_and(|u| {
        u.property_value("ID_INPUT_TRACKBALL").is_some()
            || u.property_value("ID_INPUT_POINTINGSTICK").is_some()
    })
}

/// タッチパッドに `settings` を、マウスに niri の既定を適用する。どちらでもなければ何もしない。適用した項目をログに出す。
pub fn apply(device: &mut Device, settings: &InputSettings) {
    if !device.has_capability(DeviceCapability::Pointer) {
        return;
    }
    // タップのできるポインタデバイスをタッチパッドとみなす。
    if device.config_tap_finger_count() == 0 {
        if !is_trackball_or_trackpoint(device) {
            apply_mouse(device);
        }
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
