//! Profile toolbar of the Menu tab: pick, create, rename and delete profiles, edit the apps a
//! profile applies to, and share (export / import) a profile with its icons.
//!
//! The row is kept small on purpose:
//!
//! ```text
//! [ (icon) Code  v ]  [ (icon)(icon)(icon) 5 apps ]                         [ + ]  [ ... ]
//! ```
//!
//! * the **profile chip** shows the profile with the icon of its app and opens a list of every
//!   profile, each with its icon and how many apps it covers;
//! * the **apps button** shows the real icons of the profile's apps and opens a sheet with one big
//!   tile per app (see [`ProfileSheet`]);
//! * **+** opens the "New profile" sheet; **...** unfolds rename, export, import and delete as a
//!   row of icons to its left (hiding **+** and turning into an X while they show).
//!
//! Everything that needs typing opens as a [`sheet`]: a dimmed window with the dialog in the
//! middle, the same as "Add a slice", so nothing hangs off a small button any more.
//!
//! There is exactly one general profile (no apps, cannot be deleted). Every other profile belongs
//! to at least one app: the "New profile" sheet asks for it, and the last app cannot be removed.

use crate::config::*;
use crate::msg::Msg;
use crate::platform;
use crate::ui::appicon;
use crate::ui::fonts;
use crate::ui::icons::IconState;
use crate::ui::settings::picker::spawn_file_picker;
use crate::ui::settings::state::{AppScan, Ed, PickFor, ProfileSheet};
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Stroke, Vec2};
use std::sync::mpsc::Sender;

/// Shows `text` in the status bar of Settings (this file has no access to `App`).
fn notice(tx: &Sender<Msg>, ctx: &egui::Context, text: impl Into<String>) {
    let _ = tx.send(Msg::Notice(text.into()));
    ctx.request_repaint();
}

/// A short, readable name for an app id: `com.visualstudio.code` becomes "Code", `firefox`
/// becomes "Firefox". The full id stays in the tooltip.
pub(super) fn app_label(id: &str) -> String {
    let id = id.trim().trim_end_matches(".desktop");
    let last = id
        .rsplit('.')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(id);
    let mut chars = last.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `text` cut to at most `max` characters, with an ellipsis if something was dropped.
fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}

/// Draws the icon of the app `bundle` in `rect`: its real icon when the system has one, otherwise
/// a tinted tile with the first letter.
fn avatar(
    painter: &egui::Painter,
    ctx: &egui::Context,
    ed: &mut Ed,
    rect: Rect,
    bundle: &str,
    accent: Color32,
) {
    let key = appicon::icon_string(AppIconStyle::Color, bundle);
    // the icon is read off the UI thread: scrolling a long list must not wait for the system
    let state = ed.app_icon_async(ctx, &key);
    if let IconState::Ready(tex) = &state {
        let whole = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        painter.image(tex.id(), rect, whole, Color32::WHITE);
        return;
    }
    let radius = (rect.width() * 0.28).round() as u8;
    painter.rect_filled(rect, CornerRadius::same(radius), wash(accent, 0.14));
    if matches!(state, IconState::Pending) {
        return; // the icon follows in a moment: a plain tile, not a letter that then changes
    }
    let letter: String = app_label(bundle).chars().take(1).collect();
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        if letter.is_empty() {
            "?".into()
        } else {
            letter
        },
        egui::FontId::new(rect.width() * 0.46, fonts::semibold()),
        accent_text(accent),
    );
}

/// The stand-in for a profile without an app (the general profile): a window symbol on a soft tile.
fn general_avatar(painter: &egui::Painter, rect: Rect, accent: Color32) {
    let radius = (rect.width() * 0.28).round() as u8;
    painter.rect_filled(rect, CornerRadius::same(radius), wash(accent, 0.14));
    fonts::paint_icon(
        painter,
        rect.center(),
        symbols::APP_WINDOW,
        rect.width() * 0.56,
        accent_text(accent),
        false,
    );
}

/// The avatar of a profile: the icon of its first app, or the general one.
fn profile_avatar(
    painter: &egui::Painter,
    ctx: &egui::Context,
    ed: &mut Ed,
    rect: Rect,
    profile: &Profile,
    accent: Color32,
) {
    match profile.apps.first() {
        Some(app) => avatar(painter, ctx, ed, rect, app, accent),
        None => general_avatar(painter, rect, accent),
    }
}

/// "3 apps", "1 app" or "All apps".
fn apps_text(profile: &Profile) -> String {
    if profile.is_general() {
        "All apps".into()
    } else {
        let n = profile.apps.len();
        format!("{n} app{}", if n == 1 { "" } else { "s" })
    }
}

/// A soft, control-sized clickable area with the same fill rules as a [`Look::Soft`] button.
fn soft_area(ui: &mut egui::Ui, size: Vec2, sense: egui::Sense) -> (Rect, egui::Response) {
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let fill = if resp.hovered() || resp.has_focus() {
        pal().hot
    } else {
        pal().idle
    };
    ui.painter().rect_filled(rect, CONTROL_R, fill);
    (rect, resp)
}

/// The profile chip: the icon and name of the current profile. Returns the picked profile, if any.
fn profile_chip(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    edit: &Config,
    ed: &mut Ed,
    accent: Color32,
) -> Option<usize> {
    let width = 230.0;
    let (rect, resp) = soft_area(ui, Vec2::new(width, CONTROL_H), egui::Sense::click());
    let cur = &edit.profiles[ed.profile];
    let avatar_rect = Rect::from_center_size(
        Pos2::new(rect.min.x + 8.0 + 12.0, rect.center().y),
        Vec2::splat(24.0),
    );
    profile_avatar(ui.painter(), ctx, ed, avatar_rect, cur, accent);
    ui.painter().text(
        Pos2::new(rect.min.x + 44.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        clip(&cur.name, 20),
        egui::FontId::new(14.0, fonts::semibold()),
        pal().text,
    );
    fonts::paint_icon(
        ui.painter(),
        Pos2::new(rect.max.x - 18.0, rect.center().y),
        symbols::CARET_DOWN,
        14.0,
        pal().muted,
        false,
    );
    let resp = resp
        .on_hover_text("Switch profile")
        .on_hover_cursor(egui::CursorIcon::PointingHand);

    let mut picked = None;
    let list_w = 300.0;
    egui::Popup::from_toggle_button_response(&resp)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(egui::RectAlign::BOTTOM_START)
        .gap(6.0)
        .width(list_w)
        .frame(popup_frame())
        .show(|ui| {
            ui.set_width(list_w);
            ui.spacing_mut().item_spacing.y = 2.0;
            for (i, p) in edit.profiles.iter().enumerate() {
                let (row, r) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), 46.0),
                    egui::Sense::click(),
                );
                let on = i == ed.profile;
                if r.hovered() || on {
                    let fill = if on { wash(accent, 0.10) } else { pal().idle };
                    ui.painter().rect_filled(row, CONTROL_R, fill);
                }
                let a = Rect::from_center_size(
                    Pos2::new(row.min.x + 12.0 + 15.0, row.center().y),
                    Vec2::splat(30.0),
                );
                profile_avatar(ui.painter(), ctx, ed, a, p, accent);
                ui.painter().text(
                    Pos2::new(row.min.x + 56.0, row.center().y - 9.0),
                    egui::Align2::LEFT_CENTER,
                    clip(&p.name, 26),
                    egui::FontId::new(14.0, fonts::semibold()),
                    pal().text,
                );
                ui.painter().text(
                    Pos2::new(row.min.x + 56.0, row.center().y + 10.0),
                    egui::Align2::LEFT_CENTER,
                    apps_text(p),
                    egui::FontId::new(12.0, egui::FontFamily::Proportional),
                    pal().muted,
                );
                if on {
                    fonts::paint_icon(
                        ui.painter(),
                        Pos2::new(row.max.x - 20.0, row.center().y),
                        symbols::CHECK,
                        16.0,
                        accent_text(accent),
                        false,
                    );
                }
                if r.clicked() {
                    picked = Some(i);
                    ui.close();
                }
            }
        });
    picked
}

/// The apps button: overlapping real icons of the profile's apps and a count. For the general
/// profile it is a plain label: there is nothing to edit.
fn apps_button(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    edit: &Config,
    ed: &mut Ed,
    accent: Color32,
) -> egui::Response {
    let prof = &edit.profiles[ed.profile];
    let general = prof.is_general();
    let text = ui.painter().layout_no_wrap(
        apps_text(prof),
        egui::FontId::new(13.0, egui::FontFamily::Proportional),
        pal().muted,
    );
    let shown = prof.apps.len().min(3);
    let step = 16.0;
    let stack_w = if general {
        0.0
    } else {
        24.0 + step * shown.saturating_sub(1) as f32
    };
    let gap = if general { 0.0 } else { 8.0 };
    let width = 12.0 + stack_w + gap + text.size().x + 14.0;
    let sense = if general {
        egui::Sense::hover()
    } else {
        egui::Sense::click()
    };
    let (rect, resp) = soft_area(ui, Vec2::new(width, CONTROL_H), sense);
    let x = rect.min.x + 12.0;
    let backdrop = if resp.hovered() {
        pal().hot
    } else {
        pal().idle
    };
    // later icons are drawn first so the first app ends up on top
    for (i, app) in prof.apps.iter().take(shown).enumerate().rev() {
        let a = Rect::from_min_size(
            Pos2::new(x + step * i as f32, rect.center().y - 12.0),
            Vec2::splat(24.0),
        );
        ui.painter().circle_filled(a.center(), 13.0, backdrop);
        avatar(ui.painter(), ctx, ed, a, app, accent);
    }
    ui.painter().galley(
        Pos2::new(x + stack_w + gap, rect.center().y - text.size().y / 2.0),
        text,
        pal().muted,
    );
    if general {
        resp.on_hover_text("The general profile: used for every app without its own profile.")
    } else {
        resp.on_hover_text("Choose which apps use this profile")
            .on_hover_cursor(egui::CursorIcon::PointingHand)
    }
}

/// One row: profile chip, apps, and on the right "new" and "more".
///
/// The dialogs it opens are drawn at the end, over the whole window (see [`ProfileSheet`]).
pub(super) fn profile_toolbar(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    edit: &mut Config,
    ed: &mut Ed,
    tx: &Sender<Msg>,
    accent: egui::Color32,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if let Some(i) = profile_chip(ui, ctx, edit, ed, accent)
            && i != ed.profile
        {
            ed.profile = i;
            ed.reset_nav();
        }
        let pi = ed.profile;

        let apps = apps_button(ui, ctx, edit, ed, accent);
        if apps.clicked() && !edit.profiles[pi].is_general() {
            ed.profile_sheet = Some(ProfileSheet::Apps);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            // "..." unfolds the rarely used actions as a row of icons to its left (hiding "+"
            // meanwhile) and turns into an X that folds them again
            let more_id = egui::Id::new("profile_more_open");
            let mut open = ctx.data(|d| d.get_temp::<bool>(more_id)).unwrap_or(false);
            let (icon, tip) = if open {
                (symbols::X, "Close")
            } else {
                (symbols::DOTS_THREE, "More")
            };
            if icon_button_look(ui, Look::Soft, icon, tip, true).clicked() {
                open = !open;
            }
            let t = ctx.animate_bool_with_time(more_id.with("t"), open, 0.15);
            if open {
                // added from the right edge inwards, so this is Rename, Export, Import, Delete
                // from left to right
                ui.scope(|ui| {
                    ui.set_opacity(t);
                    // the general profile is the fallback for every app, so it must stay
                    let can_delete = edit.profiles.len() > 1 && !edit.profiles[pi].is_general();
                    let delete = icon_button_look(
                        ui,
                        Look::Danger,
                        symbols::TRASH,
                        "Delete profile",
                        can_delete,
                    );
                    if delete.clicked() && can_delete {
                        edit.profiles.remove(pi);
                        ed.profile = pi.saturating_sub(1);
                        ed.reset_nav();
                        ed.stop_recording();
                        open = false;
                    }
                    let import = icon_button_look(
                        ui,
                        Look::Soft,
                        symbols::DOWNLOAD_SIMPLE,
                        "Import a profile\u{2026}",
                        true,
                    );
                    if import.clicked() {
                        spawn_file_picker(
                            tx.clone(),
                            ctx.clone(),
                            platform::FileKind::Json,
                            "Choose a CursorPie profile",
                            |file| Some(Msg::ProfileImportPicked(file)),
                        );
                        open = false;
                    }
                    let export = icon_button_look(
                        ui,
                        Look::Soft,
                        symbols::UPLOAD_SIMPLE,
                        "Export this profile\u{2026}",
                        true,
                    );
                    if export.clicked() {
                        match share::export_to_downloads_with(&edit.profiles[pi], &edit.flows) {
                            Ok(path) => {
                                platform::reveal_in_file_manager(&path);
                                notice(tx, ctx, format!("Exported to {}", path.display()));
                            }
                            Err(e) => notice(tx, ctx, format!("Export failed: {e}")),
                        }
                        open = false;
                    }
                    let rename = icon_button_look(
                        ui,
                        Look::Soft,
                        symbols::PENCIL_SIMPLE,
                        "Rename\u{2026}",
                        true,
                    );
                    if rename.clicked() {
                        ed.new_profile_name = edit.profiles[pi].name.clone();
                        open_sheet(ctx, ed, ProfileSheet::Rename);
                        open = false;
                    }
                });
            } else {
                let new = icon_button_look(ui, Look::Soft, symbols::PLUS, "New profile", true);
                if new.clicked() {
                    ed.new_profile_name.clear();
                    ed.new_profile_app.clear();
                    open_sheet(ctx, ed, ProfileSheet::New);
                }
            }
            ctx.data_mut(|d| d.insert_temp(more_id, open));
        });
    });

    match ed.profile_sheet {
        Some(ProfileSheet::New) => new_profile_sheet(ctx, edit, ed, tx, accent),
        Some(ProfileSheet::Rename) => rename_sheet(ctx, edit, ed, accent),
        Some(ProfileSheet::Apps) => apps_sheet(ctx, edit, ed, tx, accent),
        Some(ProfileSheet::Pick(for_)) => pick_app_sheet(ctx, edit, ed, tx, accent, for_),
        None => {}
    }
}

/// Opens the list of installed apps for `for_`, and reads the apps again in the background so the
/// list is current (the one read before stays on screen until the new one arrives).
pub(in crate::ui::settings) fn open_picker(
    ctx: &egui::Context,
    ed: &mut Ed,
    tx: &Sender<Msg>,
    for_: PickFor,
) {
    open_sheet(ctx, ed, ProfileSheet::Pick(for_));
    ed.app_query.clear();
    if ed.installed_scan != AppScan::Reading {
        ed.installed_scan = AppScan::Reading;
        let (tx, ctx) = (tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let apps = platform::installed::installed_apps();
            diag::log!("ui", "installed_apps", count = apps.len());
            let _ = tx.send(Msg::InstalledApps(apps));
            ctx.request_repaint();
        });
    }
}

/// The sheet that lists the installed apps with their icons and a search box, the same on every
/// system. A click answers it and goes back to the dialog it came from. "Browse files" is still
/// there for an app the list does not know (a portable program, an unusual folder).
fn pick_app_sheet(
    ctx: &egui::Context,
    edit: &mut Config,
    ed: &mut Ed,
    tx: &Sender<Msg>,
    accent: Color32,
    for_: PickFor,
) {
    // the dialog this list came from; the editor of a slice is not a dialog, so it has none
    let back = match for_ {
        PickFor::New => Some(ProfileSheet::New),
        PickFor::Add => Some(ProfileSheet::Apps),
        PickFor::Action | PickFor::Flow => None,
    };
    let profile = ed.profile;
    // apps that are already chosen are shown with a check and cannot be picked twice
    let taken: Vec<String> = match for_ {
        PickFor::Add => edit
            .profiles
            .get(profile)
            .map(|p| p.apps.clone())
            .unwrap_or_default(),
        PickFor::New => vec![ed.new_profile_app.clone()],
        PickFor::Action | PickFor::Flow => Vec::new(),
    };
    let is_taken =
        |id: &str| !id.is_empty() && taken.iter().any(|t| platform::OS.app_ids_equal(t, id));

    let size = Vec2::new(520.0, 580.0);
    let bounds = ed.card;
    let mut chosen: Option<platform::installed::InstalledApp> = None;
    let mut browse = false;
    let mut cancel = false;
    let (floating, ()) = sheet(ctx, "pick_app", bounds, size, "Choose an app", |ui| {
        let field = ui.add(
            text_field(&mut ed.app_query)
                .desired_width(ui.available_width())
                .hint_text("Search apps"),
        );
        if wants_focus(ctx) {
            field.request_focus();
        }
        ui.add_space(GAP);

        let shown: Vec<usize> = ed
            .installed
            .iter()
            .enumerate()
            .filter(|(_, a)| platform::installed::matches(a, &ed.app_query))
            .map(|(i, _)| i)
            .collect();
        // the first hit is taken with Enter
        if entered(ctx, &field)
            && let Some(&i) = shown.first()
            && !is_taken(&ed.installed[i].id)
        {
            chosen = Some(ed.installed[i].clone());
        }

        let list_h = (ui.available_height() - 54.0).max(120.0);
        if shown.is_empty() {
            ui.allocate_ui(Vec2::new(ui.available_width(), list_h), |ui| {
                ui.centered_and_justified(|ui| {
                    let text = if ed.installed_scan == AppScan::Reading && ed.installed.is_empty() {
                        "Looking for installed apps\u{2026}"
                    } else if ed.app_query.trim().is_empty() {
                        "No installed apps were found. Use \u{201c}Browse files\u{201d}."
                    } else {
                        "No app matches your search."
                    };
                    muted(ui, text);
                });
            });
        } else {
            let row_h = 52.0;
            egui::ScrollArea::vertical()
                .max_height(list_h)
                .auto_shrink([false, false])
                .show_rows(ui, row_h, shown.len(), |ui, range| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for k in range {
                        let app = ed.installed[shown[k]].clone();
                        let taken = is_taken(&app.id);
                        let (row, resp) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), row_h),
                            if taken {
                                egui::Sense::hover()
                            } else {
                                egui::Sense::click()
                            },
                        );
                        let inner = row.shrink2(Vec2::new(0.0, 2.0));
                        if resp.hovered() && !taken {
                            ui.painter().rect_filled(inner, CONTROL_R, pal().idle);
                        }
                        let icon = Rect::from_center_size(
                            Pos2::new(inner.min.x + 12.0 + 18.0, inner.center().y),
                            Vec2::splat(36.0),
                        );
                        avatar(ui.painter(), ctx, ed, icon, &app.id, accent);
                        let tint = if taken { pal().muted } else { pal().text };
                        ui.painter().text(
                            Pos2::new(inner.min.x + 64.0, inner.center().y - 9.0),
                            egui::Align2::LEFT_CENTER,
                            clip(&app.name, 36),
                            egui::FontId::new(14.0, fonts::semibold()),
                            tint,
                        );
                        ui.painter().text(
                            Pos2::new(inner.min.x + 64.0, inner.center().y + 10.0),
                            egui::Align2::LEFT_CENTER,
                            clip(&app.id, 48),
                            egui::FontId::new(12.0, egui::FontFamily::Proportional),
                            pal().muted,
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
                            chosen = Some(app);
                        }
                    }
                });
        }

        to_bottom(ui, CONTROL_H);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if button(
                ui,
                Look::Soft,
                Some(symbols::FOLDER_OPEN),
                "Browse files\u{2026}",
            )
            .on_hover_text("For an app that is not in the list")
            .clicked()
            {
                browse = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if button(ui, Look::Soft, None, "Back").clicked() {
                    cancel = true;
                }
            });
        });
    });

    if let Some(app) = chosen {
        match for_ {
            PickFor::New => {
                ed.new_profile_app = app.id;
                if ed.new_profile_name.trim().is_empty() {
                    ed.new_profile_name = app.name;
                }
            }
            PickFor::Add => {
                if let Some(p) = edit.profiles.get_mut(profile)
                    && !p.apps.contains(&app.id)
                {
                    p.apps.push(app.id);
                }
            }
            PickFor::Flow => {
                if let Some((flow, node)) = ed.flow_target.take() {
                    let _ = tx.send(Msg::FlowAppPicked {
                        flow,
                        node,
                        app: platform::installed::action_text(&app),
                    });
                    ctx.request_repaint();
                }
            }
            PickFor::Action => {
                // the same message a chosen file gives, so the slice also takes the app's icon
                if let Some(t) = ed.action_target.take() {
                    let _ = tx.send(Msg::ActionAppPicked {
                        profile: t.profile,
                        path: t.path,
                        slot: t.slot,
                        app: platform::installed::action_text(&app),
                    });
                    ctx.request_repaint();
                }
            }
        }
        ed.profile_sheet = back;
    } else if browse {
        match for_ {
            PickFor::New => spawn_file_picker(
                tx.clone(),
                ctx.clone(),
                platform::FileKind::App,
                "Choose an application",
                |file| platform::bundle_id_of(&file).map(Msg::NewProfileAppPicked),
            ),
            PickFor::Add => spawn_file_picker(
                tx.clone(),
                ctx.clone(),
                platform::FileKind::App,
                "Choose an application",
                move |file| {
                    platform::bundle_id_of(&file).map(|bundle| Msg::AppPicked { profile, bundle })
                },
            ),
            PickFor::Flow => {
                if let Some((flow, node)) = ed.flow_target.take() {
                    spawn_file_picker(
                        tx.clone(),
                        ctx.clone(),
                        platform::FileKind::App,
                        "Choose an application",
                        move |app| Some(Msg::FlowAppPicked { flow, node, app }),
                    );
                }
            }
            PickFor::Action => {
                if let Some(t) = ed.action_target.take() {
                    spawn_file_picker(
                        tx.clone(),
                        ctx.clone(),
                        platform::FileKind::App,
                        "Choose an application",
                        move |app| {
                            Some(Msg::ActionAppPicked {
                                profile: t.profile,
                                path: t.path,
                                slot: t.slot,
                                app,
                            })
                        },
                    );
                }
            }
        }
        ed.profile_sheet = back;
    } else if cancel || floating.close {
        ed.action_target = None;
        ed.flow_target = None;
        ed.profile_sheet = back;
    }
}

/// The app list of the Flow tab: shown while it was opened by the **Pick...** button of an "App"
/// block (the Menu tab shows its own with the profile toolbar).
pub(in crate::ui::settings) fn flow_app_sheet(
    ctx: &egui::Context,
    edit: &mut Config,
    ed: &mut Ed,
    tx: &Sender<Msg>,
    accent: egui::Color32,
) {
    if let Some(ProfileSheet::Pick(PickFor::Flow)) = ed.profile_sheet {
        pick_app_sheet(ctx, edit, ed, tx, accent, PickFor::Flow);
    }
}

/// Opens `which` and asks its first text field for the keyboard.
fn open_sheet(ctx: &egui::Context, ed: &mut Ed, which: ProfileSheet) {
    ed.profile_sheet = Some(which);
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("profile_sheet_focus"), true));
}

/// True once after [`open_sheet`]: the field should grab the keyboard now.
fn wants_focus(ctx: &egui::Context) -> bool {
    ctx.data_mut(|d| d.remove_temp::<bool>(egui::Id::new("profile_sheet_focus")))
        .unwrap_or(false)
}

/// Pins the next widgets to the bottom of a sheet, `height` points tall.
fn to_bottom(ui: &mut egui::Ui, height: f32) {
    ui.add_space((ui.available_height() - height).max(GAP));
}

/// A label above a field.
fn field_label(ui: &mut egui::Ui, text: &str) {
    ui.label(fonts::title(text, 12.5, pal().muted));
    ui.add_space(4.0);
}

/// Whether Enter was pressed while `field` had the keyboard.
fn entered(ctx: &egui::Context, field: &egui::Response) -> bool {
    field.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter))
}

/// "New profile": a name and the app the profile belongs to, both required.
fn new_profile_sheet(
    ctx: &egui::Context,
    edit: &mut Config,
    ed: &mut Ed,
    tx: &Sender<Msg>,
    accent: Color32,
) {
    let size = Vec2::new(480.0, 352.0);
    let bounds = ed.card;
    let mut create = false;
    let (floating, ()) = sheet(ctx, "new_profile", bounds, size, "New profile", |ui| {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(64.0), egui::Sense::hover());
            if ed.new_profile_app.trim().is_empty() {
                general_avatar(ui.painter(), rect, accent);
            } else {
                let app = ed.new_profile_app.trim().to_string();
                avatar(ui.painter(), ctx, ed, rect, &app, accent);
            }
            ui.add_space(GAP);
            ui.vertical(|ui| {
                field_label(ui, "Name");
                let name = ui.add(
                    text_field(&mut ed.new_profile_name)
                        .desired_width(ui.available_width())
                        .hint_text("For example: Code"),
                );
                if wants_focus(ctx) {
                    name.request_focus();
                }
                if entered(ctx, &name) {
                    create = true;
                }
            });
        });
        ui.add_space(GAP + 4.0);
        field_label(ui, "App");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let app = ui.add(
                text_field(&mut ed.new_profile_app)
                    .desired_width(ui.available_width() - 134.0)
                    .hint_text(platform::OS.app_id_hint()),
            );
            if entered(ctx, &app) {
                create = true;
            }
            if button(ui, Look::Soft, Some(symbols::APP_WINDOW), "Choose app")
                .on_hover_text("Pick from the installed apps")
                .clicked()
            {
                open_picker(ctx, ed, tx, PickFor::New);
            }
        });
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(
                "The profile opens only while this app is in front. The general profile covers \
                 every other app.",
            )
            .size(12.5)
            .color(pal().muted),
        );
        to_bottom(ui, CONTROL_H);
        let full = edit.profiles.len() >= MAX_PROFILES;
        let valid = Profile::for_app(&ed.new_profile_name, &ed.new_profile_app).is_some();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let go = button_if(
                ui,
                valid && !full,
                Look::Primary(accent),
                Some(symbols::CHECK),
                "Create profile",
            );
            if full {
                go.on_disabled_hover_text("The profile limit is reached. Delete one first.");
            } else if go.clicked() {
                create = true;
            }
            if button(ui, Look::Soft, None, "Cancel").clicked() {
                ed.profile_sheet = None;
            }
        });
    });

    if create
        && edit.profiles.len() < MAX_PROFILES
        && let Some(profile) = Profile::for_app(&ed.new_profile_name, &ed.new_profile_app)
    {
        edit.profiles.push(profile);
        ed.profile = edit.profiles.len() - 1;
        ed.reset_nav();
        ed.stop_recording();
        ed.new_profile_name.clear();
        ed.new_profile_app.clear();
        ed.profile_sheet = None;
    } else if floating.close {
        ed.profile_sheet = None;
    }
}

/// "Rename profile": one field, applied with the button or Enter. An empty name is not accepted.
fn rename_sheet(ctx: &egui::Context, edit: &mut Config, ed: &mut Ed, accent: Color32) {
    let size = Vec2::new(440.0, 214.0);
    let bounds = ed.card;
    let mut apply = false;
    let (floating, ()) = sheet(
        ctx,
        "rename_profile",
        bounds,
        size,
        "Rename profile",
        |ui| {
            field_label(ui, "Name");
            let name = ui.add(
                text_field(&mut ed.new_profile_name)
                    .desired_width(ui.available_width())
                    .hint_text("Profile name"),
            );
            if wants_focus(ctx) {
                name.request_focus();
            }
            if entered(ctx, &name) {
                apply = true;
            }
            to_bottom(ui, CONTROL_H);
            let ok = !ed.new_profile_name.trim().is_empty();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if button_if(ui, ok, Look::Primary(accent), Some(symbols::CHECK), "Save").clicked()
                {
                    apply = true;
                }
                if button(ui, Look::Soft, None, "Cancel").clicked() {
                    ed.profile_sheet = None;
                }
            });
        },
    );
    if apply && !ed.new_profile_name.trim().is_empty() {
        if let Some(p) = edit.profiles.get_mut(ed.profile) {
            p.name = ed.new_profile_name.trim().to_string();
        }
        ed.new_profile_name.clear();
        ed.profile_sheet = None;
    } else if floating.close {
        ed.profile_sheet = None;
    }
}

/// "Apps": one big tile per app with its real icon. Hover a tile for the remove button; the last
/// tile adds an app, and below the grid an id can be typed.
fn apps_sheet(
    ctx: &egui::Context,
    edit: &mut Config,
    ed: &mut Ed,
    tx: &Sender<Msg>,
    accent: Color32,
) {
    let profile = ed.profile;
    let Some(prof) = edit.profiles.get_mut(profile) else {
        ed.profile_sheet = None;
        return;
    };
    if prof.is_general() {
        ed.profile_sheet = None;
        return;
    }
    let size = Vec2::new(572.0, 452.0);
    let bounds = ed.card;
    let title = format!("Apps of \u{201c}{}\u{201d}", clip(&prof.name, 24));
    let mut drop_app: Option<usize> = None;
    let mut add_app: Option<String> = None;
    let mut open_apps = false;
    let (floating, ()) = sheet(ctx, "profile_apps", bounds, size, &title, |ui| {
        ui.label(
            egui::RichText::new("This profile opens while one of these apps is in front.")
                .size(13.0)
                .color(pal().muted),
        );
        ui.add_space(GAP);
        let last = prof.apps.len() == 1;
        let grid_h = (ui.available_height() - 64.0).max(120.0);
        egui::ScrollArea::vertical()
            .max_height(grid_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(10.0);
                ui.horizontal_wrapped(|ui| {
                    for (i, app) in prof.apps.iter().enumerate() {
                        let (tile, resp) =
                            ui.allocate_exact_size(Vec2::new(112.0, 104.0), egui::Sense::hover());
                        let hot = ui.rect_contains_pointer(tile);
                        ui.painter().rect_filled(
                            tile,
                            PANEL_R,
                            if hot { pal().hot } else { pal().idle },
                        );
                        let icon = Rect::from_center_size(
                            Pos2::new(tile.center().x, tile.min.y + 14.0 + 24.0),
                            Vec2::splat(48.0),
                        );
                        avatar(ui.painter(), ctx, ed, icon, app, accent);
                        ui.painter().text(
                            Pos2::new(tile.center().x, tile.max.y - 24.0),
                            egui::Align2::CENTER_CENTER,
                            clip(&app_label(app), 13),
                            egui::FontId::new(13.0, fonts::semibold()),
                            pal().text,
                        );
                        resp.on_hover_text(app);
                        // the remove badge appears with the pointer
                        if hot {
                            let badge = Rect::from_center_size(
                                Pos2::new(tile.max.x - 14.0, tile.min.y + 14.0),
                                Vec2::splat(22.0),
                            );
                            let b = ui.interact(
                                badge,
                                egui::Id::new(("drop_app", i)),
                                egui::Sense::click(),
                            );
                            let color = if last { pal().muted } else { pal().bad };
                            let alpha = if b.hovered() && !last { 0.22 } else { 0.12 };
                            ui.painter()
                                .circle_filled(badge.center(), 11.0, wash(color, alpha));
                            fonts::paint_icon(
                                ui.painter(),
                                badge.center(),
                                symbols::X,
                                12.0,
                                color,
                                false,
                            );
                            let b = if last {
                                b.on_hover_text("A profile needs at least one app")
                            } else {
                                b.on_hover_text("Remove")
                            };
                            if b.clicked() && !last {
                                drop_app = Some(i);
                            }
                        }
                    }
                    // the add tile
                    let (tile, resp) =
                        ui.allocate_exact_size(Vec2::new(112.0, 104.0), egui::Sense::click());
                    let c = if resp.hovered() { accent } else { pal().border };
                    ui.painter().rect_stroke(
                        tile.shrink(0.75),
                        PANEL_R,
                        Stroke::new(1.5, c),
                        egui::StrokeKind::Inside,
                    );
                    let plus = if resp.hovered() {
                        accent_text(accent)
                    } else {
                        pal().muted
                    };
                    fonts::paint_icon(
                        ui.painter(),
                        Pos2::new(tile.center().x, tile.min.y + 38.0),
                        symbols::PLUS,
                        26.0,
                        plus,
                        false,
                    );
                    ui.painter().text(
                        Pos2::new(tile.center().x, tile.max.y - 24.0),
                        egui::Align2::CENTER_CENTER,
                        "Add app",
                        egui::FontId::new(13.0, fonts::semibold()),
                        pal().muted,
                    );
                    if resp
                        .on_hover_text("Choose an installed application")
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        open_apps = true;
                    }
                });
            });
        to_bottom(ui, CONTROL_H);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let field = ui.add(
                text_field(&mut ed.new_app)
                    .desired_width(ui.available_width() - 250.0)
                    .hint_text(format!("Or type an id, {}", platform::OS.app_id_hint())),
            );
            let go = button_if(
                ui,
                !ed.new_app.trim().is_empty(),
                Look::Soft,
                Some(symbols::PLUS),
                "Add",
            );
            if go.clicked() || entered(ctx, &field) {
                add_app = Some(ed.new_app.trim().to_string());
                ed.new_app.clear();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if button(ui, Look::Primary(accent), None, "Done").clicked() {
                    ed.profile_sheet = None;
                }
            });
        });
    });
    if let Some(i) = drop_app {
        prof.apps.remove(i);
    }
    if open_apps {
        open_picker(ctx, ed, tx, PickFor::Add);
        return;
    }
    if let Some(id) = add_app
        && !id.is_empty()
        && !prof.apps.contains(&id)
    {
        prof.apps.push(id);
    }
    if floating.close {
        ed.profile_sheet = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_ids_read_as_names() {
        assert_eq!(app_label("firefox"), "Firefox");
        assert_eq!(app_label("com.visualstudio.code"), "Code");
        assert_eq!(app_label("org.mozilla.firefox.desktop"), "Firefox");
        assert_eq!(app_label("code-oss"), "Code-oss");
        assert_eq!(app_label(""), "");
    }

    #[test]
    fn long_text_is_cut_with_an_ellipsis() {
        assert_eq!(clip("short", 10), "short");
        assert_eq!(clip("abcdefghij", 5), "abcd\u{2026}");
    }

    #[test]
    fn apps_are_counted_in_words() {
        let one = Profile::for_app("A", "a").unwrap();
        assert_eq!(apps_text(&one), "1 app");
        let mut two = one.clone();
        two.apps.push("b".into());
        assert_eq!(apps_text(&two), "2 apps");
    }

    /// Draws the toolbar with each dialog open in a headless context: a smoke test for layout
    /// panics (zero widths, a sheet bigger than the window) and for the dialogs closing again.
    #[test]
    fn every_dialog_draws_and_closes_without_panicking() {
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut edit = Config::default();
        let mut ed = Ed::new();
        ed.profile = edit
            .profiles
            .iter()
            .position(|p| !p.is_general())
            .unwrap_or(0);
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1180.0, 740.0));
        ed.card = screen;
        ed.installed = (0..40)
            .map(|i| platform::installed::InstalledApp {
                id: format!("app{i}"),
                name: format!("App number {i}"),
            })
            .collect();
        ed.installed_scan = AppScan::Done;
        let accent = Color32::from_rgb(90, 120, 94);
        for which in [
            None,
            Some(ProfileSheet::New),
            Some(ProfileSheet::Rename),
            Some(ProfileSheet::Apps),
            Some(ProfileSheet::Pick(PickFor::New)),
            Some(ProfileSheet::Pick(PickFor::Add)),
            Some(ProfileSheet::Pick(PickFor::Action)),
            Some(ProfileSheet::Pick(PickFor::Flow)),
        ] {
            ed.profile_sheet = which;
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                };
                let mut out = ctx.run_ui(input, |ui| {
                    let ctx = ui.ctx().clone();
                    profile_toolbar(ui, &ctx, &mut edit, &mut ed, &tx, accent);
                });
                out.textures_delta.clear();
            }
        }
        assert!(ed.sheet_open() || ed.profile_sheet.is_none());
        assert!(ed.close_sheet() || ed.profile_sheet.is_none());
        assert!(!ed.sheet_open());
    }
}
