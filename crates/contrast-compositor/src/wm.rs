//! 窓の管理: 並び順の保持、ポリシーによる配置、フォーカス。
//!
//! 未使用（どこからも `mod` されていない）。shell.rs（contrast-core への翻訳層） に置き換えた。削除は人間が判断する。

use smithay::{
    desktop::Window,
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::protocol::wl_surface::WlSurface,
    },
    utils::SERIAL_COUNTER,
};

use contrast_core::Rect;

use crate::state::Contrast;

/// コンポジタが管理するトップレベル窓 1 つ。
pub struct Managed {
    /// ログ用の通し番号。
    pub id: u64,
    /// Smithay の窓。
    pub window: Window,
    /// 属するワークスペース（0 始まり）。
    pub workspace: u8,
    /// 最後にフォーカスされた順番（大きいほど新しい）。
    pub focus_stamp: u64,
    /// 最後の配置で割り当てたタイル（枠を含む）。未配置なら `None`。
    pub tile: Option<Rect>,
}

/// タイルを四辺から `b` だけ縮めた矩形。縮められなければタイルそのもの。
fn inset(tile: Rect, b: i32) -> Rect {
    let twice = b.saturating_mul(2);
    Rect::new(
        tile.x().saturating_add(b),
        tile.y().saturating_add(b),
        tile.width().saturating_sub(twice),
        tile.height().saturating_sub(twice),
    )
    .unwrap_or(tile)
}

impl Contrast {
    /// 窓のトップレベル surface。
    fn surface_of(window: &Window) -> WlSurface {
        window
            .toplevel()
            .expect("only wayland toplevels are managed")
            .wl_surface()
            .clone()
    }

    /// surface に対応する管理中の窓の位置。
    pub fn index_of_surface(&self, surface: &WlSurface) -> Option<usize> {
        self.windows
            .iter()
            .position(|m| &Self::surface_of(&m.window) == surface)
    }

    /// 新しい窓を表示中のワークスペースの末尾に加え、並べ直してフォーカスする。
    pub fn add_window(&mut self, window: Window) {
        self.next_id += 1;
        let id = self.next_id;
        tracing::info!(window = id, workspace = self.active_ws + 1, "window added");
        self.windows.push(Managed {
            id,
            window: window.clone(),
            workspace: self.active_ws,
            focus_stamp: 0,
            tile: None,
        });
        self.relayout();
        self.focus_window(Some(&window));
    }

    /// surface の窓を管理から外し、並べ直す。
    ///
    /// クライアントが自分で終了した場合も toplevel_destroyed 経由でここに来る。
    /// フォーカスを失ったら、表示中の並びで隣の窓（右隣、末尾だったなら左隣）へ移す。
    pub fn remove_window(&mut self, surface: &WlSurface) {
        let Some(index) = self.index_of_surface(surface) else {
            return;
        };
        let position = self
            .visible_windows()
            .iter()
            .position(|w| *w == self.windows[index].window);
        let removed = self.windows.remove(index);
        tracing::info!(window = removed.id, "window removed");
        self.space.unmap_elem(&removed.window);
        self.relayout();
        if self.focused_index().is_none() {
            let visible = self.visible_windows();
            let neighbor = position
                .and_then(|p| visible.get(p).or_else(|| visible.last()))
                .cloned()
                .or_else(|| self.most_recent_visible());
            self.focus_window(neighbor.as_ref());
        }
    }

    /// 表示中のワークスペースの窓を、並び順で返す。
    pub fn visible_windows(&self) -> Vec<Window> {
        self.windows
            .iter()
            .filter(|m| m.workspace == self.active_ws)
            .map(|m| m.window.clone())
            .collect()
    }

    /// 表示中の窓のうち、最後にフォーカスされたもの。
    pub fn most_recent_visible(&self) -> Option<Window> {
        self.windows
            .iter()
            .filter(|m| m.workspace == self.active_ws)
            .max_by_key(|m| m.focus_stamp)
            .map(|m| m.window.clone())
    }

    /// 表示中で、タイル（枠を含む）が点 `(x, y)` を含む窓。
    pub fn visible_window_at(&self, x: i32, y: i32) -> Option<&Managed> {
        let p = contrast_core::Point::new(x, y);
        self.windows
            .iter()
            .filter(|m| m.workspace == self.active_ws)
            .find(|m| m.tile.is_some_and(|t| t.contains(p)))
    }

    /// フォーカス枠を描くべきタイル（表示中のフォーカス窓のもの）。
    pub fn focused_tile(&self) -> Option<Rect> {
        let m = &self.windows[self.focused_index()?];
        if m.workspace == self.active_ws {
            m.tile
        } else {
            None
        }
    }

    /// キーボードフォーカスを持つ管理中の窓の位置。
    pub fn focused_index(&self) -> Option<usize> {
        let keyboard = self.seat.get_keyboard()?;
        let focus = keyboard.current_focus()?;
        self.index_of_surface(&focus)
    }

    /// 表示中の窓をポリシーの配置で並べ、非表示の窓は Space から外す。
    pub fn relayout(&mut self) {
        let Some(output) = self.space.outputs().next().cloned() else {
            return;
        };
        let geo = self
            .space
            .output_geometry(&output)
            .expect("mapped output has geometry");
        let Some(area) = Rect::new(geo.loc.x, geo.loc.y, geo.size.w, geo.size.h) else {
            tracing::warn!(?geo, "output area is empty, skip layout");
            return;
        };

        for m in self
            .windows
            .iter()
            .filter(|m| m.workspace != self.active_ws)
        {
            self.space.unmap_elem(&m.window);
        }

        let visible: Vec<(u64, Window)> = self
            .windows
            .iter()
            .filter(|m| m.workspace == self.active_ws)
            .map(|m| (m.id, m.window.clone()))
            .collect();
        let tiles = self.policy.layout(area, visible.len());
        assert_eq!(
            tiles.len(),
            visible.len(),
            "policy returned wrong rect count"
        );
        let border = self.policy.border_width().max(0);

        for ((id, window), tile) in visible.into_iter().zip(tiles) {
            if let Some(m) = self.windows.iter_mut().find(|m| m.id == id) {
                m.tile = Some(tile);
            }
            let rect = inset(tile, border);
            let toplevel = window.toplevel().expect("wayland toplevel");
            toplevel.with_pending_state(|s| {
                s.size = Some((rect.width(), rect.height()).into());
                s.states.set(xdg_toplevel::State::TiledLeft);
                s.states.set(xdg_toplevel::State::TiledRight);
                s.states.set(xdg_toplevel::State::TiledTop);
                s.states.set(xdg_toplevel::State::TiledBottom);
            });
            // 初回 configure は最初の commit で送る（handler::xdg_shell）。
            if toplevel.is_initial_configure_sent() {
                toplevel.send_pending_configure();
            }
            self.space.map_element(window, (rect.x(), rect.y()), false);
            tracing::info!(
                window = id,
                x = rect.x(),
                y = rect.y(),
                w = rect.width(),
                h = rect.height(),
                border,
                "layout"
            );
        }
    }

    /// キーボードフォーカスを `window` に移し、activated 状態を全窓に反映する。
    pub fn focus_window(&mut self, window: Option<&Window>) {
        let serial = SERIAL_COUNTER.next_serial();
        let surface = window.map(Self::surface_of);
        self.focus_counter += 1;
        let stamp = self.focus_counter;
        let mut focused_id = None;
        for m in &mut self.windows {
            let active = Some(&m.window) == window;
            if active {
                m.focus_stamp = stamp;
                focused_id = Some(m.id);
            }
            m.window.set_activated(active);
            let toplevel = m.window.toplevel().expect("wayland toplevel");
            if toplevel.is_initial_configure_sent() {
                toplevel.send_pending_configure();
            }
        }
        let keyboard = self.seat.get_keyboard().expect("seat has keyboard");
        keyboard.set_focus(self, surface, serial);
        tracing::info!("focus window={}", crate::handler::fmt_id(focused_id));
    }
}
