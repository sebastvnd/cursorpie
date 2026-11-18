//! Drawing of the pie menu (one ring at a time; submenus re-center the same window).

pub mod genz;
pub mod glass;
mod trail;
mod view;

use crate::config::{Config, MAX_ITEMS, PieStyle, cfg};
use crate::geometry::*;
use crate::running::{self, Choice};
use crate::ui::app::App;
use crate::ui::draw::*;
use eframe::egui::{self, Align2, Color32, FontId, Shape, Stroke, Vec2};
use std::f32::consts::{PI, TAU};
use view::{CenterArt, PieView};

/// Seconds the pie takes to bloom open. One easing curve is applied on top of this linear clock
/// (see [`step_linear`]).
const OPEN_SECS: f32 = 0.22;
/// Seconds the pie takes to fade away. Closing is quicker than opening: the action already ran.
const CLOSE_SECS: f32 = 0.12;
/// Seconds the badges of an "Open app" slice take to come out, and to go back.
const BADGE_IN_SECS: f32 = 0.30;
const BADGE_OUT_SECS: f32 = 0.14;
/// Center disc while the side buttons follow the other pie: the same white with a pink cast.
const CORE_INACTIVE: Color32 = Color32::from_rgb(255, 238, 238);
/// The same in the dark appearance.
const CORE_INACTIVE_DARK: Color32 = Color32::from_rgb(52, 30, 32);
/// The badge of the app that is already open: a muted sage-gold, warm enough to tell apart from the
/// sage of the new-window badge and quiet enough to sit with it on a light pie.
const OPEN_APP_GOLD: Color32 = Color32::from_rgb(212, 190, 118);
/// The same on the smoked pie (dark appearance and glass), deeper so that the white square on it
/// reads.
const OPEN_APP_GOLD_SMOKED: Color32 = Color32::from_rgb(168, 144, 70);
/// Ink on the gold badge.
const OPEN_APP_GOLD_INK: Color32 = Color32::from_rgb(58, 50, 24);
/// Points the ring may grow past [`pie_outer`] while it overshoots, so it is never cut off by the
/// edge of the window (the window leaves 10 points of margin).
const OVERSHOOT_ROOM: f32 = 8.0;
/// Points beyond half a window that a window manager may push the pie window from where it was
/// asked to be and still count as keeping it on the screen (the menu bar of macOS is 24 to 38).
const PIE_SHIFT_SLACK: f32 = 64.0;

/// What the slices of the pie are made of: opaque and following the appearance, frosted glass, or
/// the thick-outlined Gen Z look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Material {
    Solid,
    Glass(glass::Glass),
    GenZ,
}

impl Material {
    /// The material the settings ask for.
    pub fn of(cfg: &Config) -> Self {
        Self::styled(cfg.pie_style, cfg)
    }

    /// The material of `style`, with the glass numbers of `cfg`: to show a style that is not the
    /// one chosen.
    pub fn styled(style: PieStyle, cfg: &Config) -> Self {
        match style {
            PieStyle::Solid => Self::Solid,
            PieStyle::Glass => Self::Glass(glass::Glass::from_percent(cfg)),
            PieStyle::GenZ => Self::GenZ,
        }
    }

    /// The colors of a pie of this material: glass and Gen Z look the same in either appearance.
    pub fn colors(self) -> PieColors {
        match self {
            Self::Solid => pie_colors(),
            Self::Glass(_) => glass_colors(),
            Self::GenZ => genz_colors(),
        }
    }
}

impl App {
    /// Draws the pie (one ring) and advances its animations. Hides the window once the close animation ended.
    pub fn pie_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let dt = if self.fresh {
            self.fresh = false;
            0.0
        } else {
            ctx.input(|i| i.stable_dt).min(0.05)
        };

        let trail_open = self.trail_shown();
        let (view, arts, shape, thread, glide) = {
            let c = cfg();
            let view = PieView::new(&c, self.profile, &self.path);
            // trail mode: each node shows what the center of its ring showed
            let arts: Vec<CenterArt> = if trail_open && c.submenu_trail {
                (0..self.path.len())
                    .map(|k| CenterArt::of(&c, self.profile, &self.path[..k]))
                    .collect()
            } else {
                Vec::new()
            };
            (
                view,
                arts,
                c.trail_shape(),
                c.trail_thread,
                f32::from(c.trail_glide) / 100.0,
            )
        };
        let total = view.items.len().max(1);
        let glass = glass::Glass::of(&cfg());
        let genz = genz::of(&cfg());
        let outline = cfg().outline();
        // Glass is always smoked graphite, even in the light appearance: pale glass on a light page
        // looks washed out. Gen Z is always white with dark ink. Solid follows the appearance.
        let smoked = glass.is_some() || (dark() && !genz);
        let colors = if genz {
            genz_colors()
        } else {
            pie_colors_for(smoked)
        };

        let (anim, mut moving) = step_linear(
            self.anim,
            if self.pie_open { 1.0 } else { 0.0 },
            dt,
            if self.pie_open { OPEN_SECS } else { CLOSE_SECS },
        );
        self.anim = anim;
        for i in 0..MAX_ITEMS {
            moving |= smooth_approach(
                &mut self.hover_anim[i],
                (self.hover == Some(i)) as u8 as f32,
                dt,
                20.0,
            );
            moving |= smooth_approach(
                &mut self.active_anim[i],
                (view.active == Some(i)) as u8 as f32,
                dt,
                12.0,
            );
            // the badges of an "Open app" slice: out while it is hovered and its app runs, back in
            // when the pointer leaves (a little quicker than they came)
            let offers = self.hover == Some(i)
                && view
                    .open_apps
                    .get(i)
                    .and_then(|a| a.as_deref())
                    .is_some_and(running::offers_choice);
            let (b, mv) = step_linear(
                self.badge_in[i],
                offers as u8 as f32,
                dt,
                if offers {
                    BADGE_IN_SECS
                } else {
                    BADGE_OUT_SECS
                },
            );
            self.badge_in[i] = b;
            moving |= mv;
            let picked = if offers { running::shown() } else { None };
            for choice in [Choice::NewWindow, Choice::Existing] {
                let k = badge_ix(choice);
                moving |= smooth_approach(
                    &mut self.badge_pick[i][k],
                    (picked == Some(choice)) as u8 as f32,
                    dt,
                    22.0,
                );
                moving |= smooth_approach(
                    &mut self.badge_step[i][k],
                    (picked.is_some() && picked != Some(choice)) as u8 as f32,
                    dt,
                    22.0,
                );
            }
        }

        if self.pie_open {
            // when the pie takes the pointer (a trackpad hold, a mouse hold on Wayland) the cursor
            // is the one of this window: a plain arrow, not whatever the last widget left behind
            ctx.set_cursor_icon(egui::CursorIcon::Default);
        }
        if !self.pie_open && self.anim <= 0.0 {
            // the web of a gesture that is over is not kept (the close animation has ended, so
            // nothing draws it any more)
            self.forget_trail();
            if self.visible {
                self.visible = false;
                self.hide_root(&ctx);
                if !self.panel_queue.is_empty() {
                    // a panel waited for the window (see `App::end_panel`)
                    ctx.request_repaint();
                }
            }
            return;
        }
        // Tell the tap the pie is on screen: the second drawn frame, so the window has been mapped.
        // The Settings ring starts counting from here, like the timer of the tap does.
        if self.pie_frames < 2 {
            self.pie_frames += 1;
            if self.pie_frames == 2 {
                self.opened_at = std::time::Instant::now();
                crate::tap::pie_painted();
            }
            ctx.request_repaint();
        }
        // trail mode: every node glides to its place in the web (see `trail::paint`)
        if !arts.is_empty() && self.trail.len() == arts.len() {
            if self.trail_disp.len() != self.trail.len() {
                self.trail_disp = self.trail.clone(); // out of step: start from the rings
            }
            let scale =
                crate::platform::window_scale(f64::from(self.pie_pos.x), f64::from(self.pie_pos.y))
                    as f32;
            let offs: Vec<(f32, f32)> = self
                .trail
                .iter()
                .map(|q| {
                    (
                        (q.x - self.pie_pos.x) * scale,
                        (q.y - self.pie_pos.y) * scale,
                    )
                })
                .collect();
            let k = 1.0 - (-dt * 16.0 * glide).exp();
            for n in trail_layout(&offs, self.win, shape) {
                let target = self.pie_pos + Vec2::new(n.x, n.y) / scale;
                let d = target - self.trail_disp[n.level];
                if d.length() * scale > 0.4 {
                    self.trail_disp[n.level] += d * k;
                    moving = true;
                } else {
                    self.trail_disp[n.level] = target;
                }
            }
        }
        let top_level = self.path.is_empty();
        let previewing = self.preview_until.is_some();
        // The "hold for Settings" ring grows while the pointer rests in the middle of the top ring,
        // so only then the pie has to repaint on its own. It used to repaint without end whenever
        // nothing was hovered at the top ring, long after the ring had closed: every frame rebuilt
        // the whole ring (glass is a mesh per slice) for nothing, and that load landed exactly when
        // the pointer came back from a submenu, so the change of level stuttered.
        let settle_running = self.pie_open
            && !previewing
            && self.hover.is_none()
            && top_level
            && view.settle_ms > 0.0
            && (self.opened_at.elapsed().as_millis() as f32) < view.settle_ms;
        if moving || settle_running {
            ctx.request_repaint();
        }

        // load image icons before painting (needs &mut self)
        let tex: Vec<_> = view
            .items
            .iter()
            .map(|(ic, _, _)| self.ed.icon_for(&ctx, ic))
            .collect();
        let art_tex: Vec<_> = arts
            .iter()
            .map(|a| {
                (
                    a.app_icon.as_ref().and_then(|s| self.ed.icon_for(&ctx, s)),
                    self.ed.icon_for(&ctx, &a.icon),
                )
            })
            .collect();
        let center_tex = self.ed.icon_for(&ctx, &view.center_icon);
        // None while not available (not installed, unreadable): the mode icon is shown instead
        let app_tex = view
            .app_icon
            .as_ref()
            .and_then(|s| self.ed.icon_for(&ctx, s));

        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ui, |ui| {
                let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
                let painter = ui.painter_at(rect);
                // The center of the ring is the point the pointer was on, which is the center of
                // the window unless the window manager kept the window on the screen and moved
                // it (near an edge or a corner): then the ring is drawn where the tap measures
                // from, not off it. The nodes of the trail are placed from this point too.
                let outer_min = ctx.input(|i| i.viewport().outer_rect).map(|r| r.min);
                // (`pie_off`: a submenu does not move the window while its ring fits in it, see
                // `App::show_pie`, so the ring is drawn that far from the center of the window)
                let c = rect.center()
                    + crate::ui::screen::pie_center_shift(
                        self.pie_origin,
                        outer_min,
                        rect.width(),
                        PIE_SHIFT_SLACK,
                    )
                    + self.pie_off;

                let fade = ease_out_cubic(self.anim);
                let pop = ease_out_back(self.anim);
                let r = pie_outer(self.win) * (0.80 + 0.20 * pop);
                let inner = pie_inner(self.win) * (0.75 + 0.25 * pop);
                let room = pie_outer(self.win) + OVERSHOOT_ROOM;
                // the ring is made round and bent to the outline of the pie (a circle by default)
                let bend = Bend::new(outline, inner, r);
                let accent = rgb(view.accent);
                // false = the side buttons follow the other pie: tint this one with the inactive color
                let live = !view.side.shown || view.side.owner;
                let a = |v: f32, f: f32| (v * f).clamp(0.0, 255.0) as u8;

                // trail mode: the rings that were left are nodes around this one, each on a thread
                // to its center, behind it
                if !arts.is_empty() && self.trail.len() == arts.len() {
                    let scale = crate::platform::window_scale(
                        f64::from(self.pie_pos.x),
                        f64::from(self.pie_pos.y),
                    ) as f32;
                    let offs: Vec<(f32, f32)> = self
                        .trail
                        .iter()
                        .map(|q| {
                            (
                                (q.x - self.pie_pos.x) * scale,
                                (q.y - self.pie_pos.y) * scale,
                            )
                        })
                        .collect();
                    let outer = pie_outer(self.win);
                    let nodes: Vec<trail::Node> = trail_layout(&offs, self.win, shape)
                        .into_iter()
                        .map(|n| {
                            let at = self.trail_disp[n.level];
                            // how far it still has to glide: it grows as it arrives
                            let target = self.pie_pos + Vec2::new(n.x, n.y) / scale;
                            let left =
                                ((at - target).length() * scale / (1.2 * outer)).clamp(0.0, 1.0);
                            let (app_tex, icon_tex) = &art_tex[n.level];
                            trail::Node {
                                at: c + (at - self.pie_pos) * scale,
                                r: n.r * (0.55 + 0.45 * (1.0 - left)),
                                near: n.level + 1 == arts.len(),
                                app_tex: app_tex.as_ref(),
                                app_icon_color: arts[n.level].app_icon_color,
                                icon: &arts[n.level].icon,
                                icon_tex: icon_tex.as_ref(),
                            }
                        })
                        .collect();
                    trail::paint(
                        &painter,
                        c,
                        &nodes,
                        &trail::Look {
                            accent,
                            colors,
                            fade,
                            thread,
                        },
                    );
                }

                // soft shadow behind the ring: one blurred shape, so it falls off smoothly (a stack
                // of discs leaves a visible step at the edge of each). A white ring needs it to
                // stand off a white page; glass needs less, it is see-through. Gen Z has a hard
                // shadow under each piece instead.
                if !genz {
                    let shade = a(if glass.is_some() { 26.0 } else { 40.0 }, fade);
                    ring_shadow(&painter, c, r, bend, shade);
                }

                let sl = TAU / total as f32;
                let gap = if total > 1 { 0.05 } else { 0.0 };

                for (i, (icon, name, is_sub)) in view.items.iter().enumerate().take(MAX_ITEMS) {
                    let h = self.hover_anim[i];
                    let m = self.active_anim[i];

                    // slices appear one after another (stagger) while rotating slightly
                    let lp = (self.anim * 1.6 - 0.6 * i as f32 / total as f32).clamp(0.0, 1.0);
                    let lpe = ease_out_back(lp).max(0.0);
                    let lf = ease_out_cubic(lp);
                    let start = slice_start(i, total) - (1.0 - lp) * 0.35;
                    let rr = (inner + (r + 5.0 * h - inner) * lpe).min(room);

                    let t = (h * 0.95 + m * 0.4).min(1.0);
                    let rest = colors.slice;
                    let col = if live {
                        mix(rest, view.accent, t)
                    } else {
                        // the active mode is tinted with the inactive color, hovering still uses the accent
                        mix(
                            mix(rest, view.inactive, (m * 0.4).min(1.0)),
                            view.accent,
                            (h * 0.95).min(1.0),
                        )
                    };
                    // icon and name: dark on a light slice, the readable color on the accent
                    let ink = colors
                        .ink
                        .lerp_to_gamma(on_accent(accent), (h * 0.95).min(1.0));
                    let (s0, s1) = (start + gap / 2.0, start + sl - gap / 2.0);
                    if let Some(g) = glass {
                        glass::slice(
                            &painter,
                            c,
                            bend,
                            (inner, rr),
                            (s0, s1),
                            col,
                            h,
                            lf,
                            g,
                            total == 1,
                        );
                    } else if genz {
                        genz::slice(
                            &painter,
                            c,
                            bend,
                            (inner, rr),
                            (s0, s1),
                            col,
                            h,
                            lf,
                            total == 1,
                        );
                    } else {
                        let fill =
                            Color32::from_rgba_unmultiplied(col[0], col[1], col[2], a(242.0, lf));
                        painter.add(ring_slice(&painter, c, inner, rr, s0, s1, fill, bend));
                        // A hairline around the slice keeps its shape readable on any background.
                        // A lone slice is a full ring: its outline would draw the seam as a line.
                        if total > 1 {
                            let edge = if dark() {
                                Color32::from_white_alpha(a(22.0 + 24.0 * h, lf))
                            } else {
                                Color32::from_black_alpha(a(16.0 + 20.0 * h, lf))
                            };
                            painter.add(Shape::closed_line(
                                bend.points(c, ring_slice_outline(c, inner, rr, s0, s1)),
                                Stroke::new(1.0, edge),
                            ));
                        }
                    }
                    // a bright bar grows along the rim of the hovered slice
                    if h > 0.02 && sl > 0.2 && !genz {
                        painter.add(ring_slice(
                            &painter,
                            c,
                            rr - 3.0,
                            rr,
                            s0 + 0.05,
                            s1 - 0.05,
                            on_accent(accent).gamma_multiply(0.55 * h * lf),
                            bend,
                        ));
                    }

                    let am = start + sl / 2.0;
                    let dir = Vec2::new(am.cos(), am.sin());
                    let tp = c + dir * bend.radius(am, (inner + rr) / 2.0);
                    // with a name the icon sits a little high; alone it is centered and larger
                    let (icon_dy, icon_size) = if view.labels {
                        (-r * 0.04, r * 0.30)
                    } else {
                        (0.0, r * 0.36)
                    };
                    draw_icon(
                        &painter,
                        tex[i].as_ref(),
                        icon,
                        tp + Vec2::new(0.0, icon_dy),
                        icon_size * (1.0 + 0.15 * h),
                        ink.gamma_multiply(lf),
                    );
                    if view.labels {
                        painter.text(
                            tp + Vec2::new(0.0, r * 0.17),
                            Align2::CENTER_CENTER,
                            name,
                            FontId::proportional((r * 0.09).max(9.0)),
                            ink.gamma_multiply(lf * (0.78 + 0.22 * h)),
                        );
                    }
                    // An "Open app" slice whose app already runs shows two round badges outside
                    // the rim, in front of the slice, while it is hovered: a new window, or the
                    // app that is open. They grow out of the rim one after the other, and the one
                    // the pointer is on is lifted while the other steps back.
                    // `is_running` is a lookup in a cache a worker filled at the press.
                    let bi = self.badge_in[i];
                    if bi > 0.0 {
                        for choice in [Choice::NewWindow, Choice::Existing] {
                            let k = badge_ix(choice);
                            // the second badge starts a little after the first
                            let local = ((bi - 0.22 * k as f32) / 0.78).clamp(0.0, 1.0);
                            if local <= 0.0 {
                                continue;
                            }
                            let (ox, oy) = running::badge_offset(self.win, i, total, choice);
                            // it slides out from the rim: starts 12 points closer to the slice
                            let at = c + Vec2::new(ox, oy)
                                - dir * (12.0 * (1.0 - ease_out_cubic(local)));
                            draw_open_badge(
                                &painter,
                                at,
                                running::badge_size(self.win),
                                choice,
                                BadgeMotion {
                                    // starts small and overshoots a little
                                    grow: 0.35 + 0.65 * ease_out_back(local),
                                    alpha: ease_out_cubic(local) * lf,
                                    pick: self.badge_pick[i][k],
                                    step: self.badge_step[i][k],
                                },
                                &BadgeStyle {
                                    accent,
                                    colors,
                                    glass,
                                    genz,
                                    smoked,
                                },
                            );
                        }
                    }
                    if *is_sub {
                        // dot on the rim: drag outward across it to open the submenu
                        painter.circle_filled(
                            c + dir * bend.radius(am, rr - 7.0),
                            2.5,
                            ink.gamma_multiply(lf * 0.8),
                        );
                    }
                }

                // ring: accent = the side buttons follow this pie, red = they follow the other one
                let (ring_col, ring_w) = if view.side.shown {
                    let col = if view.side.owner {
                        accent
                    } else {
                        rgb(view.inactive)
                    };
                    (col.gamma_multiply(fade), 3.0)
                } else if view.is_app {
                    (accent, 2.5)
                } else if genz {
                    genz::ring(fade, total == 1)
                } else {
                    (Color32::from_black_alpha(a(14.0, fade)), 1.0)
                };
                painter.add(bend.rim(c, r, Stroke::new(ring_w, ring_col)));

                // center disc: active mode (or the parent submenu icon) + "hold for settings" progress
                let core = inner * 0.86;
                let disc = if live {
                    colors.core
                } else if smoked {
                    CORE_INACTIVE_DARK
                } else {
                    CORE_INACTIVE
                };
                if let Some(g) = glass {
                    glass::core(&painter, c, core, fade, Some(disc), g);
                } else if genz {
                    genz::core(&painter, c, core, fade, disc);
                } else {
                    painter.circle_filled(c, core, disc.gamma_multiply(fade));
                    let edge = if dark() {
                        Color32::from_white_alpha(a(22.0, fade))
                    } else {
                        Color32::from_black_alpha(a(16.0, fade))
                    };
                    painter.circle_stroke(c, core, Stroke::new(1.0, edge));
                }
                if let Some(t) = app_tex.as_ref() {
                    // the app's icon: a white glyph tinted with the ink, or its own colors (white
                    // leaves the texture as it is, only fading and dimming it)
                    let tint = if view.app_icon_color {
                        Color32::WHITE.gamma_multiply(fade * if live { 1.0 } else { 0.55 })
                    } else {
                        colors
                            .ink
                            .gamma_multiply(fade * if live { 0.85 } else { 0.45 })
                    };
                    // a soft shadow under the icon, so it stands out from a plain white disc
                    let weight = icon_shadow_weight(view.app_icon_shadow, !smoked);
                    if weight > 0.0 {
                        let dim = fade * if live { 1.0 } else { 0.55 } * weight;
                        // stacked, slightly larger and lower copies of the icon in black: the
                        // alpha of the icon makes them a soft silhouette
                        for (down, grow, alpha) in
                            [(0.10, 1.07, 0.05), (0.07, 1.04, 0.07), (0.04, 1.0, 0.09)]
                        {
                            draw_texture(
                                &painter,
                                t,
                                c + Vec2::new(0.0, core * down),
                                core * 1.3 * grow,
                                Color32::from_black_alpha(a(255.0 * alpha, dim)),
                            );
                        }
                    }
                    draw_texture(&painter, t, c, core * 1.3, tint);
                } else {
                    // Nothing to show in the center (no app icon, and no active mode or parent
                    // submenu icon): draw the CursorPie mark instead of leaving the disc empty. If
                    // the artwork cannot be loaded, the center is drawn as before.
                    let logo = view.center_icon.trim().is_empty()
                        && crate::ui::settings::brand::paint(
                            &painter,
                            crate::ui::settings::brand::Art::Mark,
                            egui::Rect::from_center_size(c, Vec2::splat(core * 1.2)),
                            colors
                                .ink
                                .gamma_multiply(fade * if live { 0.75 } else { 0.4 }),
                        );
                    if !logo {
                        draw_icon(
                            &painter,
                            center_tex.as_ref(),
                            &view.center_icon,
                            c,
                            core * 1.0,
                            colors
                                .ink
                                .gamma_multiply(fade * if live { 0.6 } else { 0.3 }),
                        );
                    }
                }
                if self.pie_open
                    && !previewing
                    && self.hover.is_none()
                    && top_level
                    && view.settle_ms > 0.0
                {
                    let prog = (self.opened_at.elapsed().as_millis() as f32 / view.settle_ms)
                        .clamp(0.0, 1.0);
                    if prog > 0.02 {
                        let pts = arc_points(c, core + 3.0, -PI / 2.0, -PI / 2.0 + TAU * prog);
                        painter.add(Shape::line(pts, Stroke::new(3.0, accent)));
                    }
                }
            });
    }
}

/// Which of the two badges of a slice: index into the animation state.
fn badge_ix(choice: Choice) -> usize {
    match choice {
        Choice::NewWindow => 0,
        Choice::Existing => 1,
    }
}

/// How one badge of an "Open app" slice is moving at this moment.
#[derive(Clone, Copy)]
struct BadgeMotion {
    /// Size while it comes out: below 1 at first, a little above 1 at the end of the overshoot.
    grow: f32,
    /// Opacity, 0 to 1.
    alpha: f32,
    /// 0 to 1: how much the pointer is on this badge (larger, with a bright ring).
    pick: f32,
    /// 0 to 1: how much the pointer is on the other badge (smaller and paler).
    step: f32,
}

/// What the badges take from the pie they sit next to, so they follow its theme.
struct BadgeStyle {
    /// The accent of the pie (sage by default): the new-window badge is this color.
    accent: Color32,
    /// The colors of the pie in the appearance it is drawn in.
    colors: PieColors,
    /// The glass settings while the pie is glass, `None` while it is solid.
    glass: Option<glass::Glass>,
    /// Whether the pie is Gen Z (white, thick outlines, hard shadows).
    genz: bool,
    /// Whether the pie is smoked graphite (dark appearance, or glass).
    smoked: bool,
}

/// The color of the badge of `choice`: the accent for a new window (lifted a little on a smoked pie,
/// where the accent is dark against the graphite), sage-gold for the app that is open.
fn badge_tint(choice: Choice, style: &BadgeStyle) -> Color32 {
    match choice {
        Choice::NewWindow if style.smoked => {
            let a = [style.accent.r(), style.accent.g(), style.accent.b()];
            rgb(mix(a, [255, 255, 255], 0.14))
        }
        Choice::NewWindow => style.accent,
        Choice::Existing if style.smoked => OPEN_APP_GOLD_SMOKED,
        Choice::Existing => OPEN_APP_GOLD,
    }
}

/// Draws one round badge of an "Open app" slice centered on `at`, `size` points wide: sage with a +
/// for a new window, sage-gold with a small square for the app that is open. It is drawn in the
/// material of the pie: an opaque disc with a hairline edge on a solid pie, a lit glass disc (the
/// same as the center of the pie) on a glass one, an outlined disc with a hard shadow on a Gen Z
/// one. `motion` says how it is coming out or being picked.
fn draw_open_badge(
    painter: &egui::Painter,
    at: egui::Pos2,
    size: f32,
    choice: Choice,
    motion: BadgeMotion,
    style: &BadgeStyle,
) {
    let scale = motion.grow.max(0.0) * (1.0 + 0.18 * motion.pick - 0.08 * motion.step);
    let alpha = motion.alpha.clamp(0.0, 1.0) * (1.0 - 0.3 * motion.step);
    let radius = size * scale / 2.0;
    let tint = badge_tint(choice, style);

    if !style.genz {
        // the same soft shadow the pie has, a little smaller
        let shadow = Color32::from_black_alpha(
            ((if style.glass.is_some() { 30.0 } else { 46.0 }) * alpha) as u8,
        );
        painter.circle_filled(at + Vec2::new(0.0, 1.5), radius + 1.0, shadow);
    }

    if let Some(g) = style.glass {
        glass::core(painter, at, radius, alpha, Some(tint), g);
    } else if style.genz {
        genz::core(painter, at, radius, alpha, tint);
    } else {
        painter.circle_filled(at, radius, tint.gamma_multiply(alpha));
        // the hairline the slices have, so the edge reads on any background
        let edge = if style.smoked {
            Color32::from_white_alpha((46.0 * alpha) as u8)
        } else {
            Color32::from_black_alpha((34.0 * alpha) as u8)
        };
        painter.circle_stroke(at, radius, Stroke::new(1.0, edge));
    }
    if motion.pick > 0.02 {
        // the badge the pointer is on gets a ring in the ink of the pie, which opens around it
        painter.circle_stroke(
            at,
            radius + 2.0 * motion.pick,
            Stroke::new(1.75, style.colors.ink.gamma_multiply(alpha * motion.pick)),
        );
    }

    // white on a smoked pie (dark appearance or glass), where it reads best; on the light solid pie
    // the readable color of the badge
    let glyph_color = if style.smoked {
        Color32::WHITE
    } else {
        match choice {
            Choice::NewWindow => on_accent(tint),
            Choice::Existing => OPEN_APP_GOLD_INK,
        }
    };
    let glyph = glyph_color.gamma_multiply(alpha);
    match choice {
        Choice::NewWindow => {
            // a +
            let arm = radius * 0.5;
            let plus = Stroke::new((radius * 0.24).max(1.8), glyph);
            painter.line_segment([at - Vec2::new(arm, 0.0), at + Vec2::new(arm, 0.0)], plus);
            painter.line_segment([at - Vec2::new(0.0, arm), at + Vec2::new(0.0, arm)], plus);
        }
        Choice::Existing => {
            // a small square inside the circle
            let rect = egui::Rect::from_center_size(at, Vec2::splat(radius * 0.9));
            painter.rect_filled(rect, (radius * 0.14).max(1.0), glyph);
        }
    }
}

/// How strong the shadow under the app's icon is: 0 for none, 1 for the soft one. `light_solid` is
/// whether the center is a light, solid disc, which is where the icon needs it.
pub(crate) fn icon_shadow_weight(mode: crate::config::IconShadow, light_solid: bool) -> f32 {
    use crate::config::IconShadow;
    match mode {
        IconShadow::Off => 0.0,
        IconShadow::Soft => 1.0,
        IconShadow::Strong => 1.9,
        IconShadow::Auto if light_solid => 1.0,
        IconShadow::Auto => 0.0,
    }
}

#[cfg(test)]
mod shadow_tests {
    use super::*;
    use crate::config::IconShadow;

    #[test]
    fn the_shadow_follows_the_setting() {
        assert_eq!(icon_shadow_weight(IconShadow::Off, true), 0.0);
        assert_eq!(icon_shadow_weight(IconShadow::Auto, true), 1.0);
        assert_eq!(icon_shadow_weight(IconShadow::Auto, false), 0.0);
        assert_eq!(icon_shadow_weight(IconShadow::Soft, false), 1.0);
        assert!(icon_shadow_weight(IconShadow::Strong, false) > 1.0);
    }
}
