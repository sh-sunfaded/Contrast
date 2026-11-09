//! スクリーンショットの範囲を選ぶ UI（niri の `screenshot` と `src/ui/screenshot_ui.rs`）の、描画を含まない状態。
//!
//! 座標は出力ごとの物理 px（出力の左上が原点）。選択は 1 つの出力の中だけ（niri と同じ）。
//! 選択は 2 つの角 `a`・`b`（どちらも含む画素）で持ち、矩形は `rect_from_corners`。
//! 開くと、前回の選択（その出力がまだあれば）か、既定の出力の中央の 1/2 の大きさ。

use std::collections::BTreeMap;

/// 方向キーで選択を動かす量（論理 px。niri の `DIRECTIONAL_MOVE_PX`）。
pub const MOVE_PX: f64 = 50.0;
/// 開くアニメーションの長さ（niri の既定の `screenshot-ui-open`: 200ms・ease-out-quad）。
pub const OPEN_DURATION: std::time::Duration = std::time::Duration::from_millis(200);
/// 選択の枠の太さ（論理 px）。
pub const SELECTION_BORDER: f64 = 2.0;

/// 出力の大きさ（物理 px）と拡大率。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutputInfo {
    /// 幅・高さ（物理 px）。
    pub size: (i32, i32),
    /// 拡大率。
    pub scale: f64,
}

/// 矩形 `(x, y, 幅, 高さ)`（物理 px）。
pub type Px = (i32, i32, i32, i32);

/// 2 つの角（どちらも含む画素）から矩形。
pub fn rect_from_corners(a: (i32, i32), b: (i32, i32)) -> Px {
    let (x1, x2) = (a.0.min(b.0), a.0.max(b.0));
    let (y1, y2) = (a.1.min(b.1), a.1.max(b.1));
    (x1, y1, x2 - x1 + 1, y2 - y1 + 1)
}

/// 論理 px を物理 px にする（四捨五入）。
fn physical(scale: f64, logical: f64) -> i32 {
    (logical * scale).round() as i32
}

/// 大きさの変え方（niri の `SizeChange`）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SizeChange {
    /// 出力の大きさに対する割合（パーセント）にする。
    SetProportion(f64),
    /// 割合をパーセントだけ増減する。
    AdjustProportion(f64),
    /// 論理 px にする。
    SetFixed(i32),
}

/// マウスのボタンの状態。
#[derive(Debug, Clone, PartialEq)]
enum Button {
    Up,
    Down {
        /// 最後のポインタの位置（出力、点）。
        last: (String, (i32, i32)),
        /// 選択ごと動かしているときの、角 `a` からのポインタのずれ（Space か、Mod を押してのクリック）。
        moving: Option<(i32, i32)>,
        /// Space で動かし始めたか（Space を離すと終える）。
        by_space: bool,
    },
}

/// キーの操作（UI が自分で扱うもの。niri の `screenshot_ui::action`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiKey {
    /// Escape: 閉じる。
    Cancel,
    /// Space・Enter: 保存（とクリップボード）。
    Confirm,
    /// Ctrl+C: クリップボードだけ。
    ConfirmToClipboard,
    /// P: ポインタの表示を切り替える。
    TogglePointer,
}

/// キー（キーシンボル名と修飾キー）を UI の操作にする。niri と同じく、Alt か Shift があれば Escape 以外は何もしない。
pub fn key_action(name: &str, ctrl: bool, alt: bool, shift: bool) -> Option<UiKey> {
    if name == "Escape" {
        return Some(UiKey::Cancel);
    }
    if alt || shift {
        return None;
    }
    match (ctrl, name) {
        (false, "space" | "Return") => Some(UiKey::Confirm),
        (true, "c") => Some(UiKey::ConfirmToClipboard),
        (false, "p") => Some(UiKey::TogglePointer),
        _ => None,
    }
}

/// 開いている UI の状態。
#[derive(Debug, Clone, PartialEq)]
pub struct Ui {
    /// 選択の出力と、2 つの角。
    pub output: String,
    a: (i32, i32),
    b: (i32, i32),
    /// 出力ごとの大きさ。
    pub outputs: BTreeMap<String, OutputInfo>,
    button: Button,
    /// ポインタを入れるか。
    pub show_pointer: bool,
}

impl Ui {
    /// 開く。`last` は前回の選択（出力の名前と矩形）。その出力が無ければ、`default_output` の中央の 1/2。
    pub fn open(
        outputs: BTreeMap<String, OutputInfo>,
        default_output: &str,
        last: Option<(String, Px)>,
        show_pointer: bool,
    ) -> Option<Self> {
        let (output, rect) = match last.filter(|(o, _)| outputs.contains_key(o)) {
            Some(l) => l,
            None => {
                let info = outputs.get(default_output)?;
                (default_output.to_string(), default_rect(info.size))
            }
        };
        let mut ui = Self {
            output,
            a: (rect.0, rect.1),
            b: (rect.0 + rect.2 - 1, rect.1 + rect.3 - 1),
            outputs,
            button: Button::Up,
            show_pointer,
        };
        ui.validate();
        Some(ui)
    }

    /// 選択の矩形（選択の出力の物理 px）。
    pub fn rect(&self) -> Px {
        rect_from_corners(self.a, self.b)
    }

    /// 選択の出力の情報。
    fn info(&self) -> Option<OutputInfo> {
        self.outputs.get(&self.output).copied()
    }

    /// 選択が出力に収まっていなければ、既定（中央の 1/2）に戻す（niri の `update_buffers`）。
    fn validate(&mut self) {
        let Some(info) = self.info() else { return };
        let (x, y, w, h) = self.rect();
        let inside = x >= 0 && y >= 0 && x + w <= info.size.0 && y + h <= info.size.1;
        if !inside {
            let r = default_rect(info.size);
            self.a = (r.0, r.1);
            self.b = (r.0 + r.2 - 1, r.1 + r.3 - 1);
        }
    }

    /// 選択を `(dx, dy)` 方向へ `MOVE_PX` だけ動かす（出力の端で止まる）。
    pub fn move_by(&mut self, dx: i32, dy: i32) {
        let Some(info) = self.info() else { return };
        let step = physical(info.scale, MOVE_PX);
        let (x, y, w, h) = self.rect();
        let mx = match dx.signum() {
            -1 => -step.min(x),
            1 => step.min(info.size.0 - (x + w)),
            _ => 0,
        };
        let my = match dy.signum() {
            -1 => -step.min(y),
            1 => step.min(info.size.1 - (y + h)),
            _ => 0,
        };
        self.a = (self.a.0 + mx, self.a.1 + my);
        self.b = (self.b.0 + mx, self.b.1 + my);
    }

    /// 幅を変える（左端を保つ。niri の `set_width`）。
    pub fn set_width(&mut self, change: SizeChange) {
        let Some(info) = self.info() else { return };
        let (x, _, w, _) = self.rect();
        let size = new_size(change, w, info.size.0, info.scale).clamp(1, info.size.0 - x);
        self.a.0 = x;
        self.b.0 = x + size - 1;
    }

    /// 高さを変える（上端を保つ）。
    pub fn set_height(&mut self, change: SizeChange) {
        let Some(info) = self.info() else { return };
        let (_, y, _, h) = self.rect();
        let size = new_size(change, h, info.size.1, info.scale).clamp(1, info.size.1 - y);
        self.a.1 = y;
        self.b.1 = y + size - 1;
    }

    /// 選択を別の出力へ移す（相対位置を保ち、論理の大きさを保つ。収まるように縮める。niri の `move_to_output`）。
    pub fn move_to_output(&mut self, name: &str) {
        if name == self.output {
            return;
        }
        let (Some(from), Some(to)) = (self.info(), self.outputs.get(name).copied()) else {
            return;
        };
        let (x, y, w, h) = self.rect();
        let rel_x = f64::from(x) / f64::from(from.size.0);
        let rel_y = f64::from(y) / f64::from(from.size.1);
        let factor = to.scale / from.scale;
        let nw = ((f64::from(w) * factor).round() as i32).clamp(1, to.size.0);
        let nh = ((f64::from(h) * factor).round() as i32).clamp(1, to.size.1);
        let nx = ((rel_x * f64::from(to.size.0)).round() as i32).clamp(0, to.size.0 - nw);
        let ny = ((rel_y * f64::from(to.size.1)).round() as i32).clamp(0, to.size.1 - nh);
        self.output = name.to_string();
        self.a = (nx, ny);
        self.b = (nx + nw - 1, ny + nh - 1);
    }

    /// ボタンを押した（出力 `output` の点 `point`）。`move_existing` は Mod を押したまま（選択ごと動かす）。
    /// 選択を描き直し始めたら true。
    pub fn pointer_down(&mut self, output: &str, point: (i32, i32), move_existing: bool) -> bool {
        if self.button != Button::Up {
            return false;
        }
        if move_existing {
            if output != self.output {
                return false;
            }
            self.button = Button::Down {
                last: (output.to_string(), point),
                moving: Some((point.0 - self.a.0, point.1 - self.a.1)),
                by_space: false,
            };
            return true;
        }
        let Some(info) = self.outputs.get(output).copied() else {
            return false;
        };
        self.button = Button::Down {
            last: (output.to_string(), point),
            moving: None,
            by_space: false,
        };
        let p = (
            point.0.clamp(0, info.size.0 - 1),
            point.1.clamp(0, info.size.1 - 1),
        );
        self.output = output.to_string();
        self.a = p;
        self.b = p;
        true
    }

    /// ポインタが選択の出力の点 `point` へ動いた（出力の外でもよい）。
    pub fn pointer_motion(&mut self, point: (i32, i32)) {
        let Some(info) = self.info() else { return };
        let (a, b) = (self.a, self.b);
        let Button::Down { last, moving, .. } = &mut self.button else {
            return;
        };
        last.1 = point;
        match *moving {
            Some(offset) => {
                let (x, y, w, h) = rect_from_corners(a, b);
                let want = (point.0 - offset.0, point.1 - offset.1);
                // 角 `a` の移動量を、矩形の左上が出力に収まるように切る。
                let left = x + (want.0 - a.0);
                let top = y + (want.1 - a.1);
                let cx = left.clamp(0, (info.size.0 - w).max(0));
                let cy = top.clamp(0, (info.size.1 - h).max(0));
                let (dx, dy) = (cx - x, cy - y);
                self.a = (a.0 + dx, a.1 + dy);
                self.b = (b.0 + dx, b.1 + dy);
            }
            None => {
                self.b = (
                    point.0.clamp(0, info.size.0 - 1),
                    point.1.clamp(0, info.size.1 - 1),
                );
            }
        }
    }

    /// ボタンを離した。押していなければ `None`。
    /// 選択が 0 か 1 画素なら、その点のまわり 32 px（出力に収まる分）にする（niri と同じ）。
    pub fn pointer_up(&mut self) -> Option<bool> {
        if self.button == Button::Up {
            return None;
        }
        self.button = Button::Up;
        let (x, y, w, h) = self.rect();
        if w <= 1
            && h <= 1
            && let Some(info) = self.info()
        {
            let x1 = (x - 16).max(0);
            let y1 = (y - 16).max(0);
            let x2 = (x - 16 + 32).min(info.size.0);
            let y2 = (y - 16 + 32).min(info.size.1);
            if x2 > x1 && y2 > y1 {
                self.a = (x1, y1);
                self.b = (x2 - 1, y2 - 1);
            }
        }
        Some(false)
    }

    /// Space を押した・離した。ボタンを押している間の Space は、選択ごと動かす（niri の `set_space_down`）。
    pub fn set_space_down(&mut self, down: bool) {
        let a = self.a;
        if let Button::Down {
            last,
            moving,
            by_space,
        } = &mut self.button
        {
            if down {
                if moving.is_none() {
                    *moving = Some((last.1.0 - a.0, last.1.1 - a.1));
                    *by_space = true;
                }
            } else if *by_space {
                *moving = None;
                *by_space = false;
            }
        }
    }

    /// ボタンを押しているか（Space を保存ではなく移動に使う）。
    pub fn is_button_down(&self) -> bool {
        self.button != Button::Up
    }

    /// 選択の外を暗くする 4 つの矩形と、選択の枠の 4 つの矩形（選択の出力の物理 px）。ほかの出力は全体を暗くする。
    pub fn shades_and_border(&self, output: &str) -> (Vec<Px>, Vec<Px>) {
        let Some(info) = self.outputs.get(output).copied() else {
            return (Vec::new(), Vec::new());
        };
        let (sw, sh) = info.size;
        if output != self.output {
            return (vec![(0, 0, sw, sh)], Vec::new());
        }
        let (x, y, w, h) = self.rect();
        let bw = physical(info.scale, SELECTION_BORDER);
        let shades = vec![
            (0, 0, sw, y),
            (0, y + h, sw, sh - y - h),
            (0, y, x, h),
            (x + w, y, sw - x - w, h),
        ];
        let border = vec![
            (x - bw, y - bw, w + bw * 2, bw),
            (x - bw, y + h, w + bw * 2, bw),
            (x - bw, y, bw, h),
            (x + w, y, bw, h),
        ];
        (shades, border)
    }
}

/// 出力の中央の 1/2 の大きさの矩形。
fn default_rect(size: (i32, i32)) -> Px {
    (size.0 / 4, size.1 / 4, size.0 / 2, size.1 / 2)
}

/// 大きさ `current`（物理 px）を `change` で変えた値（物理 px。範囲は呼び出し側が収める）。
fn new_size(change: SizeChange, current: i32, available: i32, scale: f64) -> i32 {
    let available_f = f64::from(available);
    match change {
        SizeChange::SetFixed(px) => physical(scale, f64::from(px)),
        SizeChange::SetProportion(p) => (available_f * (p / 100.0).clamp(0.0, 1.0)).round() as i32,
        SizeChange::AdjustProportion(d) => {
            let prop = (f64::from(current) / available_f + d / 100.0).clamp(0.0, 1.0);
            (available_f * prop).round() as i32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outputs() -> BTreeMap<String, OutputInfo> {
        let mut m = BTreeMap::new();
        m.insert(
            "A".to_string(),
            OutputInfo {
                size: (1000, 800),
                scale: 1.0,
            },
        );
        m.insert(
            "B".to_string(),
            OutputInfo {
                size: (2000, 1600),
                scale: 2.0,
            },
        );
        m
    }

    fn ui() -> Ui {
        Ui::open(outputs(), "A", None, true).unwrap_or_else(|| unreachable!())
    }

    #[test]
    fn it_opens_in_the_middle_half_or_on_the_last_selection() {
        assert_eq!(ui().rect(), (250, 200, 500, 400));
        let last = Some(("B".to_string(), (10, 20, 30, 40)));
        let u = Ui::open(outputs(), "A", last, true).unwrap_or_else(|| unreachable!());
        assert_eq!((u.output.as_str(), u.rect()), ("B", (10, 20, 30, 40)));
        // 前回の出力が無ければ既定。
        let gone = Some(("C".to_string(), (1, 1, 1, 1)));
        let u = Ui::open(outputs(), "A", gone, true).unwrap_or_else(|| unreachable!());
        assert_eq!(u.rect(), (250, 200, 500, 400));
    }

    #[test]
    fn dragging_draws_a_new_selection_clamped_to_the_output() {
        let mut u = ui();
        assert!(u.pointer_down("A", (100, 100), false));
        u.pointer_motion((2000, -5));
        assert_eq!(u.rect(), (100, 0, 900, 101));
        assert_eq!(u.pointer_up(), Some(false));
        assert_eq!(u.pointer_up(), None);
    }

    #[test]
    fn a_click_without_drag_makes_a_small_box() {
        let mut u = ui();
        u.pointer_down("A", (5, 400), false);
        u.pointer_up();
        assert_eq!(u.rect(), (0, 384, 21, 32));
    }

    #[test]
    fn space_or_mod_moves_the_whole_selection_within_the_output() {
        let mut u = ui();
        u.pointer_down("A", (250, 200), true);
        u.pointer_motion((0, 0));
        assert_eq!(u.rect(), (0, 0, 500, 400));
        u.pointer_motion((900, 900));
        assert_eq!(u.rect(), (500, 400, 500, 400));
        u.pointer_up();
        let mut v = ui();
        v.pointer_down("A", (300, 300), false);
        v.pointer_motion((400, 400));
        v.set_space_down(true);
        v.pointer_motion((450, 410));
        assert_eq!(v.rect(), (350, 310, 101, 101));
        v.set_space_down(false);
        v.pointer_motion((500, 500));
        assert_eq!(v.rect(), (350, 310, 151, 191));
    }

    #[test]
    fn arrows_move_by_fifty_logical_pixels_and_stop_at_the_edge() {
        let mut u = ui();
        u.move_by(-1, 0);
        assert_eq!(u.rect(), (200, 200, 500, 400));
        for _ in 0..10 {
            u.move_by(-1, -1);
        }
        assert_eq!(u.rect(), (0, 0, 500, 400));
        u.move_to_output("B");
        // 相対位置 0、論理の大きさを保つ（拡大率 2 なので物理は 2 倍）。
        assert_eq!((u.output.as_str(), u.rect()), ("B", (0, 0, 1000, 800)));
        u.move_by(1, 0);
        assert_eq!(u.rect(), (100, 0, 1000, 800));
    }

    #[test]
    fn width_and_height_change_like_niri() {
        let mut u = ui();
        u.set_width(SizeChange::AdjustProportion(10.0));
        assert_eq!(u.rect(), (250, 200, 600, 400));
        u.set_width(SizeChange::SetProportion(100.0));
        assert_eq!(u.rect(), (250, 200, 750, 400));
        u.set_height(SizeChange::SetFixed(10));
        assert_eq!(u.rect(), (250, 200, 750, 10));
    }

    #[test]
    fn keys_follow_niri() {
        assert_eq!(key_action("Escape", false, true, true), Some(UiKey::Cancel));
        assert_eq!(
            key_action("space", false, false, false),
            Some(UiKey::Confirm)
        );
        assert_eq!(
            key_action("Return", false, false, false),
            Some(UiKey::Confirm)
        );
        assert_eq!(
            key_action("c", true, false, false),
            Some(UiKey::ConfirmToClipboard)
        );
        assert_eq!(
            key_action("p", false, false, false),
            Some(UiKey::TogglePointer)
        );
        assert_eq!(key_action("space", false, false, true), None);
        assert_eq!(key_action("q", false, false, false), None);
    }

    #[test]
    fn the_other_outputs_are_fully_shaded() {
        let u = ui();
        let (shades, border) = u.shades_and_border("B");
        assert_eq!(shades, [(0, 0, 2000, 1600)]);
        assert!(border.is_empty());
        let (shades, border) = u.shades_and_border("A");
        assert_eq!(shades.len(), 4);
        assert_eq!(border.first(), Some(&(248, 198, 504, 2)));
    }
}
