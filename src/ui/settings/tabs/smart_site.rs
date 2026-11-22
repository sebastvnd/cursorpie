//! The sheet "Choose a site" of the Intelligent page: the list of sites CursorPie knows, with their
//! icons and a search box, for the sites the intelligent pie never learns.
//!
//! It is the same kind of sheet as the list of installed apps ([`profile_toolbar`]): a field on top,
//! rows with an icon below, one click answers. The field also takes an address that is not in the
//! list (`bank.example`, or a whole `https://www.bank.example/login`: only the site is kept), and
//! has an **Add** button for it.
//!
//! The same sheet also answers the **Pick** button of a "URL / file" slice and **Add site** of an
//! Intelligent slice ([`site_pick_sheet`]). There the list starts with the pages that are open in
//! the browser now (when the browser extension is connected), then the sites CursorPie has seen.
//!
//! [`profile_toolbar`]: super::profile_toolbar

use crate::config::{Config, SMART_IGNORE_MAX};
use crate::msg::Msg;
use crate::smart::siteicon;
use crate::ui::fonts;
use crate::ui::settings::state::{Ed, ProfileSheet, SiteFor};
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use crate::ui::webicon;
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use std::sync::mpsc::Sender;

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

/// The address a typed text names, as it is opened: what was typed when it has `https://` (or
/// `http://`) in front, otherwise with `https://` added.
fn typed_address(text: &str) -> Option<String> {
    webicon::parse_site(text).ok()?;
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    Some(
        if lower.starts_with("https://") || lower.starts_with("http://") {
            t.to_string()
        } else {
            format!("https://{t}")
        },
    )
}

/// What a typed text opens in a "URL / file" slice: a web address as `typed_address` reads it,
/// `http://` in front of `localhost:3000` or an IP with a port, and anything else as it was typed (a
/// file path, `mailto:...`, another scheme). `None` only when nothing is typed.
fn typed_target(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let lower = t.to_ascii_lowercase();
    // `localhost:3000` and an IP address are served without TLS far more often than with it
    let local = lower.split(['/', '?', '#']).next().unwrap_or("");
    let host = local.split(':').next().unwrap_or("");
    let is_ip = host.split('.').count() == 4 && host.split('.').all(|p| p.parse::<u8>().is_ok());
    if !lower.contains("://") && !t.contains(char::is_whitespace) && (host == "localhost" || is_ip)
    {
        return Some(format!("http://{t}"));
    }
    // `mailto:me@bank.example`, `tel:...`: a scheme without `//` is not a host
    if let Some((scheme, rest)) = t.split_once(':')
        && scheme.len() >= 2
        && scheme.chars().all(|c| c.is_ascii_alphabetic())
        && !rest.starts_with(|c: char| c.is_ascii_digit() || c == '/')
    {
        return Some(t.to_string());
    }
    Some(typed_address(t).unwrap_or_else(|| t.to_string()))
}

/// What the **Add** button answers for the typed text.
fn typed_pick(text: &str, keep_address: bool, any_text: bool) -> Option<String> {
    if keep_address {
        if any_text {
            typed_target(text)
        } else {
            typed_address(text)
        }
    } else {
        typed_site(text)
    }
}

/// One line of the list.
struct SiteRow {
    /// The site, `github.com`.
    host: String,
    /// What a click answers: the address of the page (an open tab), or `https://host`.
    address: String,
    /// What the line says: the site, or the page of an open tab without `https://www.`.
    label: String,
    /// Whether the page is open in the browser now.
    open: bool,
    /// Whether it is the typed address, which the list does not have.
    new: bool,
}

/// How an address reads in the list: no scheme, no `www.`, no closing slash.
fn plain_address(url: &str) -> String {
    let u = url.trim();
    let u = u
        .strip_prefix("https://")
        .or_else(|| u.strip_prefix("http://"))
        .unwrap_or(u);
    let u = u.strip_prefix("www.").unwrap_or(u);
    u.trim_end_matches('/').to_string()
}

/// The pages open in the browser now, as the extension tells (addresses only), the same page once.
/// Empty while no browser is connected.
fn open_pages() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (url, _) in crate::bridge::open_tabs() {
        let url = url.trim().to_string();
        let Some(host) = typed_site(&url) else {
            continue; // not a web page (about:blank, an extension page...)
        };
        if !out.iter().any(|(u, _)| *u == url) {
            out.push((url, host));
        }
    }
    out
}

/// The rows of the list for what is typed in the search field. With `keep_address` the open pages
/// come first with their own address; the sites CursorPie has seen follow.
fn site_rows(query: &str, keep_address: bool, manual: Option<&str>) -> Vec<SiteRow> {
    let mut rows: Vec<SiteRow> = Vec::new();
    let known = crate::smart::known_sites();
    let typed = typed_site(query);
    // a typed address that the list does not have comes first, as the one to add
    if let Some(host) = &typed
        && !known.contains(host)
    {
        let address = if keep_address {
            typed_address(query).unwrap_or_else(|| format!("https://{host}"))
        } else {
            format!("https://{host}")
        };
        rows.push(SiteRow {
            host: host.clone(),
            label: plain_address(&address),
            address,
            open: false,
            new: true,
        });
    }
    // text that is no site (a path, `localhost:3000`) but can still be opened as it is typed
    if typed.is_none()
        && let Some(target) = manual
    {
        rows.push(SiteRow {
            host: target.to_string(),
            address: target.to_string(),
            label: target.to_string(),
            open: false,
            new: true,
        });
    }
    if keep_address {
        for (url, host) in open_pages() {
            let label = plain_address(&url);
            if query.is_empty() || label.to_ascii_lowercase().contains(query) {
                rows.push(SiteRow {
                    host,
                    address: url,
                    label,
                    open: true,
                    new: false,
                });
            }
        }
    }
    for host in known {
        if !query.is_empty() && !host.contains(query) {
            continue;
        }
        // a site that has its own line as an open page is not listed twice
        if rows.iter().any(|r| r.label == host) {
            continue;
        }
        rows.push(SiteRow {
            address: format!("https://{host}"),
            label: host.clone(),
            host,
            open: false,
            new: false,
        });
    }
    rows
}

/// Draws the sheet and says what was chosen (the host, or with `keep_address` the address) and
/// whether it was closed. `taken` tells the sites that cannot be chosen again.
fn site_list_sheet(
    ctx: &egui::Context,
    ed: &mut Ed,
    accent: Color32,
    title: &str,
    keep_address: bool,
    any_text: bool,
    taken: &dyn Fn(&str) -> bool,
) -> (Option<String>, bool) {
    let size = Vec2::new(520.0, 580.0);
    let bounds = ed.card;
    let mut chosen: Option<String> = None;
    let mut cancel = false;
    let (floating, ()) = sheet(ctx, "pick_site", bounds, size, title, |ui| {
        let focus = ctx
            .data_mut(|d| d.remove_temp::<bool>(egui::Id::new("site_sheet_focus")))
            .unwrap_or(false);
        // the field, and beside it the button that adds what is typed
        let field = ui
            .horizontal(|ui| {
                const ADD_W: f32 = 92.0;
                let field = ui.add(
                    text_field(&mut ed.smart_new_site)
                        .desired_width(
                            (ui.available_width() - ADD_W - ui.spacing().item_spacing.x).max(120.0),
                        )
                        .hint_text("Search, or type an address such as bank.example"),
                );
                let can_add = typed_site(&ed.smart_new_site).is_some_and(|h| !taken(&h))
                    || (any_text && typed_target(&ed.smart_new_site).is_some());
                if button_if(
                    ui,
                    can_add,
                    Look::Primary(accent),
                    Some(symbols::PLUS),
                    "Add",
                )
                .on_hover_text("Use the address that is typed")
                .clicked()
                {
                    chosen = typed_pick(&ed.smart_new_site, keep_address, any_text);
                }
                field
            })
            .inner;
        if focus {
            field.request_focus();
        }
        ui.add_space(GAP);

        let query = ed.smart_new_site.trim().to_ascii_lowercase();
        let manual = if any_text {
            typed_target(&ed.smart_new_site)
        } else {
            None
        };
        let rows = site_rows(&query, keep_address, manual.as_deref());
        // Enter takes the first row that can be taken
        if field.lost_focus()
            && ctx.input(|i| i.key_pressed(egui::Key::Enter))
            && let Some(first) = rows.iter().find(|r| !taken(&r.host))
        {
            chosen = Some(if keep_address {
                first.address.clone()
            } else {
                first.host.clone()
            });
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
                        let r = &rows[k];
                        let is_taken = taken(&r.host);
                        let (row, resp) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), ROW_H),
                            if is_taken {
                                egui::Sense::hover()
                            } else {
                                egui::Sense::click()
                            },
                        );
                        let inner = row.shrink2(Vec2::new(0.0, 2.0));
                        if resp.hovered() && !is_taken {
                            ui.painter().rect_filled(inner, control_r(), pal().idle);
                        }
                        let icon = Rect::from_center_size(
                            Pos2::new(inner.min.x + 12.0 + 16.0, inner.center().y),
                            Vec2::splat(32.0),
                        );
                        // its own icon when it was fetched, otherwise the globe
                        let tex = siteicon::icon_for(&r.host).and_then(|i| ed.icons.get(ctx, &i));
                        if let Some(tex) = tex {
                            let whole = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                            let tint = if is_taken {
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
                        let mut label = if r.new {
                            format!("Add {}", r.label)
                        } else {
                            r.label.clone()
                        };
                        if label.chars().count() > 40 {
                            label =
                                format!("{}\u{2026}", label.chars().take(39).collect::<String>());
                        }
                        ui.painter().text(
                            Pos2::new(inner.min.x + 64.0, inner.center().y),
                            egui::Align2::LEFT_CENTER,
                            label,
                            egui::FontId::new(14.0, fonts::semibold()),
                            if is_taken { pal().muted } else { pal().text },
                        );
                        if is_taken {
                            fonts::paint_icon(
                                ui.painter(),
                                Pos2::new(inner.max.x - 20.0, inner.center().y),
                                symbols::CHECK,
                                16.0,
                                accent_text(accent),
                                false,
                            );
                        } else {
                            if r.open {
                                // the page is open in the browser now
                                ui.painter().text(
                                    Pos2::new(inner.max.x - 12.0, inner.center().y),
                                    egui::Align2::RIGHT_CENTER,
                                    "Open now",
                                    egui::FontId::new(12.0, fonts::semibold()),
                                    accent_text(accent),
                                );
                            }
                            if resp
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                chosen = Some(if keep_address {
                                    r.address.clone()
                                } else {
                                    r.host.clone()
                                });
                            }
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
    (chosen, cancel || floating.close)
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
    let (chosen, closed) = site_list_sheet(ctx, ed, accent, "Choose a site", false, false, &|h| {
        listed(edit, h)
    });
    if let Some(host) = chosen {
        let list = &mut edit.smart.ignore_sites;
        if list.len() < SMART_IGNORE_MAX && !list.iter().any(|s| s.eq_ignore_ascii_case(&host)) {
            list.push(host);
        }
        ed.profile_sheet = None;
        ed.smart_new_site.clear();
    } else if closed {
        ed.profile_sheet = None;
        ed.smart_new_site.clear();
    }
}

/// Opens the sheet to choose a site for the slice in [`Ed::action_target`].
pub(in crate::ui::settings) fn open_site_pick(ctx: &egui::Context, ed: &mut Ed, for_: SiteFor) {
    ed.smart_new_site.clear();
    ed.profile_sheet = Some(ProfileSheet::SitePick(for_));
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("site_sheet_focus"), true));
}

/// Draws the sheet while it is open for a slice, and sends the address that is chosen to the
/// slice: the address of its "URL / file" action, or one more slice of an Intelligent slice.
pub(in crate::ui::settings) fn site_pick_sheet(
    ctx: &egui::Context,
    ed: &mut Ed,
    tx: &Sender<Msg>,
    accent: Color32,
    for_: SiteFor,
) {
    let title = match for_ {
        SiteFor::Url => "Choose a website",
        SiteFor::Pin => "Add a website",
    };
    // a "URL / file" slice opens anything that is typed; a site slice needs a site
    let any_text = matches!(for_, SiteFor::Url);
    let (chosen, closed) = site_list_sheet(ctx, ed, accent, title, true, any_text, &|_| false);
    if let Some(address) = chosen {
        if let Some(t) = ed.action_target.take() {
            let msg = match for_ {
                SiteFor::Url => Msg::ActionUrlPicked {
                    profile: t.profile,
                    path: t.path,
                    slot: t.slot,
                    url: address,
                },
                SiteFor::Pin => {
                    // a slice of a site opens the site, not the page that happened to be open
                    let host = typed_site(&address).unwrap_or_else(|| address.clone());
                    Msg::SmartPinPicked {
                        profile: t.profile,
                        path: t.path,
                        target: format!("https://{host}"),
                        name: host,
                        site: true,
                    }
                }
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        }
        ed.profile_sheet = None;
        ed.smart_new_site.clear();
    } else if closed {
        ed.action_target = None;
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
    fn a_typed_address_is_opened_with_https() {
        assert_eq!(
            typed_address("bank.example/login").as_deref(),
            Some("https://bank.example/login")
        );
        assert_eq!(
            typed_address("http://bank.example").as_deref(),
            Some("http://bank.example")
        );
        assert_eq!(typed_address("bank"), None);
        assert_eq!(plain_address("https://www.bank.example/"), "bank.example");
    }

    #[test]
    fn a_url_slice_takes_what_is_typed_even_when_it_is_no_site() {
        assert_eq!(typed_target("").as_deref(), None);
        assert_eq!(typed_target("  ").as_deref(), None);
        assert_eq!(
            typed_target("claude.ai/chat/abc").as_deref(),
            Some("https://claude.ai/chat/abc")
        );
        assert_eq!(
            typed_target("localhost:3000/app").as_deref(),
            Some("http://localhost:3000/app")
        );
        assert_eq!(
            typed_target("192.168.1.5:8080").as_deref(),
            Some("http://192.168.1.5:8080")
        );
        assert_eq!(
            typed_target("~/Documents/a.pdf").as_deref(),
            Some("~/Documents/a.pdf")
        );
        assert_eq!(
            typed_target("mailto:me@bank.example").as_deref(),
            Some("mailto:me@bank.example")
        );
        assert_eq!(
            typed_pick("localhost:3000", true, false),
            None,
            "a site slice needs a site"
        );
        assert_eq!(
            typed_pick("localhost:3000", true, true).as_deref(),
            Some("http://localhost:3000")
        );
        assert_eq!(
            typed_pick("bank.example", false, false).as_deref(),
            Some("bank.example")
        );
    }

    #[test]
    fn the_add_button_and_enter_send_a_typed_address_to_the_slice() {
        use crate::ui::settings::state::{ActionTarget, Slot};
        for (typed, want, enter) in [
            ("claude.ai/chat/abc", "https://claude.ai/chat/abc", false),
            ("localhost:3000", "http://localhost:3000", false),
            ("/Users/me/a.pdf", "/Users/me/a.pdf", false),
            ("localhost:3000", "http://localhost:3000", true),
        ] {
            let ctx = egui::Context::default();
            crate::ui::fonts::install(&ctx);
            let mut ed = Ed::new();
            let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1180.0, 740.0));
            ed.card = screen;
            let (tx, rx) = std::sync::mpsc::channel();
            ed.action_target = Some(ActionTarget {
                profile: 0,
                path: vec![0],
                slot: Slot::Run,
            });
            open_site_pick(&ctx, &mut ed, SiteFor::Url);
            let run = |ed: &mut Ed, events: Vec<egui::Event>| {
                let input = egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                };
                ctx.run_ui(input, |ui| {
                    let ctx = ui.ctx().clone();
                    site_pick_sheet(&ctx, ed, &tx, Color32::from_rgb(90, 120, 94), SiteFor::Url);
                })
                .drop_without_applying_deltas();
            };
            for _ in 0..3 {
                run(&mut ed, vec![]);
            }
            run(&mut ed, vec![egui::Event::Text(typed.into())]);
            run(&mut ed, vec![]);
            assert_eq!(ed.smart_new_site, typed);
            if enter {
                let key = |pressed| egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: Default::default(),
                };
                run(&mut ed, vec![key(true), key(false)]);
            } else {
                // the Add button sits at the right end of the field's row
                let top = screen.center().y - 290.0;
                let p = Pos2::new(screen.center().x + 260.0 - 56.0, top + 82.0);
                let ev = |pressed| egui::Event::PointerButton {
                    pos: p,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                };
                run(&mut ed, vec![egui::Event::PointerMoved(p)]);
                run(&mut ed, vec![ev(true)]);
                run(&mut ed, vec![ev(false)]);
                run(&mut ed, vec![]);
            }
            assert!(ed.profile_sheet.is_none(), "{typed}: the sheet closes");
            match rx.try_recv() {
                Ok(Msg::ActionUrlPicked { url, .. }) => assert_eq!(url, want),
                _ => panic!("{typed}: no address was sent"),
            }
        }
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
