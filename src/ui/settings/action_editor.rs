//! Editor for one `Action`: the kind in a drop-down, with the fields that kind needs beside it.

use crate::config::*;
use crate::keys::{Shortcut, key};
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use eframe::egui::{self, Color32};

/// The kinds in the drop-down. Order = index in `action_index`. Sequence (7, which also stands for a
/// bare wait) is only shown while it is active.
const KINDS: [(char, &str); 7] = [
    (symbols::PROHIBIT, "None"),
    (symbols::KEYBOARD, "Shortcut"),
    (symbols::APP_WINDOW, "App"),
    (symbols::LINK, "URL / file"),
    (symbols::TERMINAL_WINDOW, "Command"),
    (symbols::LIGHTNING, "Flow"),
    (symbols::BROWSER, "Browser"),
];

/// Position of the action's kind in [`KINDS`] (Sequence is 7).
fn action_index(a: &Action) -> usize {
    match a {
        Action::Nothing => 0,
        Action::Key(_) => 1,
        Action::OpenApp(_) => 2,
        Action::OpenUrl(_) => 3,
        Action::Command(_) => 4,
        Action::Flow(_) => 5,
        Action::Browser(_) => 6,
        Action::Sequence(_) | Action::Wait(_) => 7,
    }
}

/// A fresh action of kind `i` (see [`KINDS`]).
fn make_action(i: usize) -> Action {
    match i {
        1 => Action::Key(Shortcut::new(key::F13, 0)),
        2 => Action::OpenApp(String::new()),
        3 => Action::OpenUrl(String::new()),
        4 => Action::Command(String::new()),
        5 => Action::Flow(String::new()),
        6 => Action::Browser("media.toggle".into()),
        7 => Action::Sequence(Vec::new()),
        _ => Action::Nothing,
    }
}

/// Which buttons of the editor were clicked this frame.
#[derive(Default)]
pub(super) struct ActionClick {
    pub shortcut: bool,
    pub pick_app: bool,
}

/// Draws the editor for `a`. The caller reacts to the returned clicks (start recording, pick an app).
pub(super) fn action_editor(
    ui: &mut egui::Ui,
    _id: &str,
    a: &mut Action,
    recording: bool,
    accent: Color32,
    flows: &[(String, String)],
) -> ActionClick {
    let mut out = ActionClick::default();
    let now = action_index(a);

    // one line: the kind in a drop-down, and next to it what that kind needs
    let mut items: Vec<(Option<char>, &str)> = KINDS
        .iter()
        .map(|&(icon, name)| (Some(icon), name))
        .collect();
    if now == 7 {
        let label = if matches!(a, Action::Wait(_)) {
            "Wait"
        } else {
            "Sequence"
        };
        items.push((Some(symbols::STACK), label));
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if let Some(i) = select_icons(ui, 156.0, &items, now)
            && i != now
        {
            *a = make_action(i);
        }
        fields(ui, a, recording, accent, flows, &mut out);
    });
    out
}

/// What the current kind of action needs: a shortcut, an app, a link or a command.
fn fields(
    ui: &mut egui::Ui,
    a: &mut Action,
    recording: bool,
    accent: Color32,
    flows: &[(String, String)],
    out: &mut ActionClick,
) {
    match a {
        Action::Key(sc) => {
            if shortcut_button(ui, sc, recording, accent).clicked() {
                out.shortcut = true;
            }
        }
        Action::OpenApp(s) => {
            let w = (ui.available_width() - 110.0).max(120.0);
            ui.add(
                text_field(s)
                    .desired_width(w)
                    .hint_text(crate::platform::OS.open_app_hint()),
            );
            // opens the list of installed apps, the same one as "Add app" in the profile's apps
            if button(ui, Look::Soft, Some(symbols::APP_WINDOW), "Pick\u{2026}")
                .on_hover_text("Choose from the installed apps")
                .clicked()
            {
                out.pick_app = true;
            }
        }
        Action::OpenUrl(s) => {
            ui.add(
                text_field(s)
                    .desired_width(ui.available_width())
                    .hint_text("https://…  or a file path"),
            );
        }
        Action::Command(s) => {
            ui.add(
                text_field(s)
                    .desired_width(ui.available_width())
                    .hint_text("shell command"),
            );
        }
        Action::Browser(spec) => browser_fields(ui, spec),
        Action::Sequence(v) => {
            muted(
                ui,
                &format!("{} steps (edit config.json to change)", v.len()),
            );
        }
        Action::Wait(ms) => muted(ui, &format!("Wait {ms} ms (edit config.json to change)")),
        Action::Flow(id) => {
            if flows.is_empty() {
                muted(ui, "No flows yet: make one in the Flow tab.");
            } else {
                let names: Vec<&str> = flows.iter().map(|(_, name)| name.as_str()).collect();
                let now = flows.iter().position(|(fid, _)| fid == id);
                let shown = now.map_or("Choose a flow\u{2026}", |i| names[i]);
                let width = ui.available_width().max(140.0);
                if let Some(i) = select(ui, width, shown, &names, now.unwrap_or(usize::MAX)) {
                    *id = flows[i].0.clone();
                }
            }
        }
        Action::Nothing => muted(ui, "The click passes through."),
    }
}

/// The labels of the commands in the list: `Media: Next track`.
fn command_labels() -> Vec<String> {
    crate::bridge::commands::CATALOG
        .iter()
        .map(|c| format!("{}: {}", c.group.label(), c.label))
        .collect()
}

/// The editor of a "Browser" action: the command in a list, and the text it takes, if any. The
/// action holds one line, `command text`, which is also how it reads in config.json.
fn browser_fields(ui: &mut egui::Ui, spec: &mut String) {
    use crate::bridge::commands::{Arg, CATALOG, find};
    let (name, arg) = {
        // only the start is trimmed: a space typed at the end of the text is still being typed
        let t = spec.trim_start();
        t.split_once(char::is_whitespace)
            .map_or((t, ""), |(n, r)| (n, r.trim_start()))
    };
    let (name, arg) = (name.to_string(), arg.to_string());
    let Some(current) = find(&name) else {
        // a line typed by hand in config.json that is not a command: show it, so it can be fixed
        let labels = command_labels();
        let names: Vec<&str> = labels.iter().map(String::as_str).collect();
        let width = ui.available_width().max(140.0);
        if let Some(i) = select(
            ui,
            width,
            "Unknown command: choose one\u{2026}",
            &names,
            usize::MAX,
        ) {
            *spec = CATALOG[i].id.to_string();
        }
        return;
    };
    let labels = command_labels();
    let names: Vec<&str> = labels.iter().map(String::as_str).collect();
    let now = CATALOG.iter().position(|c| c.id == current.id).unwrap_or(0);
    let shown = names[now];
    let takes = !matches!(current.arg, Arg::None);
    let list_width = if takes {
        (ui.available_width() * 0.5).clamp(170.0, 300.0)
    } else {
        ui.available_width().max(170.0)
    };
    let mut new_name = name.clone();
    if let Some(i) = select(ui, list_width, shown, &names, now) {
        new_name = CATALOG[i].id.to_string();
    }
    let mut new_arg = arg;
    if new_name != name {
        // the text of the old command rarely suits the new one
        new_arg.clear();
    }
    if let Some(c) = find(&new_name)
        && let Arg::Optional(hint) | Arg::Required(hint) = c.arg
    {
        let hint = if matches!(c.arg, Arg::Optional(_)) {
            format!("{hint} (optional)")
        } else {
            hint.to_string()
        };
        ui.add(
            text_field(&mut new_arg)
                .desired_width(ui.available_width())
                .hint_text(hint),
        );
    }
    *spec = if new_arg.trim().is_empty() {
        new_name
    } else {
        format!("{new_name} {}", new_arg.trim_start())
    };
}
