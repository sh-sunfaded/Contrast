//! 画面の電源（DPMS）の状態。`power-off-monitors` で消し、どんな入力でも戻す（niri と同じ）。
//!
//! 実際に CRTC を止める・戻すのはバックエンドが入れる関数（DRM だけ）。バックエンドが入れていなければ
//! （winit の nested）何もしない。niri の winit も何もしない。この型は時計も DRM も触らない。

/// 画面の電源を切る・戻すバックエンドの関数。引数が `true` なら戻す。
pub type PowerHook = Box<dyn FnMut(bool)>;

/// 画面の電源の状態。
#[derive(Default)]
pub struct Monitors {
    /// バックエンドの関数（DRM だけが入れる）。
    hook: Option<PowerHook>,
    /// 画面を消している間 true。
    off: bool,
}

impl Monitors {
    /// バックエンドの関数を入れる。
    pub fn set_hook(&mut self, hook: PowerHook) {
        self.hook = Some(hook);
    }

    /// 画面を消している間 true。
    pub fn is_off(&self) -> bool {
        self.off
    }

    /// 画面を消す。バックエンドが無い、または消している最中なら何もしない。消したら true。
    pub fn power_off(&mut self) -> bool {
        if self.off {
            return false;
        }
        match self.hook.as_mut() {
            Some(hook) => {
                hook(false);
                self.off = true;
                true
            }
            None => false,
        }
    }

    /// 画面を戻す（入力のたびに呼ぶ）。消していなければ何もしない。戻したら true。
    pub fn wake(&mut self) -> bool {
        if !self.off {
            return false;
        }
        self.off = false;
        if let Some(hook) = self.hook.as_mut() {
            hook(true);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn with_log() -> (Monitors, Rc<RefCell<Vec<bool>>>) {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut m = Monitors::default();
        let sink = log.clone();
        m.set_hook(Box::new(move |on| sink.borrow_mut().push(on)));
        (m, log)
    }

    #[test]
    fn power_off_then_any_input_wakes_once() {
        let (mut m, log) = with_log();
        assert!(!m.is_off());
        assert!(m.power_off());
        assert!(m.is_off());
        assert!(m.wake());
        assert!(!m.is_off());
        // 戻したあとの入力は何もしない。
        assert!(!m.wake());
        assert_eq!(*log.borrow(), vec![false, true]);
    }

    #[test]
    fn powering_off_twice_calls_the_backend_once() {
        let (mut m, log) = with_log();
        assert!(m.power_off());
        assert!(!m.power_off());
        assert_eq!(*log.borrow(), vec![false]);
    }

    #[test]
    fn wake_without_power_off_does_nothing() {
        let (mut m, log) = with_log();
        assert!(!m.wake());
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn without_a_backend_nothing_happens() {
        let mut m = Monitors::default();
        assert!(!m.power_off());
        assert!(!m.is_off());
        assert!(!m.wake());
    }
}
