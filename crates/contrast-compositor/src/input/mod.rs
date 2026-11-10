//! 入力: winit からのキー・ポインタイベントを処理し、キーバインドを意味操作に変える。

pub mod bindings;
pub mod debug;
pub mod devices;
pub mod wheel;

use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, GestureBeginEvent,
        GestureEndEvent, GesturePinchUpdateEvent, GestureSwipeUpdateEvent, InputBackend,
        InputEvent, KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
        PointerMotionEvent, TouchEvent,
    },
    desktop::WindowSurfaceType,
    input::{
        keyboard::{FilterResult, ModifiersState, xkb},
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, SERIAL_COUNTER},
    wayland::input_method::InputMethodSeat,
};

use contrast_policy::{Action, Mods};

use crate::state::Contrast;

/// smithay の修飾キー状態をポリシーの `Mods` に変える。
fn to_mods(m: &ModifiersState) -> Mods {
    Mods {
        alt: m.alt,
        shift: m.shift,
        ctrl: m.ctrl,
        logo: m.logo,
    }
}

impl Contrast {
    /// 全部の出力を囲む矩形（全体の論理座標）。出力が無ければ `None`。
    pub fn outputs_bounds(&self) -> Option<smithay::utils::Rectangle<i32, Logical>> {
        self.space
            .outputs()
            .filter_map(|o| self.space.output_geometry(o))
            .reduce(|a, b| a.merge(b))
    }

    /// 座標の下にある surface と、その surface の原点の位置（ほかのモジュールから使う口）。
    pub(crate) fn surface_under_point(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        self.surface_under(pos)
    }

    /// 座標の下にある surface と、その surface の原点の位置。
    fn surface_under(&self, pos: Point<f64, Logical>) -> Option<(WlSurface, Point<f64, Logical>)> {
        use smithay::{desktop::layer_map_for_output, wayland::shell::wlr_layer::Layer};
        let output = self.output_at(pos.x, pos.y);
        // レイヤー面の位置は出力の中の座標なので、出力の位置を引いて探し、足して返す。
        let origin = output
            .as_ref()
            .and_then(|o| self.space.output_geometry(o))
            .map(|g| g.loc)
            .unwrap_or_default();
        let layer_hit = |layers: [Layer; 2]| {
            let output = output.as_ref()?;
            let map = layer_map_for_output(output);
            let local = pos - origin.to_f64();
            layers.into_iter().find_map(|l| {
                let layer = map.layer_under(l, local)?;
                let geo = map.layer_geometry(layer)?;
                layer
                    .surface_under(local - geo.loc.to_f64(), WindowSurfaceType::ALL)
                    .map(|(s, p)| (s, (p + geo.loc + origin).to_f64()))
            })
        };
        if let Some(hit) = layer_hit([Layer::Overlay, Layer::Top]) {
            return Some(hit);
        }
        // 窓は、その出力に表示中のものだけ（隣の出力にはみ出したスクロール列の窓は当たらない）。
        let on_output: Vec<smithay::desktop::Window> = output
            .as_ref()
            .map(|o| {
                let view = self.core.view();
                self.windows_on(o)
                    .iter()
                    .filter_map(|w| view.window_key(w.id))
                    .filter_map(|k| self.shell.windows.get(&k).cloned())
                    .collect()
            })
            .unwrap_or_default();
        self.space
            .elements()
            .rev()
            .filter(|w| on_output.contains(w))
            .find_map(|window| {
                let location = self.space.element_location(window)? - window.geometry().loc;
                window
                    .surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)
                    .map(|(s, p)| (s, (p + location).to_f64()))
            })
            .or_else(|| layer_hit([Layer::Bottom, Layer::Background]))
    }

    /// 座標の下の窓へのフォーカスを core に要求する。窓の外（空き領域）なら何もしない。
    pub fn click_focus(&mut self, pos: Point<f64, Logical>) {
        if self.click_layer(pos) {
            return;
        }
        self.focus_at(pos.x, pos.y);
    }

    /// ポインタが動いたとき、`Input.focusFollowsMouse` が有効なら、下の窓にフォーカスを移す。
    /// レイヤー面がキーボードを持っている間と、ポインタをつかんでいる間（ドラッグなど）は動かさない。
    fn follow_focus(&mut self, pos: Point<f64, Logical>) {
        if !self.policy.input_settings().focus_follows_mouse || self.layer_focus.is_some() {
            return;
        }
        if self.seat.get_pointer().is_none_or(|p| p.is_grabbed()) {
            return;
        }
        self.hover_focus(pos.x, pos.y);
    }

    /// winit からの入力イベントを 1 つ処理する。
    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>)
    where
        I::GestureSwipeUpdateEvent: 'static,
    {
        self.last_input = std::time::Instant::now();
        self.monitors.wake();
        // ホットキーのオーバーレイは、キーを押す・ボタンを押す・ホイール・ジェスチャー・タッチで閉じる（niri の
        // `should_hide_hotkey_overlay`）。イベントは処理し、最後に閉じる（オーバーレイを開くキーを押しても、開いていれば閉じる）。
        let hide_overlay = self.hotkey_overlay.open && hides_hotkey_overlay(&event);
        self.dispatch_input_event(event);
        if hide_overlay {
            self.hotkey_overlay.open = false;
        }
    }

    /// 入力イベントを 1 つ処理する（`process_input_event` の本体）。
    fn dispatch_input_event<I: InputBackend>(&mut self, event: InputEvent<I>)
    where
        I::GestureSwipeUpdateEvent: 'static,
    {
        match event {
            InputEvent::Keyboard { event, .. } => {
                self.on_key(event.key_code(), event.state(), Event::time_msec(&event))
            }
            InputEvent::PointerMotionAbsolute { event, .. } => {
                // 絶対座標（winit のホストの窓）は、全部の出力を囲む矩形に対する位置（winit で 1 窓を複数の出力に分けるとき）。
                let Some(bounds) = self.outputs_bounds() else {
                    return;
                };
                let pos = event.position_transformed(bounds.size) + bounds.loc.to_f64();
                let serial = SERIAL_COUNTER.next_serial();
                let pointer = self.seat.get_pointer().expect("seat has pointer");
                // スクリーンショットの UI が開いている間は、ポインタをクライアントに渡さず、選択に使う。
                let under = if self.screenshot_ui.is_open() {
                    self.screenshot_ui_motion(pos);
                    None
                } else {
                    self.surface_under(pos)
                };
                pointer.motion(
                    self,
                    under,
                    &MotionEvent {
                        location: pos,
                        serial,
                        time: event.time_msec(),
                    },
                );
                pointer.frame(self);
                self.follow_focus(pos);
                self.hot_corner_motion(pos);
                self.dnd_motion(pos);
            }
            InputEvent::PointerMotion { event, .. } => {
                let Some(pointer) = self.seat.get_pointer() else {
                    return;
                };
                let current = pointer.current_location();
                let moved = current + event.delta();
                // 出力の外へ出るなら、今いる出力（なければ最初の出力）の中に留める（niri と同じ）。
                let pos = if self.output_at(moved.x, moved.y).is_some() {
                    moved
                } else {
                    let Some(output) = self
                        .output_at(current.x, current.y)
                        .or_else(|| self.space.outputs().next().cloned())
                    else {
                        return;
                    };
                    let Some(output_geo) = self.space.output_geometry(&output) else {
                        return;
                    };
                    clamp_to(moved, output_geo.to_f64())
                };
                // スクリーンショットの UI が開いている間は、ポインタをクライアントに渡さず、選択に使う。
                let under = if self.screenshot_ui.is_open() {
                    self.screenshot_ui_motion(pos);
                    None
                } else {
                    self.surface_under(pos)
                };
                pointer.motion(
                    self,
                    under,
                    &MotionEvent {
                        location: pos,
                        serial: SERIAL_COUNTER.next_serial(),
                        time: event.time_msec(),
                    },
                );
                pointer.frame(self);
                self.follow_focus(pos);
                self.hot_corner_motion(pos);
                self.dnd_motion(pos);
            }
            InputEvent::PointerButton { event, .. } => {
                let pointer = self.seat.get_pointer().expect("seat has pointer");
                let serial = SERIAL_COUNTER.next_serial();
                let state = event.state();
                // オーバービュー: 左クリックで窓・ワークスペースを選んで閉じる。クライアントには渡さない。
                if self.overview.open && !self.screenshot_ui.is_open() {
                    const BTN_LEFT: u32 = 0x110;
                    if event.button_code() == BTN_LEFT && state == ButtonState::Pressed {
                        self.overview_click(pointer.current_location());
                    }
                    return;
                }
                // スクリーンショットの UI: 左ボタンで選択を描く（Mod を押したままなら選択ごと動かす）。クライアントには渡さない。
                if self.screenshot_ui.is_open() {
                    const BTN_LEFT: u32 = 0x110;
                    if event.button_code() == BTN_LEFT {
                        let mods = self
                            .seat
                            .get_keyboard()
                            .map(|k| k.modifier_state())
                            .unwrap_or_default();
                        let mod_down = match self.policy.mod_key() {
                            contrast_policy::ModKey::Super => mods.logo,
                            contrast_policy::ModKey::Alt => mods.alt,
                        };
                        self.screenshot_ui_button(
                            state == ButtonState::Pressed,
                            pointer.current_location(),
                            mod_down,
                        );
                    }
                    return;
                }
                if state == ButtonState::Pressed && !pointer.is_grabbed() {
                    self.click_focus(pointer.current_location());
                }
                pointer.button(
                    self,
                    &ButtonEvent {
                        button: event.button_code(),
                        state,
                        serial,
                        time: event.time_msec(),
                    },
                );
                pointer.frame(self);
            }
            InputEvent::TouchDown { event } => {
                let Some(bounds) = self.outputs_bounds() else {
                    return;
                };
                let pos = event.position_transformed(bounds.size) + bounds.loc.to_f64();
                self.touch_last = pos;
                self.touch_down(event.slot(), pos, Event::time_msec(&event));
            }
            InputEvent::TouchMotion { event } => {
                let Some(bounds) = self.outputs_bounds() else {
                    return;
                };
                let pos = event.position_transformed(bounds.size) + bounds.loc.to_f64();
                self.touch_last = pos;
                self.touch_motion(event.slot(), pos, Event::time_msec(&event));
            }
            InputEvent::TouchUp { event } => {
                let pos = self.touch_last;
                self.touch_up(event.slot(), pos, Event::time_msec(&event));
            }
            InputEvent::TouchFrame { .. } => self.touch_frame(),
            InputEvent::TouchCancel { .. } => self.touch_cancel(),
            InputEvent::PointerAxis { event, .. } => {
                let source = event.source();
                // ホイールのバインド（`Mod+WheelScrollDown` など）に当たれば、クライアントには渡さない。
                if source == AxisSource::Wheel
                    && self.on_wheel(
                        event.amount_v120(Axis::Horizontal),
                        event.amount_v120(Axis::Vertical),
                    )
                {
                    return;
                }
                let mut frame = AxisFrame::new(event.time_msec()).source(source);
                for axis in [Axis::Horizontal, Axis::Vertical] {
                    let discrete = event.amount_v120(axis);
                    let amount = event
                        .amount(axis)
                        .unwrap_or_else(|| discrete.unwrap_or(0.0) * 15.0 / 120.0);
                    if amount != 0.0 {
                        frame = frame.value(axis, amount);
                        if let Some(d) = discrete {
                            frame = frame.v120(axis, d as i32);
                        }
                    }
                    if source == AxisSource::Finger && event.amount(axis) == Some(0.0) {
                        frame = frame.stop(axis);
                    }
                }
                let pointer = self.seat.get_pointer().expect("seat has pointer");
                pointer.axis(self, frame);
                pointer.frame(self);
            }
            InputEvent::GestureSwipeBegin { event, .. } => {
                let fingers = GestureBeginEvent::fingers(&event);
                // 3・4 本指はコンポジタが使う。それ以外はクライアントに渡す（niri と同じ）。
                if self.swipe_begin(i32::try_from(fingers).unwrap_or(0)) {
                    self.swipe_forwarded = false;
                } else if let Some(pointer) = self.seat.get_pointer() {
                    self.swipe_forwarded = true;
                    pointer.gesture_swipe_begin(
                        self,
                        &smithay::input::pointer::GestureSwipeBeginEvent {
                            serial: SERIAL_COUNTER.next_serial(),
                            time: Event::time_msec(&event),
                            fingers,
                        },
                    );
                }
            }
            InputEvent::GestureSwipeUpdate { event, .. } if self.swipe_forwarded => {
                if let Some(pointer) = self.seat.get_pointer() {
                    pointer.gesture_swipe_update(
                        self,
                        &smithay::input::pointer::GestureSwipeUpdateEvent {
                            time: Event::time_msec(&event),
                            delta: (event.delta_x(), event.delta_y()).into(),
                        },
                    );
                }
            }
            InputEvent::GestureSwipeEnd { event, .. } if self.swipe_forwarded => {
                self.swipe_forwarded = false;
                if let Some(pointer) = self.seat.get_pointer() {
                    pointer.gesture_swipe_end(
                        self,
                        &smithay::input::pointer::GestureSwipeEndEvent {
                            serial: SERIAL_COUNTER.next_serial(),
                            time: Event::time_msec(&event),
                            cancelled: GestureEndEvent::cancelled(&event),
                        },
                    );
                }
            }
            InputEvent::GesturePinchBegin { event, .. } => {
                if let Some(pointer) = self.seat.get_pointer() {
                    pointer.gesture_pinch_begin(
                        self,
                        &smithay::input::pointer::GesturePinchBeginEvent {
                            serial: SERIAL_COUNTER.next_serial(),
                            time: Event::time_msec(&event),
                            fingers: GestureBeginEvent::fingers(&event),
                        },
                    );
                }
            }
            InputEvent::GesturePinchUpdate { event, .. } => {
                if let Some(pointer) = self.seat.get_pointer() {
                    pointer.gesture_pinch_update(
                        self,
                        &smithay::input::pointer::GesturePinchUpdateEvent {
                            time: Event::time_msec(&event),
                            delta: (event.delta_x(), event.delta_y()).into(),
                            scale: event.scale(),
                            rotation: event.rotation(),
                        },
                    );
                }
            }
            InputEvent::GesturePinchEnd { event, .. } => {
                if let Some(pointer) = self.seat.get_pointer() {
                    pointer.gesture_pinch_end(
                        self,
                        &smithay::input::pointer::GesturePinchEndEvent {
                            serial: SERIAL_COUNTER.next_serial(),
                            time: Event::time_msec(&event),
                            cancelled: GestureEndEvent::cancelled(&event),
                        },
                    );
                }
            }
            InputEvent::GestureHoldBegin { event, .. } => {
                if let Some(pointer) = self.seat.get_pointer() {
                    pointer.gesture_hold_begin(
                        self,
                        &smithay::input::pointer::GestureHoldBeginEvent {
                            serial: SERIAL_COUNTER.next_serial(),
                            time: Event::time_msec(&event),
                            fingers: GestureBeginEvent::fingers(&event),
                        },
                    );
                }
            }
            InputEvent::GestureHoldEnd { event, .. } => {
                if let Some(pointer) = self.seat.get_pointer() {
                    pointer.gesture_hold_end(
                        self,
                        &smithay::input::pointer::GestureHoldEndEvent {
                            serial: SERIAL_COUNTER.next_serial(),
                            time: Event::time_msec(&event),
                            cancelled: GestureEndEvent::cancelled(&event),
                        },
                    );
                }
            }
            InputEvent::GestureSwipeUpdate { event, .. } => {
                let (dx, dy, natural) = swipe_delta(&event);
                let (sx, sy) = if natural { (-dx, -dy) } else { (dx, dy) };
                let t = std::time::Duration::from_micros(Event::time(&event));
                self.swipe_update(sx, sy, dy, t);
            }
            InputEvent::GestureSwipeEnd { event, .. } => {
                self.swipe_end(std::time::Duration::from_micros(Event::time(&event)));
            }
            _ => {}
        }
    }

    /// 押されているキーをすべて離す（VT 切り替えでデバイスが外れると、離したイベントが来ないため）。
    /// 握りつぶしたキー（バインド）は、クライアントに渡さない。
    pub fn release_all_keys(&mut self) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let suppressed = std::mem::take(&mut self.suppressed_keys);
        let time = u32::try_from(self.start_time.elapsed().as_millis()).unwrap_or(0);
        for keycode in keyboard.pressed_keys() {
            let serial = SERIAL_COUNTER.next_serial();
            keyboard.input::<(), _>(
                self,
                keycode,
                KeyState::Released,
                serial,
                time,
                |_, _, _| {
                    if suppressed.contains(&keycode) {
                        FilterResult::Intercept(())
                    } else {
                        FilterResult::Forward
                    }
                },
            );
        }
    }

    /// ホイールの軸イベント。押している修飾キーに完全に一致するホイールのバインドがあれば、目盛りごとに実行して `true`
    /// （クライアントには渡さない）。無ければ足し込みを捨てて `false`（niri と同じ）。
    fn on_wheel(&mut self, horizontal: Option<f64>, vertical: Option<f64>) -> bool {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return false;
        };
        let mods = to_mods(&keyboard.modifier_state());
        self.wheel_with_mods(mods, horizontal, vertical)
    }

    /// `on_wheel` の本体。修飾キー `mods` を押しているものとして扱う（デバッグ入力からも呼ぶ）。
    pub(crate) fn wheel_with_mods(
        &mut self,
        mods: Mods,
        horizontal: Option<f64>,
        vertical: Option<f64>,
    ) -> bool {
        const KEYS: [&str; 4] = [
            "WheelScrollUp",
            "WheelScrollDown",
            "WheelScrollLeft",
            "WheelScrollRight",
        ];
        if !KEYS
            .iter()
            .any(|key| self.policy.resolve_key(mods, key).is_some())
        {
            self.wheel_horizontal.reset();
            self.wheel_vertical.reset();
            return false;
        }
        // 横を先に、縦をあとに（niri と同じ順）。正は右／下、負は左／上。
        let h = self.wheel_horizontal.accumulate(horizontal.unwrap_or(0.0));
        let v = self.wheel_vertical.accumulate(vertical.unwrap_or(0.0));
        for (ticks, positive, negative) in [
            (h, "WheelScrollRight", "WheelScrollLeft"),
            (v, "WheelScrollDown", "WheelScrollUp"),
        ] {
            let (forward, backward) = wheel::directions(ticks);
            for _ in 0..forward {
                self.fire_wheel_bind(mods, positive);
            }
            for _ in 0..backward {
                self.fire_wheel_bind(mods, negative);
            }
        }
        true
    }

    /// ホイールのバインド 1 回分。`cooldown` の間は実行しない。
    fn fire_wheel_bind(&mut self, mods: Mods, key: &str) {
        let Some(action) = self.policy.resolve_key(mods, key) else {
            return;
        };
        let cooldown = self
            .policy
            .key_cooldown_ms(mods, key)
            .map(|ms| std::time::Duration::from_millis(u64::from(ms)));
        if !self
            .bind_cooldowns
            .allow(mods, key, cooldown, std::time::Instant::now())
        {
            return;
        }
        tracing::info!(key, ?action, "wheel binding");
        self.run_action(action);
    }

    /// キー 1 回分。キーバインドに当たれば握りつぶして実行し、離したときも素通ししない。
    fn on_key(&mut self, keycode: xkb::Keycode, key_state: KeyState, time: u32) {
        let serial = SERIAL_COUNTER.next_serial();
        let keyboard = self.seat.get_keyboard().expect("seat has keyboard");
        // 入力メソッドが消えたのにグラブだけ残ると、キーボードが止まる。Smithay が外すはずだが、念のためここでも外す。
        // （コンポジタのバインドと VT 切り替えは、次の `input` のフィルタで、グラブより先に処理される。）
        if keyboard.is_grabbed() && !self.seat.input_method().keyboard_grabbed() {
            tracing::warn!("ime: a keyboard grab is left without an input method, releasing it");
            keyboard.unset_grab(self);
        }
        let action = keyboard.input::<Option<Action>, _>(
            self,
            keycode,
            key_state,
            serial,
            time,
            |state, mods, handle| {
                if key_state == KeyState::Released {
                    // スクリーンショットの UI: Space を離すと、選択の移動を終える。
                    if state.screenshot_ui.is_open()
                        && handle
                            .raw_latin_sym_or_raw_current_sym()
                            .map(xkb::keysym_get_name)
                            .as_deref()
                            == Some("space")
                    {
                        state.screenshot_ui_space_released();
                    }
                    if let Some(i) = state.suppressed_keys.iter().position(|k| *k == keycode) {
                        state.suppressed_keys.remove(i);
                        return FilterResult::Intercept(None);
                    }
                    return FilterResult::Forward;
                }
                let Some(sym) = handle.raw_latin_sym_or_raw_current_sym() else {
                    return FilterResult::Forward;
                };
                let name = xkb::keysym_get_name(sym);
                if let Some(vt) =
                    vt_number(&xkb::keysym_get_name(handle.modified_sym()), &name, mods)
                {
                    tracing::info!(vt, "keybinding: switch vt");
                    state.pending_vt = Some(vt);
                    state.suppressed_keys.push(keycode);
                    return FilterResult::Intercept(None);
                }
                // オーバービューの決まったキー（Escape・Enter・矢印。修飾キーなし）。
                if state.overview.open
                    && state.overview_key(&name, mods.ctrl || mods.alt || mods.shift || mods.logo)
                {
                    state.suppressed_keys.push(keycode);
                    return FilterResult::Intercept(None);
                }
                // スクリーンショットの UI が開いている間は、キーをクライアントに渡さない。
                if state.screenshot_ui.is_open() {
                    state.suppressed_keys.push(keycode);
                    if state.screenshot_ui_key(&name, mods.ctrl, mods.alt, mods.shift) {
                        return FilterResult::Intercept(None);
                    }
                    return FilterResult::Intercept(state.policy.resolve_key(to_mods(mods), &name));
                }
                match state.policy.resolve_key(to_mods(mods), &name) {
                    Some(action) => {
                        tracing::info!(key = %name, ?action, "keybinding");
                        state.suppressed_keys.push(keycode);
                        FilterResult::Intercept(Some(action))
                    }
                    None => FilterResult::Forward,
                }
            },
        );
        if let Some(vt) = self.pending_vt.take() {
            match self.vt_switch.as_mut() {
                Some(switch) => switch(vt),
                None => tracing::info!(vt, "vt switch ignored: not on a session backend"),
            }
        }
        if let Some(Some(action)) = action {
            self.run_action(action);
        }
    }
}

/// スワイプの移動 `(dx, dy, natural-scroll か)`。libinput なら加速前の値と、デバイスの natural-scroll の設定（niri と同じ）。
fn swipe_delta<B: InputBackend, E: GestureSwipeUpdateEvent<B> + 'static>(
    event: &E,
) -> (f64, f64, bool) {
    use std::any::Any;
    let any = event as &dyn Any;
    if let Some(e) =
        any.downcast_ref::<smithay::reexports::input::event::gesture::GestureSwipeUpdateEvent>()
    {
        use smithay::reexports::input::event::{EventTrait, gesture::GestureEventCoordinates};
        let natural = EventTrait::device(e).config_scroll_natural_scroll_enabled();
        return (e.dx_unaccelerated(), e.dy_unaccelerated(), natural);
    }
    (event.delta_x(), event.delta_y(), false)
}

/// ホットキーのオーバーレイを閉じる入力か（niri の `should_hide_hotkey_overlay`）。
fn hides_hotkey_overlay<I: InputBackend>(event: &InputEvent<I>) -> bool {
    match event {
        InputEvent::Keyboard { event } => event.state() == KeyState::Pressed,
        InputEvent::PointerButton { event } => event.state() == ButtonState::Pressed,
        InputEvent::PointerAxis { .. }
        | InputEvent::GestureSwipeBegin { .. }
        | InputEvent::GesturePinchBegin { .. }
        | InputEvent::TouchDown { .. }
        | InputEvent::TouchMotion { .. }
        | InputEvent::TabletToolTip { .. }
        | InputEvent::TabletToolButton { .. } => true,
        _ => false,
    }
}

/// 座標を矩形の中に収める（右端・下端は含まない最後の画素まで）。
fn clamp_to(
    pos: Point<f64, Logical>,
    rect: smithay::utils::Rectangle<f64, Logical>,
) -> Point<f64, Logical> {
    let max_x = (rect.loc.x + rect.size.w - 1.0).max(rect.loc.x);
    let max_y = (rect.loc.y + rect.size.h - 1.0).max(rect.loc.y);
    (
        pos.x.clamp(rect.loc.x, max_x),
        pos.y.clamp(rect.loc.y, max_y),
    )
        .into()
}

/// VT 切り替えのキーなら、行き先の番号（1〜12）。`XF86Switch_VT_n`、または Ctrl+Alt を押した `F<n>`。
fn vt_number(modified: &str, raw: &str, mods: &ModifiersState) -> Option<i32> {
    let parse = |s: &str, prefix: &str| -> Option<i32> {
        let n = s.strip_prefix(prefix)?.parse::<i32>().ok()?;
        (1..=12).contains(&n).then_some(n)
    };
    parse(modified, "XF86Switch_VT_")
        .or_else(|| parse(raw, "XF86Switch_VT_"))
        .or_else(|| {
            if mods.ctrl && mods.alt {
                parse(raw, "F")
            } else {
                None
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl_alt() -> ModifiersState {
        ModifiersState {
            ctrl: true,
            alt: true,
            ..Default::default()
        }
    }

    #[test]
    fn vt_keys_are_recognised() {
        assert_eq!(vt_number("XF86Switch_VT_3", "F3", &ctrl_alt()), Some(3));
        assert_eq!(vt_number("F12", "F12", &ctrl_alt()), Some(12));
        assert_eq!(vt_number("F3", "F3", &ModifiersState::default()), None);
        assert_eq!(vt_number("XF86Switch_VT_13", "F13", &ctrl_alt()), None);
    }

    #[test]
    fn pointer_is_clamped_to_the_output() {
        let rect =
            smithay::utils::Rectangle::<f64, Logical>::new((0.0, 0.0).into(), (100.0, 50.0).into());
        assert_eq!(clamp_to((-5.0, 70.0).into(), rect), (0.0, 49.0).into());
        assert_eq!(clamp_to((10.0, 20.0).into(), rect), (10.0, 20.0).into());
    }
}
