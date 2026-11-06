//! The virtual trackpad's windows: two more native windows, next to the one that shows the pie and
//! Settings, that exist while the tool is open (`cargo tools trackpad`): the trackpad, exactly in
//! the middle of the display, and the small arrow of the virtual pointer it moves.
//!
//! # Why the main window is parked, not hidden
//!
//! egui keeps a child window (a *viewport*) alive only while the parent window's frames keep
//! declaring it, and a hidden window gets no frames: the app calls `logic` for it but never `ui`.
//! So while the trackpad is open the main window is not hidden when the pie closes: it is *parked*,
//! 1 x 1 point in the corner of the display, transparent and click-through, which draws nothing and
//! keeps the frames coming. It is hidden for real when the trackpad is closed.
//!
//! # Above or below the pie
//!
//! The pie opens around the virtual pointer. All three windows are always on top, and which one is
//! above is the window manager's choice, so while a pie is open the trackpad and the pointer step
//! down to normal windows and the pie is surely above them.

use super::App;
use crate::platform;
use crate::tap;
use crate::ui::screen::screen_rect_at;
use crate::ui::vtrackpad::draw::{self, Shared};
use crate::ui::vtrackpad::{Cue, centered, pointer, pointer_start};
use eframe::egui::{self, Pos2, Rect, ViewportBuilder, ViewportId};
use std::sync::{Arc, Mutex, MutexGuard};

/// The trackpad window's id.
pub(super) fn vt_id() -> ViewportId {
    ViewportId::from_hash_of("cursorpie-virtual-trackpad")
}

/// What the windows' level should be: normal while a pie is open (so the pie is above them).
pub(super) fn wanted_level(pie_open: bool) -> egui::WindowLevel {
    if pie_open {
        egui::WindowLevel::Normal
    } else {
        egui::WindowLevel::AlwaysOnTop
    }
}

fn lock(m: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// All the displays together (where the virtual pointer may go), if the system can tell.
fn all_displays() -> Option<Rect> {
    platform::display_rects()
        .iter()
        .map(|r| {
            Rect::from_min_size(
                Pos2::new(r.x as f32, r.y as f32),
                egui::vec2(r.w as f32, r.h as f32),
            )
        })
        .reduce(|a, b| a.union(b))
}

impl App {
    /// Whether the trackpad is wanted now.
    fn vt_wanted(&self) -> bool {
        self.vt_open && !self.quitting
    }

    /// Runs in `logic` every frame: keeps the trackpad in step with the pie and Settings.
    pub(super) fn vt_logic(&mut self, ctx: &egui::Context) {
        let wanted = self.vt_wanted();
        let close = {
            let mut sh = lock(&self.vt);
            if wanted && (sh.blocked != self.settings_open || sh.pie_open != self.pie_open) {
                ctx.request_repaint_of(vt_id()); // what it says depends on both
            }
            sh.blocked = self.settings_open;
            sh.pie_open = self.pie_open;
            if wanted && (sh.width - self.vt_width).abs() > f32::EPSILON {
                // the main window declares the new size
                ctx.request_repaint_of(ViewportId::ROOT);
            }
            std::mem::take(&mut sh.close_requested)
        };
        if close {
            self.vt_open = false;
            ctx.request_repaint_of(ViewportId::ROOT);
        }
        let wanted = self.vt_wanted();
        if wanted != self.vt_shown {
            self.vt_shown = wanted;
            if wanted {
                // a hidden window gets no frames, and the frames are what open the trackpad
                if !self.visible && !self.settings_open && !self.vt_parked {
                    self.park_root(ctx);
                }
            } else {
                tap::virtual_reset(); // a finger still down must not leave a gesture behind
                platform::set_virtual_pointer(None);
                let mut sh = lock(&self.vt);
                sh.tracker = Default::default();
                sh.pointer = None;
                self.vt_display = None;
                self.vt_pointer_start = None;
            }
            ctx.request_repaint_of(ViewportId::ROOT);
        }
        if wanted {
            let level = wanted_level(self.pie_open);
            if level != self.vt_level {
                self.vt_level = level;
                ctx.send_viewport_cmd_to(vt_id(), egui::ViewportCommand::WindowLevel(level));
                ctx.send_viewport_cmd_to(pointer::id(), egui::ViewportCommand::WindowLevel(level));
            }
        }
    }

    /// Runs in `ui` (a full frame of the main window): declares the trackpad's windows, or, when it
    /// is no longer wanted, lets them go and hides the main window if nothing else needs it.
    pub(super) fn vt_ui(&mut self, ctx: &egui::Context) {
        if !self.vt_wanted() {
            if self.vt_parked {
                self.vt_parked = false;
                if !self.settings_open && !self.pie_open && !self.visible {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
            }
            return;
        }
        let width = lock(&self.vt).width;
        self.vt_width = width;
        let size = draw::window_size(width);
        let display = match self.vt_display {
            Some(d) => d,
            None => {
                // the display the real pointer is on when the tool opens
                let at = self.cursor_or_default();
                let d = screen_rect_at(at)
                    .unwrap_or(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 800.0)));
                self.vt_display = Some(d);
                d
            }
        };
        let pos = centered(display, size);
        // the virtual pointer begins above the trackpad
        let start = match self.vt_pointer_start {
            Some(p) => p,
            None => {
                let p = pointer_start(display, Rect::from_min_size(pos, size));
                self.vt_pointer_start = Some(p);
                let mut sh = lock(&self.vt);
                sh.bounds = Some(all_displays().unwrap_or(display));
                sh.set_pointer(ctx, Some(p));
                p
            }
        };

        let mut builder = ViewportBuilder::default()
            .with_title("Virtual trackpad")
            .with_inner_size(size)
            .with_position(pos)
            .with_decorations(false)
            .with_transparent(true)
            .with_has_shadow(false)
            .with_resizable(false)
            .with_always_on_top()
            .with_active(false);
        if platform::OS == platform::Os::Windows {
            builder = builder.with_taskbar(false);
        }
        if platform::OS == platform::Os::Linux {
            // CursorPie's own windows are never "the app under the pointer"
            builder = builder.with_app_id("cursorpie");
        }
        let shared = Arc::clone(&self.vt);
        ctx.show_viewport_deferred(vt_id(), builder, move |ui, class| {
            if class == egui::ViewportClass::EmbeddedWindow {
                return; // no native second window here: nothing to show
            }
            draw::show(ui, &shared);
        });
        ctx.show_viewport_deferred(pointer::id(), pointer::builder(start), |ui, class| {
            if class == egui::ViewportClass::EmbeddedWindow {
                return;
            }
            pointer::show(ui);
        });
    }

    /// Where the pie window goes when it is not in use while the trackpad is open: 1 x 1 point in
    /// the lower right corner of the display, transparent and click-through.
    pub(super) fn park_root(&mut self, ctx: &egui::Context) {
        let at = self.cursor_or_default();
        let corner = screen_rect_at(at).map_or(Pos2::new(0.0, 0.0), |r| {
            Pos2::new(r.max.x - 1.0, r.max.y - 1.0)
        });
        self.vt_parked = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([1.0, 1.0].into()));
        ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
            egui::WindowLevel::AlwaysOnTop,
        ));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(corner));
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.request_repaint();
    }

    /// Puts the main window away: parked while the trackpad is open, hidden otherwise.
    pub(crate) fn hide_root(&mut self, ctx: &egui::Context) {
        if self.vt_wanted() {
            self.park_root(ctx);
        } else {
            self.vt_parked = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }

    /// A tick of the gesture: the trackpad shows it, if the finger on it is the one that made it.
    pub(super) fn vt_cue(&mut self, ctx: &egui::Context, cue: Cue) {
        if !self.vt_wanted() {
            return;
        }
        let mut sh = lock(&self.vt);
        if sh.tracker.finger().is_none() {
            return;
        }
        sh.cue(cue);
        drop(sh);
        ctx.request_repaint_of(vt_id());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pie_is_always_above_the_trackpad() {
        assert_eq!(wanted_level(true), egui::WindowLevel::Normal);
        assert_eq!(wanted_level(false), egui::WindowLevel::AlwaysOnTop);
    }
}
