//! Editor for one slice (the right-hand panel of the Menu tab).

use super::action_editor::action_editor;
use super::icon_picker::icon_picker;
use super::picker::spawn_file_picker;
use super::state::{ActionTarget, Ed, PickFor, Rec, Slot};
use super::tabs::profile_toolbar::open_picker;
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
    /// width of the right-hand panel (computed explicitly by the Menu tab)
    pub width: f32,
    /// the window's rectangle, which sheets dim and center over
    pub bounds: Rect,
    /// the flows a slice can run, as `(id, name)`
    pub flows: &'a [(String, String)],
}

/// Width of the content of a card that is `outer` wide: its padding and hairline come off.
fn inner_width(outer: f32) -> f32 {
    outer - 2.0 * f32::from(PANEL_PAD) - 2.0
}

/// A small title above one action of a side-button slice.
fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(GAP / 2.0);
    ui.label(fonts::title(text, 13.0, pal().muted));
}

/// Initial kind for the kind control: 0 = mode, 1 = run, otherwise submenu.
fn default_kind(k: usize) -> ItemKind {
    match k {
        0 => presets().remove(0).kind,
        1 => ItemKind::Run(Action::Key(Shortcut::new(key::F13, 0))),
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
        move |file| match crate::ui::iconfit::fit(&file) {
            Ok(fitted) => Some(Msg::IconPicked {
                profile,
                path,
                file: fitted.icon,
                note: fitted.note,
            }),
            Err(why) => Some(Msg::Notice(why)),
        },
    );
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
        card().show(ui, |ui| {
            ui.set_width(inner_width(env.width));
            ui.add_space(GAP * 4.0);
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
            ui.add_space(GAP * 4.0);
        });
        if add {
            ed.adding = true;
        }
        return;
    };
    let n = ring.len();
    let mut path = nav.to_vec();
    path.push(i);

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
        if floating.close {
            ed.picking_icon = false;
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
    let mut change_icon = false;
    // "use the icon of the app this slice opens", in this tone (with the app as the action has it)
    let mut use_app_icon: Option<(SliceTone, String)> = None;

    card().show(ui, |ui| {
        ui.set_width(inner_width(env.width));
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
                    let now = match &item.kind {
                        ItemKind::Run(_) => 0,
                        ItemKind::Mode { .. } => 1,
                        ItemKind::Submenu(_) => 2,
                    };
                    let mut kinds: Vec<(Option<char>, &str)> = vec![
                        (Some(symbols::LIGHTNING), "Action"),
                        (Some(symbols::MOUSE_SIMPLE), "Side buttons"),
                    ];
                    if nav.len() + 1 < MAX_DEPTH || now == 2 {
                        kinds.push((Some(symbols::STACK), "Submenu"));
                    }
                    if let Some(sel) = segmented(ui, &kinds, now) {
                        item.kind = match sel {
                            0 => default_kind(1),
                            1 => default_kind(0),
                            _ => default_kind(2),
                        };
                    }
                }
            });
        });

        ui.add_space(4.0);
        match &mut item.kind {
            ItemKind::Run(a) => {
                let r = action_editor(ui, &format!("{uid}_r"), a, rr, env.accent, env.flows);
                if r.shortcut {
                    start = Some(Slot::Run);
                }
                if r.pick_app {
                    pick_app = Some(Slot::Run);
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
                section_label(ui, "Forward button");
                let r = action_editor(ui, &format!("{uid}_f"), forward, rf, env.accent, env.flows);
                if r.shortcut {
                    start = Some(Slot::Forward);
                }
                if r.pick_app {
                    pick_app = Some(Slot::Forward);
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
                }
                if buttons.is_empty() {
                    muted(ui, "More buttons? Scan your mouse in General > Mouse.");
                }
            }
            ItemKind::Submenu(ch) => {
                muted(ui, &format!("{} slices inside", ch.len()));
                if button(ui, Look::Soft, Some(symbols::CARET_RIGHT), "Edit submenu").clicked() {
                    enter = true;
                }
            }
        }

        ui.add_space(GAP / 2.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if icon_button_look(ui, Look::Soft, symbols::CARET_LEFT, "Move left", i > 0).clicked() {
                shift = -1;
            }
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
        });
    });

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
