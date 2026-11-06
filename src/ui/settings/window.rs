//! The settings window itself: open/close animation, window chrome, and its layout:
//!
//! ```text
//! +--------------+-------------------------------------------------------+
//! | logo  Cursor | Page title                              (o)(o)(o) |  top bar
//! |       Pie    | one line about the page                                |
//! |              +-------------------------------------------------------+
//! |  Menu        |                                                       |
//! |  Behavior    |   page of the selected section                        |
//! |  General     |                                                       |
//! |  About       |                                                       |
//! |              +-------------------------------------------------------+
//! |  v0.6  Quit  | status or hint                                        |  footer
//! +--------------+-------------------------------------------------------+
//! ```
//!
//! The sidebar holds the brand and names the sections (like the settings of the system apps), so
//! it runs the full height of the window. The top bar holds everything you confirm with, so those
//! buttons never move. The four areas come from [`regions`], a pure function with its own tests.

use super::state::Tab;
use super::tabs::about::about_tab;
use crate::about::*;
use crate::config::backups;
use crate::platform;
use crate::tap::TAP_ACTIVE;
use crate::ui::app::App;
use crate::ui::draw::*;
use crate::ui::fonts;
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use eframe::egui::{self, RichText, Vec2};
use std::sync::atomic::Ordering;
use std::time::Duration;

/// Height of the top bar (page title, Undo / Redo / Cancel / Save).
const TOP_H: f32 = 76.0;
/// Height of the footer (status line).
const FOOT_H: f32 = 44.0;
/// Width of the sidebar that lists the sections.
const SIDE_W: f32 = 232.0;
/// Width of the sidebar when it is folded to icons.
const RAIL_W: f32 = 76.0;
/// Widest a page of panels grows. Wider than this, sliders and text lines become too long to
/// follow, so the page keeps this width and sits in the middle of the content area.
const PAGE_W: f32 = 820.0;
/// Whether the sidebar is folded to icons. It starts folded (compact is the default look), and is
/// remembered while the app runs, not saved: it is a way of looking at the window, not a setting.
static COMPACT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
/// Space between the edge of the window and the page.
const PAD: f32 = GAP * 2.0 + 4.0;

/// The four areas of the window, as rectangles in screen points.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Regions {
    top: egui::Rect,
    side: egui::Rect,
    body: egui::Rect,
    foot: egui::Rect,
}

/// Cuts the `card` rectangle into the sidebar (`side_w` wide, the full height, on the left), and,
/// in the column to its right, the top bar, the body and the footer. The body is inset by [`PAD`]
/// at the sides.
fn regions(card: egui::Rect, side_w: f32) -> Regions {
    let side = egui::Rect::from_min_size(card.min, Vec2::new(side_w, card.height()));
    let main_left = side.max.x;
    let top = egui::Rect::from_min_max(
        egui::pos2(main_left, card.min.y),
        egui::pos2(card.max.x, card.min.y + TOP_H),
    );
    let foot = egui::Rect::from_min_max(egui::pos2(main_left, card.max.y - FOOT_H), card.max);
    let body = egui::Rect::from_min_max(
        egui::pos2(main_left + PAD, top.max.y),
        egui::pos2(card.max.x - PAD, foot.min.y),
    );
    Regions {
        top,
        side,
        body,
        foot,
    }
}

/// Size of the hit area and radius of the small round window-style buttons in the top bar.
const DOT_HIT: f32 = 20.0;
const DOT_R: f32 = 6.5;

/// The unsaved-changes dot: a muted ochre that sits with the sage palette (lighter in dark mode).
fn amber() -> egui::Color32 {
    if pal().dark {
        egui::Color32::from_rgb(232, 198, 112)
    } else {
        egui::Color32::from_rgb(208, 160, 56)
    }
}

/// A small round colored button (like the close / zoom dots of a window). Its glyph shows while
/// the pointer is over it.
fn dot_button(ui: &mut egui::Ui, color: egui::Color32, icon: char, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(DOT_HIT), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
    let fill = if resp.is_pointer_button_down_on() {
        color.gamma_multiply(0.8)
    } else {
        color
    };
    ui.painter().circle_filled(rect.center(), DOT_R, fill);
    if resp.hovered() {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            icon,
            egui::FontId::new(9.0, fonts::icons()),
            egui::Color32::from_black_alpha(170),
        );
    }
    resp.on_hover_text(tip)
}

/// What the top bar and sidebar buttons asked for this frame.
enum UiAction {
    None,
    Save,
    Cancel,
    Quit,
}

/// The icon, title and one-line description of a section.
fn section_info(tab: Tab) -> (char, &'static str, &'static str) {
    match tab {
        Tab::Menu => (
            symbols::CHART_PIE_SLICE,
            "Menu",
            "Profiles, slices and what each one does.",
        ),
        Tab::Behavior => (
            symbols::SLIDERS_HORIZONTAL,
            "Behavior",
            "How the pie opens, how it looks and which pie each app gets.",
        ),
        Tab::General => (
            symbols::GEAR_SIX,
            "General",
            "Input status, your mouse, your configuration and its backups.",
        ),
        Tab::About => (symbols::INFO, "About", "Version, credits and links."),
    }
}

impl App {
    /// Draws the Settings window (animation, sidebar, top bar, body, footer) and applies Save /
    /// Cancel / Quit.
    pub fn settings_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();

        // the window is still becoming the card: an empty frame, the animation waits
        if self.window_settling(&ctx) {
            return;
        }

        // open / close animation
        let dt = ctx.input(|i| i.stable_dt).min(0.05);
        let target = if self.closing.is_some() { 0.0 } else { 1.0 };
        if smooth_approach(&mut self.settings_anim, target, dt, 14.0) {
            ctx.request_repaint();
        }
        if let Some(save) = self.closing
            && self.settings_anim <= 0.02
        {
            self.close_settings(&ctx, save);
            return;
        }
        // keep permission status fresh while the window is open
        ctx.request_repaint_after(Duration::from_millis(1000));

        let t = self.settings_anim.clamp(0.0, 1.0);
        let slide = (1.0 - t) * (1.0 - t) * 26.0;

        // Esc closes what is on top: a sheet, then a popup (egui closes it itself), then the window
        // (when not recording) = cancel
        if self.closing.is_none() && self.ed.recording.is_none() && !super::picker::choosing() {
            let esc = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            if esc {
                if self.confirm_quit {
                    self.confirm_quit = false; // "keep editing"
                } else if self.confirm_discard {
                    self.confirm_discard = false; // "keep editing"
                } else if !self.ed.close_sheet() && !egui::Popup::is_any_open(&ctx) {
                    self.request_cancel();
                }
            }
        }
        // Ctrl+S / Cmd+S saves and keeps Settings open (the green dot saves and closes)
        if self.closing.is_none()
            && self.ed.recording.is_none()
            && !self.confirm_quit
            && !self.confirm_discard
            && !super::picker::choosing()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S))
        {
            self.persist_edit();
            self.opened = Some(self.edit.clone()); // nothing is unsaved any more
            self.say("Saved.");
            ctx.request_repaint();
        }
        self.history_keys(&ctx);
        if self
            .toast
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(6))
        {
            self.toast = None;
        }

        let accent = rgb(self.edit.accent);
        style_settings(ui, accent);
        ui.set_opacity(t);

        // the sidebar folds to icons (the button in the top bar, or Ctrl / Cmd + B)
        let fold = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::B);
        if self.ed.recording.is_none() && ctx.input_mut(|i| i.consume_shortcut(&fold)) {
            COMPACT.fetch_xor(true, Ordering::Relaxed);
        }
        let compact = COMPACT.load(Ordering::Relaxed);
        let open = ctx.animate_bool_with_time(egui::Id::new("sidebar_open"), !compact, 0.16);
        let side_w = RAIL_W + (SIDE_W - RAIL_W) * open;
        let wide = open > 0.5;

        // The window, always centered. Its rectangle is snapped to whole pixels: a hairline that
        // sits between two pixels is smeared over both, which is what made the old card look soft.
        let full = ui.max_rect();
        let card_rect = snap_rect(
            egui::Rect::from_center_size(full.center(), self.card_size)
                .translate(Vec2::new(0.0, slide)),
            ctx.pixels_per_point(),
        );
        self.ed.card = card_rect;
        let Regions {
            top,
            side,
            body,
            foot,
        } = regions(card_rect, side_w);
        let painter = ui.painter().clone();
        painter.add(window_shadow().as_shape(card_rect, CARD_RADIUS));
        painter.rect_filled(card_rect, CARD_RADIUS, pal().bg);
        painter.rect_filled(
            side,
            egui::CornerRadius {
                nw: CARD_RADIUS_PT,
                sw: CARD_RADIUS_PT,
                ne: 0,
                se: 0,
            },
            pal().sidebar,
        );
        painter.line_segment(
            [side.right_top(), side.right_bottom()],
            egui::Stroke::new(1.0, pal().border),
        );
        window_outline(&painter, card_rect, accent);

        // dragging the empty part of the top bar or the sidebar's head moves the window; the
        // buttons are added after it, so they sit on top and keep their clicks
        let drag_area = egui::Rect::from_min_max(side.min, egui::pos2(top.max.x, top.max.y));
        let drag = ui.interact(
            drag_area,
            egui::Id::new("settings_drag"),
            egui::Sense::click_and_drag(),
        );
        if drag.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }

        let mut action = UiAction::None;
        let unsaved = self.has_unsaved_changes();
        let primary = platform::OS.primary_name();

        // ---------------------------------------------------------------- sidebar
        let brand_y = side.min.y + 30.0;
        if wide {
            // the logo already has the name written in it
            draw_wordmark(&painter, side.min.x + PAD, brand_y + 14.0, 30.0);
        } else {
            draw_logo(&painter, egui::pos2(side.center().x, brand_y + 14.0), 14.0);
        }
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
                egui::pos2(side.min.x + GAP + 4.0, side.min.y + 92.0),
                egui::pos2(side.max.x - GAP - 4.0, side.max.y - 90.0),
            )),
            |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for tab in [Tab::Menu, Tab::Behavior, Tab::General, Tab::About] {
                    let (icon, title, _) = section_info(tab);
                    if nav_item(ui, icon, title, self.ed.tab == tab, accent, !wide).clicked() {
                        self.ed.tab = tab;
                        self.ed.scan = None; // a scan belongs to the General tab
                        self.ed.close_sheet();
                        if tab == Tab::General {
                            self.ed.backups = backups(); // read once, not on every frame
                        }
                    }
                }
            },
        );
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(side.shrink2(Vec2::new(GAP + 4.0, GAP + 4.0)))
                .layout(egui::Layout::bottom_up(egui::Align::Min)),
            |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                if wide {
                    if button(ui, Look::Danger, Some(symbols::POWER), "Quit CursorPie").clicked() {
                        action = UiAction::Quit;
                    }
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(format!("Version {VERSION}"))
                                .size(12.0)
                                .color(pal().muted),
                        );
                    });
                } else {
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                        let quit = icon_button_look(
                            ui,
                            Look::Danger,
                            symbols::POWER,
                            "Quit CursorPie",
                            true,
                        );
                        if quit.clicked() {
                            action = UiAction::Quit;
                        }
                    });
                }
            },
        );

        // ---------------------------------------------------------------- top bar
        let (_, title, subtitle) = section_info(self.ed.tab);
        let title_x = top.min.x + PAD + 44.0;
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::pos2(top.min.x + PAD, top.min.y + 8.0),
                Vec2::splat(CONTROL_H),
            )),
            |ui| {
                let tip = if compact {
                    format!("Show the sidebar ({primary}+B)")
                } else {
                    format!("Fold the sidebar to icons ({primary}+B)")
                };
                if icon_button(ui, symbols::SIDEBAR_SIMPLE, &tip, true).clicked() {
                    COMPACT.fetch_xor(true, Ordering::Relaxed);
                }
            },
        );
        painter.text(
            egui::pos2(title_x, top.min.y + 26.0),
            egui::Align2::LEFT_CENTER,
            title,
            egui::FontId::new(24.0, fonts::semibold()),
            pal().text,
        );
        painter.text(
            egui::pos2(title_x, top.min.y + 54.0),
            egui::Align2::LEFT_CENTER,
            subtitle,
            egui::FontId::proportional(13.0),
            pal().muted,
        );
        // Right side, added from the right edge inwards. Save is the primary (accent) button at
        // the corner.
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_min_max(
                    egui::pos2(top.min.x + 300.0, top.min.y),
                    egui::pos2(top.max.x - PAD, top.min.y + 52.0),
                ))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                // window-style dots, left to right: red (cancel), yellow (unsaved changes), green (save)
                let save = dot_button(ui, accent, symbols::CHECK, "Save and close");
                if save.clicked() {
                    action = UiAction::Save;
                }
                if unsaved {
                    let (rect, resp) =
                        ui.allocate_exact_size(Vec2::splat(DOT_HIT), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), DOT_R, amber());
                    resp.on_hover_text("Unsaved changes");
                }
                if dot_button(ui, pal().bad, symbols::X, "Cancel (Esc)").clicked() {
                    action = UiAction::Cancel;
                }
            },
        );

        // ---------------------------------------------------------------- body
        ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(body));
            if self.ed.tab == Tab::Menu {
                // Menu: two tiles, no outer scroll
                self.menu_tab(ui, &ctx, body.width());
            } else {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // the page keeps a readable width and sits in the middle of the body
                        let room = ui.available_rect_before_wrap();
                        let width = PAGE_W.min(room.width());
                        let page = egui::Rect::from_min_size(
                            egui::pos2(room.min.x + (room.width() - width) / 2.0, room.min.y),
                            Vec2::new(width, room.height()),
                        );
                        ui.scope_builder(egui::UiBuilder::new().max_rect(page), |ui| {
                            ui.add_space(4.0);
                            match self.ed.tab {
                                Tab::Behavior => self.behavior_tab(ui),
                                Tab::General => self.general_tab(ui),
                                Tab::About => about_tab(ui, accent),
                                Tab::Menu => {}
                            }
                        });
                    });
            }
        });

        // ---------------------------------------------------------------- footer
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_min_max(
                    egui::pos2(foot.min.x + PAD, foot.min.y),
                    egui::pos2(foot.max.x - PAD, foot.max.y),
                ))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                // one line of small text in every case, so the footer stays calm
                let hint = |text: &str| RichText::new(text).size(12.0).color(pal().muted);
                if !platform::is_trusted_cached() || !TAP_ACTIVE.load(Ordering::SeqCst) {
                    let problem = if platform::OS.needs_permission() {
                        "Accessibility permission needed"
                    } else {
                        "Mouse hook not running"
                    };
                    ui.label(RichText::new(problem).size(12.0).color(pal().bad));
                    if platform::OS.needs_permission() {
                        ui.label(hint("\u{b7}"));
                        if link(ui, "Open System Settings", accent).clicked() {
                            platform::open_accessibility_settings();
                        }
                    }
                } else if let Some((msg, _)) = &self.toast {
                    ui.label(hint(msg));
                } else {
                    ui.label(hint(&format!(
                        "Esc cancels  \u{b7}  {primary}+Z undoes  \u{b7}  {primary}+S saves"
                    )));
                }
            },
        );

        // a native file chooser is open beside the window: nothing here can be used meanwhile
        if super::picker::choosing() {
            super::picker::waiting_scrim(&ctx, card_rect);
        }

        // the sheet that asks again before quitting with unsaved changes (over everything else)
        self.quit_dialog(&ctx, accent);
        // the sheet that asks before Cancel / Esc throws unsaved changes away
        self.discard_dialog(&ctx);

        match action {
            UiAction::Save => {
                self.ed.stop_recording();
                self.closing = Some(true);
            }
            UiAction::Cancel => {
                self.ed.stop_recording();
                self.request_cancel();
            }
            UiAction::Quit => self.request_quit(&ctx),
            UiAction::None => {}
        }
        self.history_observe(&ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::app::{CARD_MIN_H, CARD_MIN_W, CARD_SIZE, WIN_MARGIN};

    fn card(w: f32, h: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(WIN_MARGIN, WIN_MARGIN), Vec2::new(w, h))
    }

    #[test]
    fn areas_tile_the_window_without_overlap() {
        let c = card(CARD_SIZE[0], CARD_SIZE[1]);
        let r = regions(c, SIDE_W);
        assert_eq!(
            r.side.min, c.min,
            "the sidebar starts at the window's corner"
        );
        assert_eq!(
            r.side.height(),
            c.height(),
            "the sidebar is as high as the window"
        );
        assert_eq!(r.side.width(), SIDE_W);
        assert_eq!(r.top.min.y, c.min.y, "the bar is at the top");
        assert_eq!(
            r.top.min.x, r.side.max.x,
            "the bar starts where the sidebar ends"
        );
        assert_eq!(r.top.max.x, c.max.x);
        assert_eq!(r.foot.max, c.max, "the footer ends at the window's corner");
        assert_eq!(r.foot.min.x, r.side.max.x);
        assert_eq!(r.top.height(), TOP_H);
        assert_eq!(r.foot.height(), FOOT_H);
        assert!(
            r.side.max.x + PAD <= r.body.min.x + 1e-3,
            "body clears the sidebar"
        );
        assert!(r.top.max.y <= r.body.min.y + 1e-3, "body clears the bar");
        assert!(
            r.body.max.y <= r.foot.min.y + 1e-3,
            "body clears the footer"
        );
        assert!(c.contains_rect(r.body));
        assert!(c.contains_rect(r.side));
    }

    #[test]
    fn a_folded_sidebar_gives_its_room_to_the_body() {
        let c = card(CARD_SIZE[0], CARD_SIZE[1]);
        let (open, folded) = (regions(c, SIDE_W), regions(c, RAIL_W));
        assert_eq!(folded.side.width(), RAIL_W);
        assert_eq!(folded.body.width() - open.body.width(), SIDE_W - RAIL_W);
        const {
            assert!(
                RAIL_W >= CONTROL_H + 2.0 * GAP,
                "an icon button fits the rail"
            );
        }
    }

    #[test]
    fn the_page_fits_the_body_of_the_full_card() {
        let r = regions(card(CARD_SIZE[0], CARD_SIZE[1]), SIDE_W);
        assert!(
            r.body.width() >= PAGE_W,
            "the page is not squeezed on a full card"
        );
    }

    #[test]
    fn the_body_is_inset_by_the_padding() {
        let c = card(1180.0, 740.0);
        let r = regions(c, SIDE_W);
        assert_eq!(r.body.min.x - r.side.max.x, PAD);
        assert_eq!(c.max.x - r.body.max.x, PAD);
        assert_eq!(r.body.min.y, r.top.max.y);
    }

    #[test]
    fn the_body_stays_usable_on_a_small_display() {
        // `card_size_for` never goes below this card
        let r = regions(card(CARD_MIN_W, CARD_MIN_H), SIDE_W);
        assert!(r.body.height() >= 300.0, "body: {}", r.body.height());
        // the Menu tab needs a 320 point pie tile and an editor of at least 360 points beside it
        assert!(
            r.body.width() >= 320.0 + GAP + 360.0,
            "body: {}",
            r.body.width()
        );
    }

    #[test]
    fn the_full_card_gives_the_editor_as_much_room_as_before_the_sidebar() {
        // the editor keeps the width it had before the sidebar grew
        let r = regions(card(CARD_SIZE[0], CARD_SIZE[1]), SIDE_W);
        assert!(r.body.width() - 320.0 - GAP >= 560.0);
    }

    #[test]
    fn the_areas_follow_the_card_when_it_moves() {
        let a = regions(card(900.0, 600.0), SIDE_W);
        let moved = regions(card(900.0, 600.0).translate(Vec2::new(0.0, 13.0)), SIDE_W);
        assert_eq!(moved.top.min.y - a.top.min.y, 13.0);
        assert_eq!(moved.side.min.y - a.side.min.y, 13.0);
        assert_eq!(moved.foot.max.y - a.foot.max.y, 13.0);
        assert_eq!(moved.body.height(), a.body.height());
    }

    /// Draws the whole window for `frames` frames on a headless context.
    fn draw(app: &mut App, ctx: &egui::Context, frames: usize) {
        for _ in 0..frames {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(
                        CARD_SIZE[0] + 2.0 * WIN_MARGIN,
                        CARD_SIZE[1] + 2.0 * WIN_MARGIN,
                    ),
                )),
                ..Default::default()
            };
            // nothing renders the output, so the texture uploads are dropped on purpose
            ctx.run_ui(input, |ui| app.settings_ui(ui))
                .drop_without_applying_deltas();
        }
    }

    #[test]
    fn every_section_draws_without_panicking() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = App::new(rx, tx, 200.0, None);
        app.settings_open = true;
        app.edit = crate::config::Config::default();
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        for tab in [Tab::Menu, Tab::Behavior, Tab::General, Tab::About] {
            app.ed.tab = tab;
            draw(&mut app, &ctx, 3);
        }
        assert!(
            app.ed.tab == Tab::About,
            "drawing never changes the section"
        );
    }
}
