//! The name of the slice under the pointer, shown in the pill of the notices (see
//! [`ui::hud`](crate::ui::hud)) while the pie is open.
//!
//! The name used to be drawn under the icon of each slice, where it crowded the icon. Now the pie
//! and the pill are on screen at the same time: the pill sits where the notices of actions appear, its badge
//! carries the icon of the slice the pointer is on and its text the name of the slice. Moving to
//! another slice swaps the text and the icon with the pill's usual animation (the old ones fade
//! out and rise while the new ones come in, the badge bounces, the pill glides to the new width);
//! the first slice makes the pill come in, and leaving every slice makes it go away, both the way a
//! notice does.
//!
//! While the names are on, the window of the pie is as big as the display for as long as the pie is
//! open (`App::show_pie`; `App::hide_root` makes it small again), so the pill is at the bottom center
//! of the display, always in the same place, and not beside the ring wherever the pie was called.
//!
//! The pill is painted in the window of the pie, not in the window of the notices: it is a second
//! thing drawn by [`App::pie_ui`], with its own [`Hud`] (`App::slice_pill`), so it never takes the
//! window from the pie and the notices of actions are not touched. It is on when *Show slice names*
//! is on (`Config::show_labels`).

use super::view::PieView;
use crate::config::cfg;
use crate::ui::app::{App, pill_center};
use crate::ui::draw::{draw_icon, ease_out_cubic};
use crate::ui::hud::{self, Hud, HudNote, PillLook};
use crate::ui::screen::screen_rect_at;
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use std::time::{Duration, Instant};

/// How long the pill stays after the pointer left the last slice, before it goes away. Long enough
/// for a pointer that crosses the ring from one slice to another by way of the middle not to make
/// it blink.
const HOLD: Duration = Duration::from_millis(160);
/// How long a name is on screen before the next one replaces it. Much shorter than the dwell of the
/// notices of actions: the pointer sweeps over the slices, and every one of them should be named.
const DWELL: Duration = Duration::from_millis(140);
/// Space kept between the pill and the edge of the window (points at scale 1).
const EDGE: f32 = 6.0;

/// The pill that follows the pointer over the slices of the open pie.
pub struct SlicePill {
    hud: Hud,
    /// What the pill was last told to show; `None` once the pointer left every slice.
    sent: Option<HudNote>,
    /// Where the pill sits for the pointer position it was last placed for (see [`Place`]).
    place: Option<Place>,
}

/// Where the pill goes, worked out for one pointer position and kept until that changes.
///
/// The place is a function of the display under the pointer and of the *Notice position* setting,
/// and neither changes while a ring is on screen. It used to be worked out again in every frame the
/// pill was painted: a list of the displays, which on X11 is a round trip to the X server and in
/// a Wayland session a read of the compositor's outputs, in the very frames that were already
/// busy animating the pill and the pie.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Place {
    /// The pointer position (pointer space) this was worked out for.
    pie_pos: Pos2,
    /// The size of the pill it was worked out for.
    scale: f32,
    /// Where the notice would be on the display (pointer space).
    at: Pos2,
    /// How many points of the window one point of the pointer's space is.
    k: f32,
}

impl Place {
    /// Works out the place for a pill of `scale` and a pointer at `pie_pos`.
    fn of(pie_pos: Pos2, scale: f32) -> Self {
        let k = crate::platform::window_scale(f64::from(pie_pos.x), f64::from(pie_pos.y)) as f32;
        let area = screen_rect_at(pie_pos)
            .unwrap_or_else(|| Rect::from_center_size(pie_pos, Vec2::new(1600.0, 1000.0)));
        let at = pill_center(cfg().hud_position, area, pie_pos, scale);
        Self {
            pie_pos,
            scale,
            at,
            k,
        }
    }
}

/// One frame of the pill: what to draw and with which textures. Made before the pie paints (it
/// needs `&mut App` to load the icons) and painted after it.
pub(super) struct PillFrame {
    place: Place,
    note: HudNote,
    prev: Option<HudNote>,
    look: PillLook,
    tex: Option<egui::TextureHandle>,
    prev_tex: Option<egui::TextureHandle>,
}

impl App {
    /// Steps the pill of the slice names for this frame: points it at the slice under the pointer,
    /// advances its animation and asks for the repaints it needs. `None` when there is nothing to
    /// draw (names are off, or the pill has gone away).
    pub(super) fn slice_pill_frame(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        view: &PieView,
    ) -> Option<PillFrame> {
        let now = Instant::now();
        // the slice under the pointer, if the pie is open and the slice has a name
        let want = if view.labels && self.pie_open {
            self.hover
                .and_then(|i| view.items.get(i))
                .filter(|(_, name, _)| !name.trim().is_empty())
                .map(|(icon, name, _)| HudNote::slice(icon, name))
        } else {
            None
        };
        match want {
            Some(note) => {
                if let Some(p) = self.slice_pill.as_mut() {
                    if p.sent.as_ref() != Some(&note) {
                        p.hud.follow(note.clone(), now);
                        p.sent = Some(note);
                    }
                } else {
                    let scale = hud::scale_of(&cfg());
                    let hud = Hud::new(note.clone(), HOLD)
                        .with_scale(scale)
                        .with_dwell(DWELL)
                        .pinned();
                    self.slice_pill = Some(SlicePill {
                        hud,
                        sent: Some(note),
                        place: None,
                    });
                }
            }
            None => {
                if let Some(p) = self.slice_pill.as_mut()
                    && p.sent.take().is_some()
                {
                    p.hud.unpin(now);
                }
            }
        }

        // The pill goes with the pie when the pie is *closing*, not whenever the bloom clock runs
        // from zero: a submenu starts the clock again (`App::show_pie`) while the pie stays open,
        // and the pill, which follows the pointer from one ring to the next, blinked out and in
        // with it at every change of level.
        let pie_fade = if self.pie_open {
            1.0
        } else {
            ease_out_cubic(self.anim)
        };
        let pie_pos = self.pie_pos;
        let (accent, smoked) = {
            let c = cfg();
            (hud::accent_of(&c), hud::smoked(&c))
        };
        let p = self.slice_pill.as_mut()?;
        let phase = p.hud.phase(now);
        if phase.over {
            self.slice_pill = None;
            return None;
        }
        // where it goes: worked out again only when the pointer's position or the size changed
        let scale = p.hud.scale;
        let place = match p.place {
            Some(place) if place.pie_pos == pie_pos && place.scale == scale => place,
            _ => {
                let place = Place::of(pie_pos, scale);
                p.place = Some(place);
                place
            }
        };
        // a name that came too soon after the last one takes its turn now
        p.hud.flush_queued(now);
        let pulse = p.hud.pulse(now);
        let note = p.hud.note.clone();
        let (prev, swap) = p.hud.previous(now);
        let prev = prev.cloned();
        // the pill glides to the width its name asks for instead of jumping to it
        let size = p.hud.morph_size(hud::pill_size(painter, &note, scale), now);
        let wake = p.hud.next_wake(now);
        let moving = p.hud.gliding(now);
        if phase.animating() || pulse > 0.0 || moving {
            ctx.request_repaint();
        } else if let Some(wait) = wake {
            // nothing moves until the pill is let go (or the name changes)
            ctx.request_repaint_after(wait);
        }

        let look = PillLook {
            accent,
            smoked,
            // it goes with the pie when the pie is closed (the window is hidden once that is over)
            alpha: phase.alpha() * pie_fade,
            lift: phase.offset(),
            pulse,
            pop: phase.pop(),
            grow: phase.grow(),
            text: phase.text(),
            scale,
            elapsed: 0.0,
            size: Some(size),
            swap,
            sweep: 0.0,
            bar: None,
        };
        let tex = note.icon.as_deref().and_then(|s| self.ed.icon_for(ctx, s));
        let prev_tex = prev
            .as_ref()
            .and_then(|n| n.icon.as_deref())
            .and_then(|s| self.ed.icon_for(ctx, s));
        Some(PillFrame {
            place,
            note,
            prev,
            look,
            tex,
            prev_tex,
        })
    }
}

impl PillFrame {
    /// Paints the pill for a ring centered on `c` (window points) whose pointer position is
    /// `pie_pos`, in a window `rect`.
    ///
    /// The pill goes where the notices of actions go (*Notice position* in Settings: bottom or top
    /// of the display, or by the pointer), not next to the ring. While the names are on, the pie
    /// window is as big as the display (see `App::show_pie`), so that place is inside it and the
    /// pill stays at the bottom center whatever the place the pie was called at. The clamp below is
    /// only for a window that could not be made that big (a display it does not fit on).
    pub(super) fn paint(&self, painter: &egui::Painter, rect: Rect, c: Pos2, pie_pos: Pos2) {
        let scale = self.look.scale;
        let size = self
            .look
            .size
            .unwrap_or_else(|| Vec2::new(hud::PILL_MIN_W, hud::PILL_H) * scale);
        let half = size / 2.0;
        let edge = EDGE * scale;
        // where the notice would be on the display, in the window (worked out once, see `Place`)
        let Place { at, k, .. } = self.place;
        let want = c + (at - pie_pos) * k;
        let x = want
            .x
            .min(rect.max.x - half.x - edge)
            .max(rect.min.x + half.x + edge);
        let y = want
            .y
            .min(rect.max.y - half.y - edge)
            .max(rect.min.y + half.y + edge);

        let art = |p: &egui::Painter, n: &HudNote, at: Pos2, size: f32, ink: Color32| {
            let tex = if n.icon == self.note.icon {
                self.tex.as_ref()
            } else {
                self.prev_tex.as_ref()
            };
            draw_icon(p, tex, n.icon.as_deref().unwrap_or(""), at, size, ink);
        };
        let art: hud::BadgeArt<'_> = &art;
        hud::paint_pill_icons(
            painter,
            Pos2::new(x, y),
            &self.note,
            self.prev.as_ref(),
            &self.look,
            Some(art),
        );
    }
}
