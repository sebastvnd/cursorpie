//! The "Intelligent" page of the Behavior tab: whether CursorPie learns, from what, and what the
//! intelligent pie and the intelligent slices suggest. See [`smart`](crate::smart).
//!
//! The switches and numbers edit `self.edit` like every other page, so they apply on Save. The two
//! buttons (*Learn now*, *Forget everything*) act at once on what is already learned: they do not
//! change a setting.

use super::profile_toolbar::{app_label, ignore_app_sheet, open_picker};
use super::smart_site::{open_site_sheet, site_sheet};
use crate::config::{
    Config, ItemKind, SMART_HOLD_MS_MAX, SMART_HOLD_MS_MIN, SMART_IDLE_S_MAX, SMART_IDLE_S_MIN,
    SMART_KEEP_DAYS_MAX, SMART_KEEP_DAYS_MIN, SMART_SAMPLE_S_MAX, SMART_SAMPLE_S_MIN,
    SMART_SLICES_MAX, SMART_SLICES_MIN, SmartLayout, SmartSource,
};
use crate::msg::Msg;
use crate::platform;
use crate::ui::app::App;
use crate::ui::settings::state::{Ed, PickFor};
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use eframe::egui::{self, Color32};

const INTRO: &str = "Hold Space in an open pie to turn it into a pie of suggestions made from what you use, sorted into Apps, Sites, Actions and Flows. It stays that way, even when you close the pie, until you hold Space again. A quick press of Space still opens Settings. An Intelligent slice puts the same suggestions in a submenu of any pie. Everything stays on this computer, and nothing is learned until you switch this on.";

/// One line for the panel of what was learned.
fn learned_line(s: &crate::smart::Stats) -> String {
    if s.counts.apps + s.counts.pages + s.counts.actions == 0 {
        return "Nothing learned yet.".into();
    }
    let mut line = format!(
        "{} apps, {} pages, {} actions.",
        s.counts.apps, s.counts.pages, s.counts.actions
    );
    if s.since > 0 {
        let days = crate::smart::collect::now_secs().saturating_sub(s.since) / 86_400;
        line += &match days {
            0 => " Learning since today.".to_string(),
            1 => " Learning since yesterday.".to_string(),
            d => format!(" Learning for {d} days."),
        };
    }
    line
}

/// Whether any pie has an intelligent slice at its top ring.
fn has_slice(c: &Config) -> bool {
    c.profiles
        .iter()
        .any(|p| p.items.iter().any(|i| matches!(i.kind, ItemKind::Smart(_))))
}

/// The name Settings shows for the app `id`: the one the list of installed apps has, otherwise the
/// last part of the id.
fn app_name(ed: &Ed, id: &str) -> String {
    ed.installed
        .iter()
        .find(|a| platform::OS.app_ids_equal(&a.id, id))
        .map_or_else(|| app_label(id), |a| a.name.clone())
}

fn notice(tx: &std::sync::mpsc::Sender<Msg>, ctx: &egui::Context, text: impl Into<String>) {
    let _ = tx.send(Msg::Notice(text.into()));
    ctx.request_repaint();
}

impl App {
    /// The "Intelligent" page: the switch, what is learned from, what is suggested, privacy, and
    /// where the intelligent slice goes.
    pub(in crate::ui::settings) fn behavior_smart(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let ctx = ui.ctx().clone();
        let tx = self.tx.clone();
        let stats = crate::smart::stats();
        let (cfg, ed) = (&mut self.edit, &mut self.ed);
        let was_on = cfg.smart.enabled;
        panel(ui, "Intelligent pie", |ui| {
            let s = &mut cfg.smart;
            note(ui, INTRO);
            toggle_row(
                ui,
                &mut s.enabled,
                "Learn and suggest",
                "The master switch. Off: nothing is observed or stored, Space is only for Settings, and intelligent slices are empty. What was learned is kept until you forget it.",
                accent,
            );
            ui.add_enabled_ui(s.enabled, |ui| {
                slider_row(
                    ui,
                    "Hold Space to switch",
                    &mut s.hold_ms,
                    SMART_HOLD_MS_MIN..=SMART_HOLD_MS_MAX,
                    " ms",
                    accent,
                );
                let labels = ["Mixed", "Grouped"];
                let at = usize::from(s.layout == SmartLayout::Grouped);
                row(ui, "Layout", "Grouped gives each category (Apps, Sites, Actions, Flows) a slice that opens its own ring, so a pie of suggestions is easy to read. Mixed puts the best suggestions of every source in one ring.", |ui| {
                    if let Some(i) = select(ui, 150.0, labels[at], &labels, at) {
                        s.layout = if i == 1 { SmartLayout::Grouped } else { SmartLayout::Mixed };
                        s.layout_chosen = true;
                    }
                });
                ui.add_enabled_ui(s.layout == SmartLayout::Mixed, |ui| {
                    slider_row(
                        ui,
                        "Slices in the mixed pie",
                        &mut s.pie_slices,
                        SMART_SLICES_MIN..=SMART_SLICES_MAX,
                        "",
                        accent,
                    );
                });
                toggle_row(
                    ui,
                    &mut s.time_of_day,
                    "Weigh the time of day",
                    "What you usually open in the morning is suggested more in the morning.",
                    accent,
                );
            });
        });
        panel(ui, "What it suggests", |ui| {
            let s = &mut cfg.smart;
            ui.add_enabled_ui(s.enabled, |ui| {
                for src in SmartSource::ALL {
                    let hint = match src {
                        SmartSource::OpenApps => "Apps that are open now, the ones you use most first. Only apps CursorPie has seen in front before.",
                        SmartSource::FrequentApps => "The apps you use most, and the one you usually go to next from the app in front.",
                        SmartSource::OpenPages => "Pages open in your browser now. Needs the browser extension (Behavior > Apps).",
                        SmartSource::FrequentPages => "The sites you visit most. Needs the browser extension.",
                        SmartSource::Actions => "What you run most from the pie in the app in front: shortcuts, commands, links, sub-actions.",
                        SmartSource::Flows => "Your flows, the ones you run most first.",
                    };
                    let mut on = s.has(src);
                    if toggle_row(ui, &mut on, src.label(), hint, accent).changed() {
                        s.set(src, on);
                    }
                }
                toggle_row(
                    ui,
                    &mut s.site_icons,
                    "Show the icon of a site",
                    "A suggested site shows its own logo instead of a placeholder. CursorPie asks the site for its icon (the same lookup as Icon from a website; if the site has none, Google's icon service is asked for the site's name), a few sites at a time in the background. The icons are files in the icons folder, and Forget everything deletes them.",
                    accent,
                );
            });
        });
        panel(ui, "Learning", |ui| {
            let s = &mut cfg.smart;
            ui.add_enabled_ui(s.enabled, |ui| {
                toggle_row(ui, &mut s.learn_apps, "Learn apps", "Which app is in front, for how long, and which one comes next. Never window titles or what you type.", accent);
                toggle_row(ui, &mut s.learn_pages, "Learn web pages", "The site of the page you are looking at in a browser, by its address only: never the title or the content. Needs the browser extension.", accent);
                toggle_row(ui, &mut s.learn_actions, "Learn what I run from the pie", "Which slices you release over, and in which app.", accent);
                toggle_row(ui, &mut s.learn_when_idle, "Tidy up only when idle", "Fading old habits, naming apps and saving wait until the pointer has been still for the time below. Off: it is done about a minute after something changed.", accent);
                ui.add_enabled_ui(s.learn_when_idle, |ui| {
                    slider_row(ui, "Idle after", &mut s.idle_s, SMART_IDLE_S_MIN..=SMART_IDLE_S_MAX, " s", accent);
                });
                slider_row(ui, "Look every", &mut s.sample_s, SMART_SAMPLE_S_MIN..=SMART_SAMPLE_S_MAX, " s", accent);
                slider_row(ui, "Forget after", &mut s.keep_days, SMART_KEEP_DAYS_MIN..=SMART_KEEP_DAYS_MAX, " days", accent);
            });
            note(ui, &learned_line(&stats));
            // The two buttons are about 300 points wide together, more than the usual control
            // column: in a row of the usual width they were drawn over the text beside them.
            row_wide(
                ui,
                "Now",
                "Learn now tidies, names the apps and saves at once. Forget everything deletes every habit and cannot be undone. Both act at once, not on Save.",
                340.0,
                |ui| {
                    ui.horizontal(|ui| {
                        if button(ui, Look::Soft, Some(symbols::SPARKLE), "Learn now").clicked() {
                            // the sheet opens at the click and the worker's answer fills it
                            ed.smart_sheet = Some(crate::ui::settings::SmartSheet::working(false));
                            crate::smart::learn_now();
                        }
                        if button(ui, Look::Danger, Some(symbols::TRASH), "Forget everything")
                            .clicked()
                        {
                            ed.smart_sheet = Some(crate::ui::settings::SmartSheet::working(true));
                            crate::smart::forget_all();
                        }
                    });
                },
            );
        });
        panel(ui, "Privacy", |ui| {
            let s = &mut cfg.smart;
            toggle_row(
                ui,
                &mut s.keep_page_paths,
                "Keep the path of a page",
                "Learn github.com/rust-lang/rust and not only github.com. A path can say much more than a site does, so this is off by default.",
                accent,
            );
            // apps: chosen from the same list as everywhere else in Settings
            row(
                ui,
                "Never learn these apps",
                "Choose them from the list of your apps. They are not learned and not suggested.",
                |ui| {
                    let room = s.ignore_apps.len() < crate::config::SMART_IGNORE_MAX;
                    if button_if(
                        ui,
                        room,
                        Look::Soft,
                        Some(symbols::APP_WINDOW),
                        "Choose app\u{2026}",
                    )
                    .clicked()
                    {
                        open_picker(&ctx, ed, &tx, PickFor::Ignore);
                    }
                },
            );
            let mut gone = None;
            for (i, id) in s.ignore_apps.iter().enumerate() {
                row(ui, &app_name(ed, id), id, |ui| {
                    if button(ui, Look::Danger, Some(symbols::TRASH), "Remove").clicked() {
                        gone = Some(i);
                    }
                });
            }
            if let Some(i) = gone {
                s.ignore_apps.remove(i);
            }
            // sites: chosen from the sites CursorPie knows, or typed
            row(
                ui,
                "Never learn these sites",
                "A site and its subdomains: bank.example covers www.bank.example. Choose one CursorPie has seen, or type it.",
                |ui| {
                    let room = s.ignore_sites.len() < crate::config::SMART_IGNORE_MAX;
                    if button_if(
                        ui,
                        room,
                        Look::Soft,
                        Some(symbols::GLOBE),
                        "Choose site\u{2026}",
                    )
                    .clicked()
                    {
                        open_site_sheet(&ctx, ed);
                    }
                },
            );
            let mut gone = None;
            for (i, site) in s.ignore_sites.iter().enumerate() {
                row(ui, site, "", |ui| {
                    if button(ui, Look::Danger, Some(symbols::TRASH), "Remove").clicked() {
                        gone = Some(i);
                    }
                });
            }
            if let Some(i) = gone {
                s.ignore_sites.remove(i);
            }
            note(
                ui,
                "The habits are one file, smart-habits.json, next to config.json, readable only by you. The habits are never sent anywhere. Only the icons of the suggested sites are asked of those sites (see What it suggests).",
            );
        });
        panel(ui, "Intelligent slice", |ui| {
            note(
                ui,
                "An Intelligent slice is a submenu of suggestions. Add it to the general pie, or to the pie of one app. It can also be added from the Menu tab (Add a slice, Custom).",
            );
            let names: Vec<String> = cfg.profiles.iter().map(|p| p.name.clone()).collect();
            let labels: Vec<&str> = names.iter().map(String::as_str).collect();
            let at = ed.smart_add_to.min(names.len().saturating_sub(1));
            row(ui, "Add it to the pie of", "", |ui| {
                ui.horizontal(|ui| {
                    if !labels.is_empty()
                        && let Some(i) = select(ui, 150.0, labels[at], &labels, at)
                    {
                        ed.smart_add_to = i;
                    }
                    if button(ui, Look::Soft, Some(symbols::PLUS), "Add").clicked() {
                        let name = names.get(at).cloned().unwrap_or_default();
                        if crate::smart::add_slice(cfg, at) {
                            notice(&tx, &ctx, format!("Added an Intelligent slice to \u{201c}{name}\u{201d}. Save to keep it."));
                        } else {
                            notice(&tx, &ctx, format!("\u{201c}{name}\u{201d} already has one, or has no room for another slice."));
                        }
                    }
                });
            });
        });
        // switching the feature on for the first time: a pie with no intelligent slice has nowhere
        // to show the suggestions but the held Space, so the general pie gets one
        if !was_on && cfg.smart.enabled && !has_slice(cfg) {
            let general = cfg.global_profile();
            if crate::smart::add_slice(cfg, general) {
                notice(
                    &tx,
                    &ctx,
                    "Added an Intelligent slice to the general pie. Save to keep it.",
                );
            }
        }
        ignore_app_sheet(&ctx, cfg, ed, &tx, accent);
        site_sheet(&ctx, cfg, ed, accent);
    }
}
