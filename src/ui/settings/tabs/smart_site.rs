//! The sheet "Choose a site" of the Intelligent page: the list of sites CursorPie knows, with their
//! icons and a search box, for the sites the intelligent pie never learns.
//!
//! It is the same kind of sheet as the list of installed apps ([`profile_toolbar`]): a field on top,
//! rows with an icon below, one click answers. The field also takes an address that is not in the
//! list (`bank.example`, or a whole `https://www.bank.example/login`: only the site is kept).
//!
//! [`profile_toolbar`]: super::profile_toolbar

use crate::config::{Config, SMART_IGNORE_MAX};
use crate::smart::siteicon;
use crate::ui::fonts;
use crate::ui::settings::state::{Ed, ProfileSheet};
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use crate::ui::webicon;
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};

/// Row height of the list, the same as the list of apps.
const ROW_H: f32 = 48.0;

/// Opens the sheet and asks its field for the keyboard.
pub(in crate::ui::settings) fn open_site_sheet(ctx: &egui::Context, ed: &mut Ed) {
    ed.smart_new_site.clear();
    ed.profile_sheet = Some(ProfileSheet::Site);
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("site_sheet_focus"), true));
}

/// A site as it is stored: lower case, no `www.`.
pub(in crate::ui::settings) fn clean(host: &str) -> String {
    let host = host.trim().to_ascii_lowercase();
    host.strip_prefix("www.").unwrap_or(&host).to_string()
}

/// The site a typed text names (`bank.example`, `https://www.bank.example/login`), if it names one.
fn typed_site(text: &str) -> Option<String> {
    webicon::parse_site(text).ok().map(|s| clean(&s.host))
}

/// Whether `host` is on the list already.
fn listed(edit: &Config, host: &str) -> bool {
    edit.smart
        .ignore_sites
        .iter()
        .any(|s| s.eq_ignore_ascii_case(host))
}

/// Draws the sheet while it is open, and adds the site that is chosen to the ignore list.
pub(in crate::ui::settings) fn site_sheet(
    ctx: &egui::Context,
    edit: &mut Config,
    ed: &mut Ed,
    accent: Color32,
) {
    if ed.profile_sheet != Some(ProfileSheet::Site) {
        return;
    }
    let known = crate::smart::known_sites();
    let size = Vec2::new(520.0, 580.0);
    let bounds = ed.card;
    let mut chosen: Option<String> = None;
    let mut cancel = false;
    let (floating, ()) = sheet(ctx, "pick_site", bounds, size, "Choose a site", |ui| {
        let field = ui.add(
            text_field(&mut ed.smart_new_site)
                .desired_width(ui.available_width())
                .hint_text("Search, or type an address such as bank.example"),
        );
        let focus = ctx
            .data_mut(|d| d.remove_temp::<bool>(egui::Id::new("site_sheet_focus")))
            .unwrap_or(false);
        if focus {
            field.request_focus();
        }
        ui.add_space(GAP);

        let query = ed.smart_new_site.trim().to_ascii_lowercase();
        let typed = typed_site(&query);
        let mut rows: Vec<String> = Vec::new();
        // a typed address that the list does not have comes first, as the one to add
        if let Some(t) = &typed
            && !known.contains(t)
        {
            rows.push(t.clone());
        }
        rows.extend(
            known
                .iter()
                .filter(|h| query.is_empty() || h.contains(&query))
                .cloned(),
        );
        // Enter takes the first row that can be taken
        if field.lost_focus()
            && ctx.input(|i| i.key_pressed(egui::Key::Enter))
            && let Some(first) = rows.iter().find(|h| !listed(edit, h))
        {
            chosen = Some(first.clone());
        }

        let list_h = (ui.available_height() - 54.0).max(120.0);
        if rows.is_empty() {
            ui.allocate_ui(Vec2::new(ui.available_width(), list_h), |ui| {
                ui.centered_and_justified(|ui| {
                    muted(
                        ui,
                        "CursorPie has not seen a site yet. Type the address of one above.",
                    );
                });
            });
        } else {
            egui::ScrollArea::vertical()
                .max_height(list_h)
                .auto_shrink([false, false])
                .show_rows(ui, ROW_H, rows.len(), |ui, range| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for k in range {
                        let host = &rows[k];
                        let taken = listed(edit, host);
                        let (row, resp) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), ROW_H),
                            if taken {
                                egui::Sense::hover()
                            } else {
                                egui::Sense::click()
                            },
                        );
                        let inner = row.shrink2(Vec2::new(0.0, 2.0));
                        if resp.hovered() && !taken {
                            ui.painter().rect_filled(inner, control_r(), pal().idle);
                        }
                        let icon = Rect::from_center_size(
                            Pos2::new(inner.min.x + 12.0 + 16.0, inner.center().y),
                            Vec2::splat(32.0),
                        );
                        // its own icon when it was fetched, otherwise the globe
                        let tex = siteicon::icon_for(host).and_then(|i| ed.icons.get(ctx, &i));
                        if let Some(tex) = tex {
                            let whole = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                            let tint = if taken {
                                Color32::from_white_alpha(110)
                            } else {
                                Color32::WHITE
                            };
                            ui.painter().image(tex.id(), icon, whole, tint);
                        } else {
                            fonts::paint_icon(
                                ui.painter(),
                                icon.center(),
                                symbols::GLOBE,
                                24.0,
                                pal().muted,
                                false,
                            );
                        }
                        let new = typed.as_deref() == Some(host.as_str()) && !known.contains(host);
                        ui.painter().text(
                            Pos2::new(inner.min.x + 64.0, inner.center().y),
                            egui::Align2::LEFT_CENTER,
                            if new {
                                format!("Add {host}")
                            } else {
                                host.clone()
                            },
                            egui::FontId::new(14.0, fonts::semibold()),
                            if taken { pal().muted } else { pal().text },
                        );
                        if taken {
                            fonts::paint_icon(
                                ui.painter(),
                                Pos2::new(inner.max.x - 20.0, inner.center().y),
                                symbols::CHECK,
                                16.0,
                                accent_text(accent),
                                false,
                            );
                        } else if resp
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            chosen = Some(host.clone());
                        }
                    }
                });
        }

        ui.add_space((ui.available_height() - CONTROL_H).max(GAP));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if button(ui, Look::Soft, None, "Back").clicked() {
                cancel = true;
            }
        });
    });

    if let Some(host) = chosen {
        let list = &mut edit.smart.ignore_sites;
        if list.len() < SMART_IGNORE_MAX && !list.iter().any(|s| s.eq_ignore_ascii_case(&host)) {
            list.push(host);
        }
        ed.profile_sheet = None;
        ed.smart_new_site.clear();
    } else if cancel || floating.close {
        ed.profile_sheet = None;
        ed.smart_new_site.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_address_is_reduced_to_its_site() {
        assert_eq!(
            typed_site("https://www.Bank.example/login?x=1").as_deref(),
            Some("bank.example")
        );
        assert_eq!(typed_site("bank.example").as_deref(), Some("bank.example"));
        assert_eq!(typed_site("bank"), None, "a site has a dot in it");
        assert_eq!(typed_site(""), None);
    }

    #[test]
    fn the_sheet_draws_and_answers_without_panicking() {
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        let mut edit = Config::default();
        let mut ed = Ed::new();
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1180.0, 740.0));
        ed.card = screen;
        open_site_sheet(&ctx, &mut ed);
        ed.smart_new_site = "bank.example".into();
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                let ctx = ui.ctx().clone();
                site_sheet(&ctx, &mut edit, &mut ed, Color32::from_rgb(90, 120, 94));
            });
            out.textures_delta.clear();
        }
        assert_eq!(ed.profile_sheet, Some(ProfileSheet::Site));
        assert!(edit.smart.ignore_sites.is_empty(), "nothing was clicked");
    }
}
