//! タッチ入力（niri の `on_touch_*`）。タッチ画面の点を、クライアントの wl_touch に渡す。
//! スクリーンショットの範囲選択 UI が開いている間は、最初の指（スロット）をポインタの左ボタンと同じに使う
//! （niri の `pointer_down(.., slot)`）。2 本目以降の指は無視する。
//!
//! 絶対座標は、全部の出力を囲む矩形に対する位置（絶対座標のポインタと同じ）。

use smithay::{
    backend::input::TouchSlot,
    input::touch::{DownEvent, MotionEvent, UpEvent},
    utils::{Logical, Point, SERIAL_COUNTER},
};

use crate::state::Contrast;

impl Contrast {
    /// 指が触れた。`pos` は全体の論理座標、`time` はミリ秒。
    pub fn touch_down(&mut self, slot: TouchSlot, pos: Point<f64, Logical>, time: u32) {
        if self.screenshot_ui.is_open() {
            if self.screenshot_ui.touch_slot.is_none() {
                self.screenshot_ui.touch_slot = Some(slot);
                self.screenshot_ui_button(true, pos, false);
            }
            return;
        }
        let Some(touch) = self.seat.get_touch() else {
            return;
        };
        self.click_focus(pos);
        let under = self.surface_under_point(pos);
        touch.down(
            self,
            under,
            &DownEvent {
                slot,
                location: pos,
                serial: SERIAL_COUNTER.next_serial(),
                time,
            },
        );
    }

    /// 指が動いた。
    pub fn touch_motion(&mut self, slot: TouchSlot, pos: Point<f64, Logical>, time: u32) {
        if self.screenshot_ui.is_open() {
            if self.screenshot_ui.touch_slot == Some(slot) {
                self.screenshot_ui_motion(pos);
            }
            return;
        }
        let Some(touch) = self.seat.get_touch() else {
            return;
        };
        let under = self.surface_under_point(pos);
        touch.motion(
            self,
            under,
            &MotionEvent {
                slot,
                location: pos,
                time,
            },
        );
    }

    /// 指が離れた。`pos` は、離れる直前の位置（UI の選択の確定に使う）。
    pub fn touch_up(&mut self, slot: TouchSlot, pos: Point<f64, Logical>, time: u32) {
        if self.screenshot_ui.is_open() {
            if self.screenshot_ui.touch_slot == Some(slot) {
                self.screenshot_ui.touch_slot = None;
                self.screenshot_ui_button(false, pos, false);
            }
            return;
        }
        let Some(touch) = self.seat.get_touch() else {
            return;
        };
        touch.up(
            self,
            &UpEvent {
                slot,
                serial: SERIAL_COUNTER.next_serial(),
                time,
            },
        );
    }

    /// 同時の指の変化のまとまりの終わり。
    pub fn touch_frame(&mut self) {
        if let Some(touch) = self.seat.get_touch() {
            touch.frame(self);
        }
    }

    /// 指の追跡の取り消し。UI の指も放す。
    pub fn touch_cancel(&mut self) {
        self.screenshot_ui.touch_slot = None;
        if let Some(touch) = self.seat.get_touch() {
            touch.cancel(self);
        }
    }
}
