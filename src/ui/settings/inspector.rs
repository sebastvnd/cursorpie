//! Editor for one slice (the right-hand panel of the Menu tab).

use super::action_editor::action_editor;
use super::icon_picker::icon_picker;
use super::picker::spawn_file_picker;
use super::state::{ActionTarget, Ed, PickFor, Rec, SiteFor, Slot};
use super::tabs::profile_toolbar::open_picker;
use super::tabs::smart_site::open_site_pick;
use crate::config::*;
use crate::keys::{Shortcut, key};
use crate::msg::Msg;
use crate::platform::FileKind;
use crate::tap::{RECORDING, SWALLOW_KEY};
use crate::ui::appicon::{self, SliceTone};
use crate::ui::draw::*;
use crate::ui::fonts;
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use eframe::egui::{self, Color32, Rect, Vec2};
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;

/// Read-only things the recursive editor needs.
pub(super) struct Env<'a> {
    /// The trigger button of the edited config: it opens the pie, so it cannot be a side button too
    pub trigger: i64,
    /// The extra buttons of the mouse in use (`mouse::active_buttons`)
    pub mouse: &'a [i64],
    pub ctx: &'a egui::Context,
    pub tx: &'a Sender<Msg>,
    pub accent: Color32,
    pub profile: usize,
    /// width of the right-hand panel (computed explicitly by the Menu tab). With `flat`, the width
    /// of what is inside the panel the host drew.
    pub width: f32,
    /// The host already drew the panel (a full-height one, like the inspector of the Flow tab): the
    /// editor draws no card of its own, and keeps its footer at the bottom of `height`.
    pub flat: bool,
    /// With `flat`, how tall the room for the editor is, so that a short editor does not leave the
    /// panel looking empty; unused otherwise.
    pub height: f32,
    /// the window's rectangle, which sheets dim and center over
    pub bounds: Rect,
    /// the flows a slice can run, as `(id, name)`
    pub flows: &'a [(String, String)],
}

/// Width of the content of a card that is `outer` wide: its padding and hairline come off.
fn inner_width(outer: f32) -> f32 {
    outer - 2.0 * f32::from(PANEL_PAD) - 2.0
}

/// A few points of room left under the footer of a panel of its own, so that rounding never makes
/// the panel scroll for a pixel.
const FOOTER_SLACK: f32 = 4.0;

/// A pale line across the editor, above its footer (the line between rows of a panel).
fn hairline(ui: &mut egui::Ui) {
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, pal().divider);
}

/// A small title above one action of a side-button slice.
fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(GAP / 2.0);
    ui.label(fonts::title(text, 13.0, pal().muted));
}

/// The number of a slice's kind in the kind control: 0 = mode (side buttons), 1 = run (action),
/// 2 = submenu, 3 = intelligent. It is the numbering [`default_kind`] takes, and the control must
/// use no other: it once numbered run as 0 and mode as 1, so the control marked "Side buttons" on
/// a slice that runs an action (and "Action" on one that is a mode). The marked option is the one
/// that cannot be picked, so "Side buttons" did nothing on an action slice, and "Action" made a new
/// action over the one it already had.
fn kind_code(kind: &ItemKind) -> usize {
    match kind {
        ItemKind::Mode { .. } => 0,
        ItemKind::Run(_) => 1,
        ItemKind::Submenu(_) => 2,
        ItemKind::Smart(_) => 3,
    }
}

/// Initial kind for the kind control: 0 = mode, 1 = run, 3 = intelligent, otherwise submenu.
fn default_kind(k: usize) -> ItemKind {
    match k {
        0 => presets().remove(0).kind,
        1 => ItemKind::Run(Action::Key(Shortcut::new(key::F13, 0))),
        3 => ItemKind::Smart(crate::config::SmartSlice::default()),
        _ => ItemKind::Submenu(Vec::new()),
    }
}

/// Whether this exact action field is being recorded.
fn is_rec(ed: &Ed, profile: usize, path: &[usize], slot: Slot) -> bool {
    ed.recording
        .as_ref()
        .is_some_and(|r| r.profile == profile && r.path == path && r.slot == slot)
}

/// Starts (or toggles off) shortcut recording for one field and arms the event tap.
fn start_rec(ed: &mut Ed, profile: usize, path: &[usize], slot: Slot) {
    if is_rec(ed, profile, path, slot) {
        ed.stop_recording();
    } else {
        SWALLOW_KEY.store(-1, Ordering::SeqCst);
        RECORDING.store(true, Ordering::SeqCst);
        ed.recording = Some(Rec {
            profile,
            path: path.to_vec(),
            slot,
        });
    }
}

/// Lets the user choose an image file for the slice icon.
fn pick_icon(env: &Env, path: &[usize]) {
    let (profile, path) = (env.profile, path.to_vec());
    spawn_file_picker(
        env.tx.clone(),
        env.ctx.clone(),
        FileKind::Image,
        "Choose an icon image",
        // runs on the chooser's worker thread: a big image is shrunk there, not on the UI thread
        move |file| match crate::ui::iconfit::fit(&file, crate::ui::iconfit::Mode::InPlace) {
            Ok(fitted) => Some(Msg::IconPicked {
                profile,
                path,
                file: fitted.icon,
                note: fitted.note,
            }),
            // a pop-up that says why, not one line that disappears
            Err(why) => Some(Msg::IconRefused {
                file,
                lines: vec![why],
            }),
        },
    );
}

/// Imports the images dropped on the window as icons, on a worker thread (a big photo takes a
/// moment to shrink). The first one that works becomes the icon of the slice at `path`; the others
/// are added to the icons folder ("Mine"); each one that cannot be used is explained in a pop-up.
fn import_dropped(env: &Env, path: &[usize], files: Vec<std::path::PathBuf>) {
    let (profile, path) = (env.profile, path.to_vec());
    let (tx, ctx) = (env.tx.clone(), env.ctx.clone());
    std::thread::spawn(move || {
        let batch = crate::ui::iconfit::import_all(
            &files,
            &crate::config::icons_dir(),
            &crate::config::tilde,
        );
        if let Some(icon) = batch.icon {
            let more = batch.added.saturating_sub(1);
            let note = match (batch.note, more) {
                (n, 0) => n,
                (Some(n), m) => Some(format!("{n} {m} more added to your icons (Mine).")),
                (None, m) => Some(format!("{m} more added to your icons (Mine).")),
            };
            let _ = tx.send(Msg::IconPicked {
                profile,
                path,
                file: icon,
                note,
            });
        }
        if !batch.failed.is_empty() {
            let (file, lines) = match batch.failed.as_slice() {
                [(name, why)] => (name.clone(), vec![why.clone()]),
                many => (
                    format!("{} files", many.len()),
                    many.iter().map(|(n, why)| format!("{n}: {why}")).collect(),
                ),
            };
            let _ = tx.send(Msg::IconRefused { file, lines });
        }
        ctx.request_repaint();
    });
}

/// Drag and drop of image files on Settings: while files are held over the window a hint covers it,
/// and when they are dropped they become the icon of the selected slice (`path`). Works with the
/// icon picker open or closed.
fn icon_drop(env: &Env, ed: &mut Ed, path: &[usize], name: &str) {
    let (hovering, dropped) = env.ctx.input(|i| {
        let files: Vec<std::path::PathBuf> = i
            .raw
            .dropped_files
            .iter()
            .map(|f| f.path().to_path_buf())
            .collect();
        (!i.raw.hovered_files.is_empty(), files)
    });
    if hovering {
        let painter = env.ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("icon_drop_hint"),
        ));
        let rect = env.bounds.shrink(10.0);
        painter.rect(
            rect,
            14.0,
            env.accent.gamma_multiply(0.16),
            egui::Stroke::new(2.0, env.accent),
            egui::StrokeKind::Inside,
        );
        let label = if name.trim().is_empty() {
            "Drop images to use as the icon".to_string()
        } else {
            format!(
                "Drop images to use as the icon of \u{201c}{}\u{201d}",
                name.trim()
            )
        };
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(18.0),
            pal().text,
        );
        env.ctx.request_repaint();
    }
    if !dropped.is_empty() {
        ed.picking_icon = false;
        import_dropped(env, path, dropped);
    }
}

/// Draws the editor for the selected slice of `ring` (`nav` = path of the ring itself).
pub(super) fn item_inspector(
    ui: &mut egui::Ui,
    ed: &mut Ed,
    env: &Env,
    ring: &mut Vec<Item>,
    nav: &[usize],
) {
    let Some(i) = ed.sel.filter(|&i| i < ring.len()) else {
        // nothing selected: an empty tile that says what to do
        let mut add = false;
        let mut tile_of = |ui: &mut egui::Ui, pad: f32| {
            ui.add_space(pad);
            ui.vertical_centered(|ui| {
                ui.label(fonts::icon(symbols::CURSOR_CLICK, 40.0, pal().muted));
                ui.add_space(GAP / 2.0);
                ui.label(fonts::title("Pick a slice", 17.0, pal().text));
                muted(
                    ui,
                    "Select a slice on the pie to edit its name, icon and actions.",
                );
                if ring.len() < MAX_ITEMS {
                    ui.add_space(GAP);
                    if button(ui, Look::Soft, Some(symbols::PLUS), "Add a slice").clicked() {
                        add = true;
                    }
                }
            });
            ui.add_space(pad);
        };
        if env.flat {
            // the middle of the panel, not the top of it
            ui.set_width(env.width);
            tile_of(ui, ((env.height - 170.0) / 2.0).max(GAP * 4.0));
        } else {
            card().show(ui, |ui| {
                ui.set_width(inner_width(env.width));
                tile_of(ui, GAP * 4.0);
            });
        }
        if add {
            ed.adding = true;
        }
        return;
    };
    let n = ring.len();
    let mut path = nav.to_vec();
    path.push(i);

    // ---- drag and drop of image files on the window (the picker may be open or not)
    icon_drop(env, ed, &path, &ring[i].name);

    // ---- icon picker: a sheet over the window, the editor stays visible behind it
    if ed.picking_icon {
        let item = &mut ring[i];
        let (floating, r) = sheet(
            env.ctx,
            "icon_picker",
            env.bounds,
            Vec2::new(560.0, 620.0),
            "Choose an icon",
            |ui| {
                let w = ui.available_width();
                icon_picker(ui, ed, env.ctx, &mut item.icon, env.accent, w)
            },
        );
        if let Some(g) = r.chosen {
            item.icon = g;
            ed.picking_icon = false;
        }
        if r.pick_file {
            pick_icon(env, &path);
            ed.picking_icon = false;
        }
        if r.pick_web {
            // a slice that opens a link starts with that site
            let link = match &ring[i].kind {
                ItemKind::Run(Action::OpenUrl(s)) => s.clone(),
                _ => String::new(),
            };
            ed.web_icon.open_for(&link, env.ctx);
            ed.picking_icon = false;
        }
        if let Some(file) = r.delete {
            // one small file: removed here; the UI thread says what it was (and who used it)
            let error = super::icon_catalog::delete_user_icon(&file).err();
            let _ = env.tx.send(Msg::IconDeleted { file, error });
            env.ctx.request_repaint();
        }
        if floating.close {
            ed.picking_icon = false;
        }
    }

    // ---- icon from a website: fetched with a loading animation, then tidied to match the others
    if ed.web_icon.open {
        let step =
            super::web_icon::web_icon_sheet(&mut ed.web_icon, env.ctx, env.bounds, env.accent);
        match step {
            super::web_icon::Step::Use { file, note } => {
                let _ = env.tx.send(Msg::IconPicked {
                    profile: env.profile,
                    path: path.clone(),
                    file,
                    note,
                });
                ed.web_icon.close();
                env.ctx.request_repaint();
            }
            super::web_icon::Step::Close => ed.web_icon.close(),
            super::web_icon::Step::Stay => {}
        }
    }

    let uid = format!(
        "insp_{}_{}",
        env.profile,
        path.iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join("-")
    );
    let (rb, rf, rr) = (
        is_rec(ed, env.profile, &path, Slot::Back),
        is_rec(ed, env.profile, &path, Slot::Forward),
        is_rec(ed, env.profile, &path, Slot::Run),
    );
    // the extra buttons offered for a mode: the mouse's, and the ones that already have an action
    let used: Vec<i64> = match &ring[i].kind {
        ItemKind::Mode { extra, .. } => extra.keys().copied().collect(),
        _ => Vec::new(),
    };
    let buttons = crate::mouse::editor_buttons(env.mouse, &used, env.trigger);
    let extra_rec: Vec<bool> = buttons
        .iter()
        .map(|&b| is_rec(ed, env.profile, &path, Slot::Extra(b)))
        .collect();
    let (mut start, remove, mut enter, mut shift) = (None::<Slot>, false, false, 0i32);
    let mut pick_app: Option<Slot> = None;
    // "choose a website for this slice", and the slice that was added by hand: an app (false) or a
    // website (true) for the Intelligent slice
    let mut pick_site: Option<Slot> = None;
    let mut pin_pick: Option<bool> = None;
    let mut change_icon = false;
    // "use the icon of the app this slice opens", in this tone (with the app as the action has it)
    let mut use_app_icon: Option<(SliceTone, String)> = None;
    // "use the icon of the website this slice opens": the web link, as the action has it
    let mut use_site_icon: Option<String> = None;

    let mut body = |ui: &mut egui::Ui| {
        let item = &mut ring[i];

        // icon (click to change) + name + slice kind
        ui.horizontal(|ui| {
            let tex = ed.icon_for(env.ctx, &item.icon);
            let (r, resp) = tile(ui, Vec2::splat(72.0), false, env.accent);
            draw_icon(
                ui.painter(),
                tex.as_ref(),
                &item.icon,
                r.center() - Vec2::new(0.0, 4.0),
                34.0,
                pie_colors().ink,
            );
            fonts::paint_icon(
                ui.painter(),
                r.center_bottom() - Vec2::new(0.0, 11.0),
                symbols::PENCIL_SIMPLE,
                12.0,
                pal().muted,
                false,
            );
            if resp.on_hover_text("Change icon").clicked() {
                change_icon = true;
            }
            ui.vertical(|ui| {
                ui.add(
                    text_field(&mut item.name)
                        .desired_width(ui.available_width())
                        .hint_text("Name"),
                );
                {
                    let now = kind_code(&item.kind);
                    // (what the button makes, the button); a ring one level deeper than the limit
                    // is not offered, unless the slice already is one
                    let mut kinds: Vec<(usize, Option<char>, &str)> = vec![
                        (1, Some(symbols::LIGHTNING), "Action"),
                        (0, Some(symbols::MOUSE_SIMPLE), "Side buttons"),
                    ];
                    if nav.len() + 1 < MAX_DEPTH || now == 2 {
                        kinds.push((2, Some(symbols::STACK), "Submenu"));
                    }
                    if nav.len() + 1 < MAX_DEPTH || now == 3 {
                        kinds.push((3, Some(symbols::SPARKLE), "Intelligent"));
                    }
                    let buttons: Vec<(Option<char>, &str)> =
                        kinds.iter().map(|(_, i, l)| (*i, *l)).collect();
                    let at = kinds.iter().position(|(k, _, _)| *k == now).unwrap_or(0);
                    if let Some(sel) = segmented(ui, &buttons, at)
                        && let Some((made, _, _)) = kinds.get(sel)
                    {
                        item.kind = default_kind(*made);
                    }
                }
            });
        });

        ui.add_space(GAP / 2.0);
        match &mut item.kind {
            ItemKind::Run(a) => {
                section_label(ui, "Action");
                let r = action_editor(ui, &format!("{uid}_r"), a, rr, env.accent, env.flows);
                if r.shortcut {
                    start = Some(Slot::Run);
                }
                if r.pick_app {
                    pick_app = Some(Slot::Run);
                }
                if r.pick_site {
                    pick_site = Some(Slot::Run);
                }
                // a slice that opens an app can wear that app's icon, in color or as a white or
                // black glyph
                if let Action::OpenApp(app) = a
                    && !app.trim().is_empty()
                {
                    let now = appicon::parse_slice(&item.icon).map(|(tone, _)| tone);
                    section_label(ui, "Icon of this app");
                    ui.horizontal(|ui| {
                        for tone in SliceTone::ALL {
                            let look = if now == Some(tone) {
                                Look::Primary(env.accent)
                            } else {
                                Look::Soft
                            };
                            if button(ui, look, None, tone.label()).clicked() {
                                use_app_icon = Some((tone, app.clone()));
                            }
                        }
                    });
                    muted(ui, "White suits a dark or glass pie, black a light one.");
                }
                // a slice that opens a web link can wear the icon of that website, taken from the
                // site itself (no browser extension) and kept in the icons folder
                if let Action::OpenUrl(url) = a
                    && crate::config::is_web_url(url)
                {
                    section_label(ui, "Icon of this website");
                    if button(ui, Look::Soft, Some(symbols::LINK), "Use website icon").clicked() {
                        use_site_icon = Some(url.trim().to_string());
                    }
                    muted(
                        ui,
                        "Fetched from the site, tidied to match your other icons, and saved in Mine.",
                    );
                }
            }
            ItemKind::Mode {
                back,
                forward,
                extra,
            } => {
                section_label(ui, "Back button");
                let r = action_editor(ui, &format!("{uid}_b"), back, rb, env.accent, env.flows);
                if r.shortcut {
                    start = Some(Slot::Back);
                }
                if r.pick_app {
                    pick_app = Some(Slot::Back);
                }
                if r.pick_site {
                    pick_site = Some(Slot::Back);
                }
                section_label(ui, "Forward button");
                let r = action_editor(ui, &format!("{uid}_f"), forward, rf, env.accent, env.flows);
                if r.shortcut {
                    start = Some(Slot::Forward);
                }
                if r.pick_app {
                    pick_app = Some(Slot::Forward);
                }
                if r.pick_site {
                    pick_site = Some(Slot::Forward);
                }
                for (k, &b) in buttons.iter().enumerate() {
                    section_label(ui, &crate::mouse::button_label(b));
                    // an action of "None" is not stored: the button then passes through
                    let mut act = extra.get(&b).cloned().unwrap_or_default();
                    let r = action_editor(
                        ui,
                        &format!("{uid}_x{b}"),
                        &mut act,
                        extra_rec[k],
                        env.accent,
                        env.flows,
                    );
                    if act == Action::Nothing {
                        extra.remove(&b);
                    } else {
                        extra.insert(b, act);
                    }
                    if r.shortcut {
                        start = Some(Slot::Extra(b));
                    }
                    if r.pick_app {
                        pick_app = Some(Slot::Extra(b));
                    }
                    if r.pick_site {
                        pick_site = Some(Slot::Extra(b));
                    }
                }
                if buttons.is_empty() {
                    muted(ui, "More buttons? Scan your mouse in General > Mouse.");
                }
            }
            ItemKind::Submenu(ch) => {
                section_label(ui, "Submenu");
                muted(ui, &format!("{} slices inside", ch.len()));
                if button(ui, Look::Soft, Some(symbols::CARET_RIGHT), "Edit submenu").clicked() {
                    enter = true;
                }
            }
            ItemKind::Smart(s) => {
                section_label(ui, "Suggested from your habits");
                muted(
                    ui,
                    "A ring that is made when the pie opens. Choose what it draws from; none chosen means everything that is switched on in Behavior > Intelligent.",
                );
                // the sources flow side by side and wrap, instead of one tall column; the room
                // between them is wide enough for the shadow of the theme
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(10.0, 12.0);
                    ui.horizontal_wrapped(|ui| {
                        for src in crate::config::SmartSource::ALL {
                            let on = s.sources.contains(&src);
                            let look = if on {
                                Look::Primary(env.accent)
                            } else {
                                Look::Soft
                            };
                            if button(ui, look, None, src.label()).clicked() {
                                if on {
                                    s.sources.retain(|x| *x != src);
                                } else {
                                    s.sources.push(src);
                                    s.sources.sort();
                                }
                            }
                        }
                    });
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Suggestions");
                    ui.add(egui::DragValue::new(&mut s.count).range(1..=MAX_ITEMS as u8));
                });
                // what the person adds by hand: always in the ring, before the suggestions
                section_label(ui, "Added by you");
                muted(
                    ui,
                    "Apps and websites you choose yourself. They come first in the ring; the suggestions follow.",
                );
                let mut gone: Option<usize> = None;
                for (k, pin) in s.pinned.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let icon = if pin.site {
                            symbols::GLOBE
                        } else {
                            symbols::APP_WINDOW
                        };
                        let (at, _) =
                            ui.allocate_exact_size(Vec2::splat(20.0), egui::Sense::hover());
                        fonts::paint_icon(
                            ui.painter(),
                            at.center(),
                            icon,
                            16.0,
                            pal().muted,
                            false,
                        );
                        ui.label(&pin.name);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icon_button_look(ui, Look::Danger, symbols::X, "Remove", true)
                                .clicked()
                            {
                                gone = Some(k);
                            }
                        });
                    });
                }
                if let Some(k) = gone {
                    s.pinned.remove(k);
                }
                let room = s.pinned.len() < MAX_ITEMS;
                ui.horizontal_wrapped(|ui| {
                    if button_if(
                        ui,
                        room,
                        Look::Soft,
                        Some(symbols::APP_WINDOW),
                        "Add app\u{2026}",
                    )
                    .clicked()
                    {
                        pin_pick = Some(false);
                    }
                    if button_if(
                        ui,
                        room,
                        Look::Soft,
                        Some(symbols::GLOBE),
                        "Add site\u{2026}",
                    )
                    .clicked()
                    {
                        pin_pick = Some(true);
                    }
                });
            }
        }

        // the footer: where the slice stands in its ring, and the two ways to move it. In a panel
        // of its own it stays at the bottom, so the panel is never half empty.
        if env.flat {
            // Everything the footer takes, the space egui puts between two things included: the
            // room before it, the gap, the line, the gap and the row. Counted short, the panel
            // would scroll by the difference.
            let sp = ui.spacing().item_spacing.y;
            let footer = 4.0 * sp + GAP + 1.0 + GAP / 2.0 + CONTROL_H;
            let used = ui.min_rect().height();
            let room = env.height - used - footer - FOOTER_SLACK;
            if room > 0.0 {
                ui.add_space(room);
            }
        }
        ui.add_space(GAP);
        hairline(ui);
        ui.add_space(GAP / 2.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("Slice {} of {}", i + 1, n)).color(pal().muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                // right to left: the one added first is the one on the right
                if icon_button_look(
                    ui,
                    Look::Soft,
                    symbols::CARET_RIGHT,
                    "Move right",
                    i + 1 < n,
                )
                .clicked()
                {
                    shift = 1;
                }
                if icon_button_look(ui, Look::Soft, symbols::CARET_LEFT, "Move left", i > 0)
                    .clicked()
                {
                    shift = -1;
                }
            });
        });
    };
    if env.flat {
        ui.set_width(env.width);
        body(ui);
    } else {
        card().show(ui, |ui| {
            ui.set_width(inner_width(env.width));
            body(ui);
        });
    }

    if let Some(slot) = start {
        start_rec(ed, env.profile, &path, slot);
    }
    if let Some(slot) = pick_app {
        // the list of installed apps, the same one as "Add app" in the profile's apps
        ed.action_target = Some(ActionTarget {
            profile: env.profile,
            path: path.clone(),
            slot,
        });
        open_picker(env.ctx, ed, env.tx, PickFor::Action);
    }
    if let Some(slot) = pick_site {
        // the list of sites, the one of Behavior > Intelligent, answering this slice
        ed.action_target = Some(ActionTarget {
            profile: env.profile,
            path: path.clone(),
            slot,
        });
        open_site_pick(env.ctx, ed, SiteFor::Url);
    }
    if let Some(site) = pin_pick {
        ed.action_target = Some(ActionTarget {
            profile: env.profile,
            path: path.clone(),
            slot: Slot::Run,
        });
        if site {
            open_site_pick(env.ctx, ed, SiteFor::Pin);
        } else {
            open_picker(env.ctx, ed, env.tx, PickFor::Pin);
        }
    }
    if let Some((tone, app)) = use_app_icon {
        match appicon::app_id_for_action(&app, &ed.installed) {
            Some(id) => ring[i].icon = appicon::slice_icon_string(tone, &id),
            None => {
                let _ = env.tx.send(Msg::Notice(
                    "CursorPie could not find the icon of that app.".into(),
                ));
                env.ctx.request_repaint();
            }
        }
    }
    if let Some(url) = use_site_icon {
        // the sheet fetches the icon (with a loading animation) and lets the user tidy it
        ed.picking_icon = false;
        ed.web_icon.open_for(&url, env.ctx);
        env.ctx.request_repaint();
    }
    if change_icon {
        ed.picking_icon = true;
        ed.icon_query.clear();
        ed.reload_user_icons();
    }
    if shift != 0 {
        let j = (i as i32 + shift) as usize;
        ring.swap(i, j);
        ed.sel = Some(j);
        ed.stop_recording();
    }
    if remove {
        ring.remove(i);
        ed.sel = None;
        ed.stop_recording();
    }
    if enter {
        ed.nav.push(i);
        ed.sel = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kind_control_marks_the_kind_the_slice_really_has() {
        // what the control makes (`default_kind`) must be what it then marks (`kind_code`)
        for code in 0..=3 {
            assert_eq!(kind_code(&default_kind(code)), code, "kind {code}");
        }
        assert!(matches!(default_kind(0), ItemKind::Mode { .. }));
        assert!(matches!(default_kind(1), ItemKind::Run(_)));
    }
}
