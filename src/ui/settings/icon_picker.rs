//! Icon picker panel: built-in catalog (`icon_catalog`) + images from the icons folder + free text.
//!
//! All widths are derived from `w` (the panel width given by the caller), not from
//! `available_width()`, and the grid is laid out by hand, row by row. That way nothing can
//! spill out of the card even if egui gets the remaining width wrong.
//!
//! The grid is **virtualized**: `ScrollArea::show_rows` only runs the closure for the rows that
//! are on screen, so a folder with 120 images costs the same as one with 12, and only the visible
//! images are ever turned into textures. Those loads are also spread over several frames
//! ([`LOADS_PER_FRAME`]) so opening the "Mine" page never freezes the window.

use super::icon_catalog::{self, CATEGORIES, IconDef};
use super::state::Ed;
use crate::ui::draw::*;
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use eframe::egui::{self, Align2, Color32, FontId, Vec2};

/// What happened in the picker this frame.
pub(super) struct Picked {
    pub chosen: Option<String>,
    pub pick_file: bool,
}

/// Most image files decoded in one frame while the grid fills up. Tiles still waiting show "…"
/// and are loaded on the following frames (the picker asks for them with `request_repaint`).
const LOADS_PER_FRAME: usize = 6;

/// One tile of the grid. A user image is an index into `Ed::user_icons`, so building the list of
/// tiles (every frame) does not clone a single path.
enum Cell {
    Glyph(&'static IconDef),
    Mine(usize),
}

/// Draws the picker in a panel `w` pixels wide; `icon` is the icon being edited.
pub(super) fn icon_picker(
    ui: &mut egui::Ui,
    ed: &mut Ed,
    ctx: &egui::Context,
    icon: &mut String,
    accent: Color32,
    w: f32,
) -> Picked {
    let mut out = Picked {
        chosen: None,
        pick_file: false,
    };
    let mine = CATEGORIES.len() + 1;

    // row 1: search, and one "+" that opens a small menu for adding your own images
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.add(
            text_field(&mut ed.icon_query)
                .desired_width(w - CONTROL_H - 8.0)
                .hint_text("Search icons"),
        );
        let add = icon_button_look(ui, Look::Soft, symbols::PLUS, "Add your own icons", true);
        menu_popup(&add, 232.0, |ui| {
            if menu_item(ui, symbols::IMAGE, "Choose an image\u{2026}").clicked() {
                out.pick_file = true;
            }
            if menu_item(ui, symbols::FOLDER_OPEN, "Open the icons folder").clicked() {
                icon_catalog::open_icons_folder();
            }
            if menu_item(ui, symbols::ARROW_CLOCKWISE, "Reload the icons folder").clicked() {
                ed.reload_user_icons();
            }
        });
    });

    // row 2: the groups, as icons sharing the width (the name of each is its tooltip)
    let mut groups: Vec<(char, &str)> = vec![(symbols::SQUARES_FOUR, "All")];
    groups.extend(CATEGORIES.iter().map(|c| (c.icon, c.title)));
    groups.push((symbols::IMAGE, "Mine (icons folder)"));
    if let Some(k) = icon_tabs(ui, &groups, ed.icon_cat, accent, w) {
        ed.icon_cat = k;
        if k == mine {
            ed.reload_user_icons();
        }
    }

    // row 4: free text
    ui.add(
        text_field(icon)
            .desired_width(w)
            .hint_text("The icon, or type text / emoji"),
    );
    ui.add_space(GAP / 2.0);

    // grid content
    let q = ed.icon_query.trim().to_lowercase();
    let mut cells: Vec<Cell> = Vec::new();
    if ed.icon_cat != mine {
        for d in icon_catalog::filtered(&q, ed.icon_cat) {
            cells.push(Cell::Glyph(d));
        }
    }
    if ed.icon_cat == 0 || ed.icon_cat == mine {
        for (i, p) in ed.user_icons.iter().enumerate() {
            let file = p.rsplit('/').next().unwrap_or(p);
            if q.is_empty() || file.to_lowercase().contains(&q) {
                cells.push(Cell::Mine(i));
            }
        }
    }

    if ed.icon_cat == mine && ed.icons_not_listed > 0 {
        muted(
            ui,
            &format!(
                "{} image(s) in the icons folder are not listed (over 2 MB, or more than {}). \
                 Choose a big one with Image… and it is shrunk for you.",
                ed.icons_not_listed,
                icon_catalog::MAX_USER_ICONS
            ),
        );
    }

    if cells.is_empty() {
        muted(
            ui,
            if ed.icon_cat == mine {
                "Drop image files here, or put them in the icons folder and press Reload."
            } else {
                "No icons found. You can also drop an image file here."
            },
        );
        return out;
    }

    let (tile_w, gap) = (54.0_f32, 10.0_f32);
    let cols = (((w - 12.0) + gap) / (tile_w + gap)).floor().max(1.0) as usize;
    let total_rows = cells.len().div_ceil(cols);
    let h = ui.available_height().max(80.0);
    let mut loads_left = LOADS_PER_FRAME;
    let mut pending = false;
    // `show_rows` takes the row distance from the spacing of the Ui it is called on, so the gap is
    // set on a scope around the scroll area (nothing else is drawn after the grid).
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(gap);
        egui::ScrollArea::vertical()
            .id_salt("icon_grid")
            .max_height(h)
            .auto_shrink([false, false])
            .show_rows(ui, tile_w, total_rows, |ui, rows| {
                ui.set_max_width(w);
                for row in rows {
                    let from = row * cols;
                    let to = (from + cols).min(cells.len());
                    ui.horizontal(|ui| {
                        for cell in &cells[from..to] {
                            match cell {
                                Cell::Glyph(def) => {
                                    let (rect, resp) = tile(
                                        ui,
                                        Vec2::splat(tile_w),
                                        icon.as_str() == def.icon,
                                        accent,
                                    );
                                    draw_icon(
                                        ui.painter(),
                                        None,
                                        def.icon,
                                        rect.center(),
                                        26.0,
                                        pie_colors().ink,
                                    );
                                    if resp.on_hover_text(def.name).clicked() {
                                        out.chosen = Some(def.icon.to_string());
                                    }
                                }
                                Cell::Mine(i) => {
                                    // only the rows on screen get here: one clone per visible tile
                                    let path = ed.user_icons[*i].clone();
                                    let file = path.rsplit('/').next().unwrap_or(path.as_str());
                                    let loaded = ed.icons.contains(&path);
                                    let waiting = !loaded && loads_left == 0;
                                    let tex = if waiting {
                                        None
                                    } else {
                                        if !loaded {
                                            loads_left -= 1;
                                        }
                                        ed.icon_for(ctx, &path)
                                    };
                                    let (rect, resp) =
                                        tile(ui, Vec2::splat(tile_w), *icon == path, accent);
                                    if waiting {
                                        pending = true;
                                        ui.painter().text(
                                            rect.center(),
                                            Align2::CENTER_CENTER,
                                            "\u{2026}",
                                            FontId::proportional(18.0),
                                            pal().muted,
                                        );
                                    } else {
                                        draw_icon(
                                            ui.painter(),
                                            tex.as_ref(),
                                            &path,
                                            rect.center(),
                                            34.0,
                                            pal().text,
                                        );
                                    }
                                    if resp.on_hover_text(file).clicked() {
                                        out.chosen = Some(path.clone());
                                    }
                                }
                            }
                        }
                    });
                }
            });
    });
    if pending {
        ctx.request_repaint(); // the tiles that had to wait are loaded on the next frame
    }

    out
}
