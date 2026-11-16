//! The rules of the **panel designer**: how a panel is built by pointing and clicking instead of by
//! typing JSON (`ui::settings::panel_designer` draws it, this file decides).
//!
//! The designer holds a [`PanelSpec`] that is *not* checked yet, because a panel that is half made
//! (an empty id, two boxes with the same id) is still a panel being made, and because the texts may
//! hold `{{variables}}` of a flow. Everything it does to that spec is a plain function here, so each
//! rule has a test and none of them needs a window:
//!
//! | Rule | Function |
//! |---|---|
//! | what can be added, and what it starts as | [`Kind`], [`new_widget`] |
//! | order, copies, removal | [`insert_widget`], [`duplicate_widget`], [`move_widget`], [`remove_widget`] |
//! | a new id that is free, and renaming one without breaking a `show_if` | [`unique_id`], [`rename_widget`] |
//! | the options of a list as lines of text | [`options_to_text`], [`text_to_options`] |
//! | ready-made panels to start from | [`TEMPLATES`], [`template_spec`] |
//! | to and from the text a flow block keeps | [`from_flow_text`], [`to_flow_text`] |
//! | several widgets at once: copy, paste, remove, move | [`copy_widgets`], [`paste_widgets`], [`remove_widgets`], [`move_widgets`] |

use super::spec::{
    Button, Icon, Layout, MAX_BUTTONS, MAX_OPTIONS, MAX_WIDGETS, Opt, Padding, PanelSpec, ShowIf,
    TextFmt, TextStyle, Tone, Widget,
};
use crate::config::flow::MAX_PANEL_CHARS;
use std::collections::{BTreeMap, BTreeSet};

/// Something the designer can add to a panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Heading,
    Text,
    Input,
    Multiline,
    Number,
    Toggle,
    Slider,
    Color,
    Dropdown,
    List,
    Chips,
    Multi,
    Progress,
    Separator,
    /// A grid of buttons with icons.
    Buttons,
    /// Empty room.
    Spacer,
}

impl Kind {
    /// Everything that can be added, in the order of the palette.
    pub const ALL: [Kind; 16] = [
        Kind::Heading,
        Kind::Text,
        Kind::Input,
        Kind::Multiline,
        Kind::Number,
        Kind::Toggle,
        Kind::Slider,
        Kind::Color,
        Kind::Dropdown,
        Kind::List,
        Kind::Chips,
        Kind::Multi,
        Kind::Progress,
        Kind::Separator,
        Kind::Buttons,
        Kind::Spacer,
    ];

    /// The name in the palette.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Heading => "Heading",
            Kind::Text => "Text",
            Kind::Input => "Text box",
            Kind::Multiline => "Notes box",
            Kind::Number => "Number",
            Kind::Toggle => "Switch",
            Kind::Slider => "Slider",
            Kind::Color => "Color",
            Kind::Dropdown => "Dropdown",
            Kind::List => "Pick one",
            Kind::Chips => "Chips",
            Kind::Multi => "Pick several",
            Kind::Progress => "Progress",
            Kind::Separator => "Line",
            Kind::Buttons => "Button grid",
            Kind::Spacer => "Space",
        }
    }

    /// What the answer of a field of this kind is, for the hint under the name.
    pub fn hint(self) -> &'static str {
        match self {
            Kind::Heading | Kind::Text | Kind::Progress | Kind::Separator | Kind::Spacer => {
                "Shows something"
            }
            Kind::Buttons => "Answer: the button pressed",
            Kind::Input | Kind::Multiline | Kind::Color => "Answer: text",
            Kind::Number | Kind::Slider => "Answer: a number",
            Kind::Toggle => "Answer: on or off",
            Kind::Dropdown | Kind::List | Kind::Chips => "Answer: one option",
            Kind::Multi => "Answer: a list",
        }
    }

    /// The id a new field of this kind starts with (followed by a number when taken).
    fn base_id(self) -> &'static str {
        match self {
            Kind::Input => "text",
            Kind::Multiline => "notes",
            Kind::Number => "number",
            Kind::Toggle => "switch",
            Kind::Slider => "level",
            Kind::Color => "color",
            Kind::Dropdown | Kind::List | Kind::Chips => "choice",
            Kind::Multi => "choices",
            Kind::Buttons => "buttons",
            Kind::Heading | Kind::Text | Kind::Progress | Kind::Separator | Kind::Spacer => "item",
        }
    }
}

/// The kind of a widget, to pick its icon and its name in the list.
pub fn kind_of(widget: &Widget) -> Kind {
    match widget {
        Widget::Text {
            style: TextStyle::Heading,
            ..
        } => Kind::Heading,
        Widget::Text { .. } => Kind::Text,
        Widget::Separator { .. } => Kind::Separator,
        Widget::Input {
            multiline: true, ..
        } => Kind::Multiline,
        Widget::Input { .. } => Kind::Input,
        Widget::Number { .. } => Kind::Number,
        Widget::Toggle { .. } => Kind::Toggle,
        Widget::Slider { .. } => Kind::Slider,
        Widget::Color { .. } => Kind::Color,
        Widget::Choice {
            layout: Layout::Dropdown,
            ..
        } => Kind::Dropdown,
        Widget::Choice {
            layout: Layout::Chips,
            ..
        } => Kind::Chips,
        Widget::Choice { .. } => Kind::List,
        Widget::Multi { .. } => Kind::Multi,
        Widget::Progress { .. } => Kind::Progress,
        Widget::Buttons { .. } => Kind::Buttons,
        Widget::Spacer { .. } => Kind::Spacer,
    }
}

/// Every id of a button of `spec`, at the bottom or in a grid.
pub fn taken_button_ids(spec: &PanelSpec) -> BTreeSet<String> {
    spec.all_buttons().map(|b| b.id.clone()).collect()
}

/// Every id used by a widget of `spec`.
pub fn taken_ids(spec: &PanelSpec) -> BTreeSet<String> {
    spec.widgets
        .iter()
        .filter_map(Widget::id)
        .map(str::to_string)
        .collect()
}

/// `base` when no widget has it, else `base2`, `base3`, ...
pub fn unique_id(base: &str, taken: &BTreeSet<String>) -> String {
    if !taken.contains(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}{n}"))
        .find(|id| !taken.contains(id))
        .unwrap_or_else(|| base.to_string())
}

fn opts(values: &[&str]) -> Vec<Opt> {
    values.iter().map(|v| Opt::new(*v)).collect()
}

/// A new widget of `kind`, with sensible contents and an id that no widget of `taken` has.
pub fn new_widget(kind: Kind, taken: &BTreeSet<String>) -> Widget {
    let id = || unique_id(kind.base_id(), taken);
    match kind {
        Kind::Heading => Widget::Text {
            id: String::new(),
            text: "Heading".into(),
            style: TextStyle::Heading,
            fmt: TextFmt::default(),
            show_if: None,
        },
        Kind::Text => Widget::Text {
            id: String::new(),
            text: "Some text".into(),
            style: TextStyle::Body,
            fmt: TextFmt::default(),
            show_if: None,
        },
        Kind::Separator => Widget::Separator { show_if: None },
        Kind::Input | Kind::Multiline => Widget::Input {
            id: id(),
            label: "Label".into(),
            hint: String::new(),
            value: String::new(),
            secret: false,
            multiline: kind == Kind::Multiline,
            required: false,
            max_chars: None,
            value_fmt: TextFmt::default(),
            width: None,
            height: None,
            text_padding: Padding::default(),
            show_if: None,
        },
        Kind::Number => Widget::Number {
            id: id(),
            label: "Amount".into(),
            value: Some(1.0),
            min: Some(0.0),
            max: Some(100.0),
            step: Some(1.0),
            unit: String::new(),
            show_if: None,
        },
        Kind::Toggle => Widget::Toggle {
            id: id(),
            label: "Turn on".into(),
            value: false,
            show_if: None,
        },
        Kind::Slider => Widget::Slider {
            id: id(),
            label: "Level".into(),
            min: 0.0,
            max: 100.0,
            step: Some(1.0),
            value: Some(50.0),
            unit: "%".into(),
            show_if: None,
        },
        Kind::Color => Widget::Color {
            id: id(),
            label: "Color".into(),
            value: super::spec::DEFAULT_COLOR.into(),
            show_if: None,
        },
        Kind::Dropdown | Kind::List | Kind::Chips => Widget::Choice {
            id: id(),
            label: "Choose one".into(),
            options: opts(&["First", "Second", "Third"]),
            value: "First".into(),
            layout: match kind {
                Kind::Dropdown => Layout::Dropdown,
                Kind::Chips => Layout::Chips,
                _ => Layout::List,
            },
            submit: false,
            search: None,
            show_if: None,
        },
        Kind::Multi => Widget::Multi {
            id: id(),
            label: "Choose any".into(),
            options: opts(&["First", "Second", "Third"]),
            values: Vec::new(),
            show_if: None,
        },
        Kind::Progress => Widget::Progress {
            id: String::new(),
            label: "Working".into(),
            value: -1.0,
            show_if: None,
        },
        Kind::Buttons => {
            // the ids of the buttons come from the id of the grid, which is free, so they are too
            let grid = id();
            let button = |n: u8, label: &str, icon: &str| Button {
                id: format!("{grid}_{n}"),
                label: label.into(),
                icon: icon.into(),
                ..Button::default()
            };
            Widget::Buttons {
                id: grid.clone(),
                label: String::new(),
                columns: 0,
                buttons: vec![
                    button(1, "Previous", "skip-back"),
                    button(2, "", "play"),
                    button(3, "Next", "skip-forward"),
                ],
                align: None,
                show_if: None,
            }
        }
        Kind::Spacer => Widget::Spacer {
            height: Some(super::spec::SPACER_DEFAULT),
            show_if: None,
        },
    }
}

/// Puts `widget` at place `at` (the end when it is too far). Returns where it went, or `None` when
/// the panel is full.
pub fn insert_widget(spec: &mut PanelSpec, at: usize, widget: Widget) -> Option<usize> {
    if spec.widgets.len() >= MAX_WIDGETS {
        return None;
    }
    let at = at.min(spec.widgets.len());
    spec.widgets.insert(at, widget);
    Some(at)
}

/// A copy of widget `index` right after it, with an id of its own. Returns where the copy is.
pub fn duplicate_widget(spec: &mut PanelSpec, index: usize) -> Option<usize> {
    let mut copy = spec.widgets.get(index)?.clone();
    if let Some(old) = copy.id() {
        let taken = taken_ids(spec);
        let base = old.trim_end_matches(|c: char| c.is_ascii_digit());
        let base = if base.is_empty() { old } else { base };
        copy.set_id(unique_id(base, &taken));
    }
    if let Widget::Buttons { buttons, .. } = &mut copy {
        // the buttons of the copy answer with ids of their own too
        let mut taken = taken_button_ids(spec);
        for b in buttons.iter_mut() {
            let base =
                b.id.trim_end_matches(|c: char| c.is_ascii_digit())
                    .to_string();
            let base = if base.is_empty() { b.id.clone() } else { base };
            b.id = unique_id(&base, &taken);
            taken.insert(b.id.clone());
        }
    }
    insert_widget(spec, index + 1, copy)
}

/// What the text of copied widgets starts with on the clipboard (the widgets follow as JSON), so a
/// paste can tell them from any other text.
pub const CLIP_WIDGETS: &str = "cursorpie-panel-widgets:";

/// The most a paste reads: a bigger clipboard text is not a copy of widgets.
const CLIP_MAX_BYTES: usize = 1_000_000;

/// What [`paste_widgets`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PasteWidgets {
    /// The text is not a copy of widgets: nothing happened.
    NotWidgets,
    /// The panel has no room for the copies: nothing happened.
    Full,
    /// The copies were added, at these places.
    Done(Vec<usize>),
}

/// `old` when no id of `taken` is it, else the first free `base2`, `base3`, ... (the digits at the
/// end of `old` are not part of the base, so `toggle2` becomes `toggle3` and not `toggle22`).
fn fresh_like(old: &str, taken: &BTreeSet<String>) -> String {
    if !taken.contains(old) {
        return old.to_string();
    }
    let base = old.trim_end_matches(|c: char| c.is_ascii_digit());
    let base = if base.is_empty() { old } else { base };
    unique_id(base, taken)
}

/// The text for the clipboard when the widgets at `indices` are copied: [`CLIP_WIDGETS`] and then
/// the widgets as JSON, in the order they have in the panel. `None` when there is nothing to copy.
pub fn copy_widgets(spec: &PanelSpec, indices: &[usize]) -> Option<String> {
    let mut at: Vec<usize> = indices
        .iter()
        .copied()
        .filter(|i| *i < spec.widgets.len())
        .collect();
    at.sort_unstable();
    at.dedup();
    if at.is_empty() {
        return None;
    }
    let widgets: Vec<&Widget> = at.iter().map(|i| &spec.widgets[*i]).collect();
    let json = serde_json::to_string(&widgets).ok()?;
    Some(format!("{CLIP_WIDGETS}{json}"))
}

/// Puts the widgets of a copy ([`copy_widgets`]) into `spec` from place `at` on (the end when it is
/// too far). Each copy keeps its id unless a widget of the panel has it, and then gets one of its
/// own, like the buttons of a grid; a `show_if` follows the widget it looked at when that was
/// copied too, stays when the panel has a widget with that id, and is dropped otherwise (it would
/// name a field that is not there). A group is pasted whole or not at all.
pub fn paste_widgets(spec: &mut PanelSpec, at: usize, text: &str) -> PasteWidgets {
    if text.len() > CLIP_MAX_BYTES {
        return PasteWidgets::NotWidgets;
    }
    let Some(json) = text.trim_start().strip_prefix(CLIP_WIDGETS) else {
        return PasteWidgets::NotWidgets;
    };
    let Ok(mut widgets) = serde_json::from_str::<Vec<Widget>>(json.trim()) else {
        return PasteWidgets::NotWidgets;
    };
    if widgets.is_empty() {
        return PasteWidgets::NotWidgets;
    }
    if spec.widgets.len() + widgets.len() > MAX_WIDGETS {
        return PasteWidgets::Full;
    }
    let mut taken = taken_ids(spec);
    let mut taken_buttons = taken_button_ids(spec);
    let mut renamed: BTreeMap<String, String> = BTreeMap::new();
    for w in &mut widgets {
        if let Some(old) = w.id().map(str::to_string) {
            let new = fresh_like(&old, &taken);
            taken.insert(new.clone());
            renamed.entry(old).or_insert_with(|| new.clone());
            w.set_id(new);
        }
        if let Widget::Buttons { buttons, .. } = w {
            for b in buttons.iter_mut() {
                let new = fresh_like(&b.id, &taken_buttons);
                taken_buttons.insert(new.clone());
                b.id = new;
            }
        }
    }
    for w in &mut widgets {
        if let Some(mut cond) = w.show_if().cloned() {
            if let Some(new) = renamed.get(&cond.id) {
                cond.id.clone_from(new);
            }
            let keep = taken.contains(&cond.id) && w.id() != Some(cond.id.as_str());
            w.set_show_if(keep.then_some(cond));
        }
    }
    let at = at.min(spec.widgets.len());
    let count = widgets.len();
    for (k, w) in widgets.into_iter().enumerate() {
        spec.widgets.insert(at + k, w);
    }
    PasteWidgets::Done((at..at + count).collect())
}

/// A copy of the widgets at `indices` right after the last of them, with ids of their own.
/// Returns where the copies are, or `None` when there is nothing to copy or the panel is full.
pub fn duplicate_widgets(spec: &mut PanelSpec, indices: &[usize]) -> Option<Vec<usize>> {
    let text = copy_widgets(spec, indices)?;
    let last = indices
        .iter()
        .copied()
        .filter(|i| *i < spec.widgets.len())
        .max()?;
    match paste_widgets(spec, last + 1, &text) {
        PasteWidgets::Done(at) => Some(at),
        _ => None,
    }
}

/// Removes the widgets at `indices` (and every `show_if` that waited for one of them, see
/// [`remove_widget`]). Returns what to select next: the widget now at the place of the first one
/// removed, or the last one, or `None` for an empty panel.
pub fn remove_widgets(spec: &mut PanelSpec, indices: &[usize]) -> Option<usize> {
    let mut at: Vec<usize> = indices
        .iter()
        .copied()
        .filter(|i| *i < spec.widgets.len())
        .collect();
    at.sort_unstable();
    at.dedup();
    let first = *at.first()?;
    for i in at.into_iter().rev() {
        remove_widget(spec, i);
    }
    (!spec.widgets.is_empty()).then(|| first.min(spec.widgets.len() - 1))
}

/// Moves the widgets at `indices` up (`-1`) or down (`1`) by one place, together: the ones that
/// are not next to each other stay apart, and a group that is at the end already stays where it
/// is. Returns where they are now, or `None` when nothing moved.
pub fn move_widgets(spec: &mut PanelSpec, indices: &[usize], delta: isize) -> Option<Vec<usize>> {
    let len = spec.widgets.len();
    let mut on = vec![false; len];
    for i in indices {
        if *i < len {
            on[*i] = true;
        }
    }
    let mut moved = false;
    if delta < 0 {
        for i in 1..len {
            if on[i] && !on[i - 1] {
                spec.widgets.swap(i, i - 1);
                on.swap(i, i - 1);
                moved = true;
            }
        }
    } else {
        for i in (0..len.saturating_sub(1)).rev() {
            if on[i] && !on[i + 1] {
                spec.widgets.swap(i, i + 1);
                on.swap(i, i + 1);
                moved = true;
            }
        }
    }
    moved.then(|| (0..len).filter(|i| on[*i]).collect())
}

/// Where an item that is dragged from `from` ends up when it is dropped on the item `over`, in the
/// upper half of it (`after` false) or in the lower half (`after` true), in a list of `len` items.
/// `None` when it would not move.
pub fn drop_target(len: usize, from: usize, over: usize, after: bool) -> Option<usize> {
    if from >= len || over >= len {
        return None;
    }
    let mut to = if after { over + 1 } else { over };
    if from < to {
        to -= 1;
    }
    (to != from).then_some(to)
}

/// Moves the item `from` of `items` so that it ends up at place `to` (see [`drop_target`]).
pub fn move_item<T>(items: &mut Vec<T>, from: usize, to: usize) -> bool {
    if from >= items.len() || to >= items.len() || from == to {
        return false;
    }
    let item = items.remove(from);
    items.insert(to, item);
    true
}

/// Drags widget `from` onto widget `over` (above it, or below it with `after`). Returns where the
/// widget is now, or `None` when nothing moved.
pub fn drop_widget(spec: &mut PanelSpec, from: usize, over: usize, after: bool) -> Option<usize> {
    let to = drop_target(spec.widgets.len(), from, over, after)?;
    move_item(&mut spec.widgets, from, to).then_some(to)
}

/// Moves widget `index` up (`-1`) or down (`1`). Returns where it is now, or `None` when it is
/// already at that end.
pub fn move_widget(spec: &mut PanelSpec, index: usize, delta: isize) -> Option<usize> {
    let to = index.checked_add_signed(delta)?;
    if index >= spec.widgets.len() || to >= spec.widgets.len() {
        return None;
    }
    spec.widgets.swap(index, to);
    Some(to)
}

/// Removes widget `index`, and every `show_if` that waited for it (the widgets stay, always shown).
/// Returns what to select next: the widget now at that place, or the last one, or `None` for an
/// empty panel.
pub fn remove_widget(spec: &mut PanelSpec, index: usize) -> Option<usize> {
    if index >= spec.widgets.len() {
        return None;
    }
    let gone = spec.widgets.remove(index);
    if let Some(id) = gone.id() {
        for w in &mut spec.widgets {
            if w.show_if().is_some_and(|c| c.id == id) {
                w.set_show_if(None);
            }
        }
    }
    (!spec.widgets.is_empty()).then(|| index.min(spec.widgets.len() - 1))
}

/// Gives widget `index` the id `new`. The widgets that wait for it (`show_if`) follow it, so a
/// rename never breaks a condition.
pub fn rename_widget(spec: &mut PanelSpec, index: usize, new: &str) {
    let Some(widget) = spec.widgets.get(index) else {
        return;
    };
    if matches!(widget, Widget::Separator { .. } | Widget::Spacer { .. }) {
        return;
    }
    // a text or a progress bar may have no id yet: nothing waits for it, but it can get one
    let old = widget.id().unwrap_or_default().to_string();
    if old == new {
        return;
    }
    for w in &mut spec.widgets {
        if let Some(c) = w.show_if().filter(|c| !old.is_empty() && c.id == old) {
            let renamed = ShowIf {
                id: new.to_string(),
                ..c.clone()
            };
            w.set_show_if(Some(renamed));
        }
    }
    spec.widgets[index].set_id(new.to_string());
}

/// The ids of the fields that widget `index` could wait for: every other field, which is what the
/// "Show only when" list offers. A field that waits for this widget (directly or not) is left out,
/// so the list can never offer a loop.
pub fn conditions_for(spec: &PanelSpec, index: usize) -> Vec<String> {
    let me = spec
        .widgets
        .get(index)
        .and_then(Widget::id)
        .map(str::to_string);
    spec.widgets
        .iter()
        .enumerate()
        .filter(|(i, w)| *i != index && w.is_field())
        .filter_map(|(_, w)| w.id())
        .filter(|id| !waits_for(spec, id, me.as_deref(), 0))
        .map(str::to_string)
        .collect()
}

/// Whether the field `from` waits for the field `target`, directly or through others.
fn waits_for(spec: &PanelSpec, from: &str, target: Option<&str>, depth: usize) -> bool {
    let Some(target) = target else { return false };
    if depth > MAX_WIDGETS {
        return true;
    }
    let Some(c) = spec
        .widgets
        .iter()
        .find(|w| w.id() == Some(from))
        .and_then(Widget::show_if)
    else {
        return false;
    };
    c.id == target || waits_for(spec, &c.id, Some(target), depth + 1)
}

/// The options of a list as text, a line each: `value`, `value | Label` or `value | Label | hint`.
pub fn options_to_text(options: &[Opt]) -> String {
    options
        .iter()
        .map(|o| {
            let label = if o.label.is_empty() {
                &o.value
            } else {
                &o.label
            };
            match (label == &o.value, o.hint.is_empty()) {
                (true, true) => o.value.clone(),
                (false, true) => format!("{} | {}", o.value, label),
                _ => format!("{} | {} | {}", o.value, label, o.hint),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The lines of `text` as options (see [`options_to_text`]). Empty lines are skipped, and so is
/// everything past [`MAX_OPTIONS`].
pub fn text_to_options(text: &str) -> Vec<Opt> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '|').map(str::trim);
            let value = parts.next().filter(|v| !v.is_empty())?;
            Some(Opt {
                value: value.to_string(),
                label: parts.next().unwrap_or_default().to_string(),
                hint: parts.next().unwrap_or_default().to_string(),
            })
        })
        .take(MAX_OPTIONS)
        .collect()
}

/// A button that is free to add, or `None` when the panel has all it may.
pub fn new_button(spec: &PanelSpec) -> Option<Button> {
    if spec.buttons.len() >= MAX_BUTTONS {
        return None;
    }
    let taken: BTreeSet<String> = spec.buttons.iter().map(|b| b.id.clone()).collect();
    Some(Button {
        id: unique_id("button", &taken),
        label: "Button".into(),
        tone: Tone::Secondary,
        cancel: false,
        ..Button::default()
    })
}

/// Whether the panel has no button of its own and did not ask for none, so it gets the default
/// ones (Cancel and OK).
pub fn uses_default_buttons(spec: &PanelSpec) -> bool {
    spec.buttons.is_empty() && !spec.no_buttons
}

/// A line for the list of widgets: what the person would call it.
pub fn summary(widget: &Widget) -> String {
    let label = match widget {
        Widget::Text { text, .. } => text.as_str(),
        Widget::Separator { .. } => "",
        Widget::Input { label, id, .. }
        | Widget::Number { label, id, .. }
        | Widget::Toggle { label, id, .. }
        | Widget::Slider { label, id, .. }
        | Widget::Color { label, id, .. }
        | Widget::Choice { label, id, .. }
        | Widget::Multi { label, id, .. } => {
            if label.is_empty() {
                id.as_str()
            } else {
                label.as_str()
            }
        }
        Widget::Progress { label, .. } | Widget::Buttons { label, .. } => label.as_str(),
        Widget::Spacer { .. } => "",
    };
    let first = label.lines().next().unwrap_or_default();
    if first.chars().count() > 34 {
        let head: String = first.chars().take(33).collect();
        format!("{}\u{2026}", head.trim_end())
    } else {
        first.to_string()
    }
}

/// A ready-made panel to start from.
pub struct Template {
    pub name: &'static str,
    pub hint: &'static str,
    pub icon: Icon,
    pub json: &'static str,
}

/// The panels offered by the designer's **Start from** list.
pub const TEMPLATES: &[Template] = &[
    Template {
        name: "Ask for text",
        hint: "A question and a box to answer in.",
        icon: Icon::Flow,
        json: r#"{
  "title": "Question",
  "widgets": [
    { "type": "input", "id": "answer", "label": "Your answer", "required": true }
  ],
  "buttons": [
    { "id": "cancel", "label": "Cancel", "cancel": true },
    { "id": "ok", "label": "OK", "tone": "primary" }
  ]
}"#,
    },
    Template {
        name: "Confirm",
        hint: "A yes or no before something that cannot be undone.",
        icon: Icon::Command,
        json: r#"{
  "title": "Are you sure?",
  "widgets": [
    { "type": "text", "text": "This cannot be undone." }
  ],
  "buttons": [
    { "id": "cancel", "label": "Cancel", "cancel": true },
    { "id": "yes", "label": "Yes, do it", "tone": "danger" }
  ]
}"#,
    },
    Template {
        name: "Menu",
        hint: "A list that answers as soon as one entry is picked.",
        icon: Icon::App,
        json: r#"{
  "title": "Pick one",
  "place": "center",
  "widgets": [
    { "type": "choice", "id": "pick", "submit": true,
      "options": ["First", { "value": "second", "label": "Second", "hint": "with a second line" }, "Third"] }
  ]
}"#,
    },
    Template {
        name: "Settings form",
        hint: "Several kinds of fields, one of them shown only when asked for.",
        icon: Icon::Shortcut,
        json: r##"{
  "title": "Options",
  "subtitle": "Change what you need",
  "widgets": [
    { "type": "input", "id": "name", "label": "Name", "required": true },
    { "type": "choice", "id": "size", "label": "Size", "layout": "dropdown", "options": ["small", "medium", "large"], "value": "medium" },
    { "type": "number", "id": "copies", "label": "Copies", "min": 1, "max": 20, "value": 1 },
    { "type": "toggle", "id": "advanced", "label": "Show more options" },
    { "type": "slider", "id": "quality", "label": "Quality", "min": 0, "max": 100, "step": 5, "value": 80, "unit": "%",
      "show_if": { "id": "advanced" } },
    { "type": "color", "id": "tint", "label": "Color", "value": "#3b82f6",
      "show_if": { "id": "advanced" } }
  ],
  "buttons": [
    { "id": "cancel", "label": "Cancel", "cancel": true },
    { "id": "go", "label": "Apply", "tone": "primary" }
  ]
}"##,
    },
    Template {
        name: "Button pad",
        hint: "A grid of buttons with icons that stays open while a flow answers each press.",
        icon: Icon::Browser,
        json: r#"{
  "id": "pad",
  "title": "Remote",
  "keep_open": true,
  "buttons_align": "stretch",
  "widgets": [
    { "type": "text", "text": "Now playing", "style": "heading" },
    { "type": "buttons", "columns": 3, "buttons": [
      { "id": "prev", "icon": "skip-back" },
      { "id": "play_pause", "icon": "play", "tone": "primary" },
      { "id": "next", "icon": "skip-forward" },
      { "id": "back10", "label": "10 s", "icon": "rewind" },
      { "id": "mute", "icon": "speaker-slash" },
      { "id": "fwd10", "label": "10 s", "icon": "fast-forward" }
    ] }
  ],
  "no_buttons": true
}"#,
    },
    Template {
        name: "Progress",
        hint: "A bar that a flow or the extension moves while it works.",
        icon: Icon::Browser,
        json: r#"{
  "id": "job",
  "title": "Working",
  "widgets": [
    { "type": "progress", "id": "bar", "label": "Please wait" }
  ],
  "buttons": [
    { "id": "cancel", "label": "Cancel", "cancel": true }
  ]
}"#,
    },
];

/// Template `index` as a spec that has not been checked (the designer keeps it that way).
pub fn template_spec(index: usize) -> Option<PanelSpec> {
    serde_json::from_str(TEMPLATES.get(index)?.json).ok()
}

/// Reads the text a flow block keeps. An empty text is an empty panel. The spec is *not* checked
/// (see the module documentation); `Err` says why the text is not a panel at all.
pub fn from_flow_text(text: &str) -> Result<PanelSpec, String> {
    if text.trim().is_empty() {
        return Ok(PanelSpec::default());
    }
    serde_json::from_str(text).map_err(|e| e.to_string())
}

/// The text a flow block keeps for `spec`: indented, or without indentation when that is what makes
/// it fit the room a block has ([`MAX_PANEL_CHARS`]). `Err` when even that is too long.
pub fn to_flow_text(spec: &PanelSpec) -> Result<String, String> {
    let pretty = serde_json::to_string_pretty(spec).map_err(|e| e.to_string())?;
    if pretty.chars().count() <= MAX_PANEL_CHARS {
        return Ok(pretty);
    }
    let compact = serde_json::to_string(spec).map_err(|e| e.to_string())?;
    if compact.chars().count() <= MAX_PANEL_CHARS {
        return Ok(compact);
    }
    Err(format!(
        "The panel is longer than a block can hold ({} characters): shorten some texts or remove a widget",
        MAX_PANEL_CHARS
    ))
}

/// `Ok` when the panel as it is could be shown by a flow, else the reason, in words for the person.
pub fn problem(spec: &PanelSpec) -> Option<String> {
    spec.clone().checked().err().map(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(spec: &PanelSpec) -> Vec<String> {
        spec.widgets
            .iter()
            .map(|w| w.id().unwrap_or("-").to_string())
            .collect()
    }

    fn spec(json: &str) -> PanelSpec {
        serde_json::from_str(json).expect("json")
    }

    #[test]
    fn everything_that_can_be_added_makes_a_valid_widget_with_a_free_id() {
        let mut s = PanelSpec::default();
        for kind in Kind::ALL {
            let at = s.widgets.len();
            let w = new_widget(kind, &taken_ids(&s));
            assert_eq!(kind_of(&w), kind, "{kind:?}");
            insert_widget(&mut s, at, w).expect("room");
        }
        // twice over: still valid, ids still unique
        for kind in Kind::ALL {
            if s.widgets.len() < MAX_WIDGETS {
                let at = s.widgets.len();
                let w = new_widget(kind, &taken_ids(&s));
                insert_widget(&mut s, at, w);
            }
        }
        let checked = s.clone().checked();
        assert!(checked.is_ok(), "{checked:?}");
        assert!(problem(&s).is_none());
    }

    #[test]
    fn a_dropped_widget_lands_above_or_below_the_one_it_was_dropped_on() {
        assert_eq!(
            drop_target(4, 0, 2, false),
            Some(1),
            "above the third: the second place"
        );
        assert_eq!(drop_target(4, 0, 2, true), Some(2), "below the third");
        assert_eq!(drop_target(4, 3, 1, false), Some(1));
        assert_eq!(drop_target(4, 3, 1, true), Some(2));
        assert_eq!(drop_target(4, 1, 1, false), None, "on itself");
        assert_eq!(drop_target(4, 1, 0, true), None, "right where it is");
        assert_eq!(drop_target(4, 9, 0, true), None);
        let mut s = spec(
            r#"{"widgets":[{"type":"toggle","id":"a"},{"type":"toggle","id":"b"},{"type":"toggle","id":"c"}]}"#,
        );
        assert_eq!(drop_widget(&mut s, 0, 2, true), Some(2));
        assert_eq!(ids(&s), ["b", "c", "a"]);
        assert_eq!(drop_widget(&mut s, 2, 0, false), Some(0));
        assert_eq!(ids(&s), ["a", "b", "c"]);
        let mut v = vec![1, 2, 3];
        assert!(move_item(&mut v, 0, 2) && v == [2, 3, 1]);
        assert!(!move_item(&mut v, 0, 5));
    }

    #[test]
    fn a_copy_of_a_grid_has_buttons_with_ids_of_their_own() {
        let mut s =
            spec(r#"{"widgets":[{"type":"buttons","id":"g","buttons":[{"id":"a1"},{"id":"b"}]}]}"#);
        assert_eq!(duplicate_widget(&mut s, 0), Some(1));
        assert!(problem(&s).is_none(), "{:?}", problem(&s));
        assert_eq!(taken_button_ids(&s).len(), 4);
    }

    #[test]
    fn ids_count_up_when_taken() {
        let taken: BTreeSet<String> = ["text", "text2"].iter().map(|s| s.to_string()).collect();
        assert_eq!(unique_id("text", &taken), "text3");
        assert_eq!(unique_id("other", &taken), "other");
    }

    #[test]
    fn a_full_panel_takes_no_more_widgets() {
        let mut s = PanelSpec::default();
        for _ in 0..MAX_WIDGETS {
            let at = s.widgets.len();
            assert!(insert_widget(&mut s, at, Widget::Separator { show_if: None }).is_some());
        }
        assert!(insert_widget(&mut s, 0, Widget::Separator { show_if: None }).is_none());
        assert!(duplicate_widget(&mut s, 0).is_none());
    }

    #[test]
    fn a_copy_gets_an_id_of_its_own_right_after_the_original() {
        let mut s = spec(
            r#"{"widgets":[{"type":"input","id":"name"},{"type":"toggle","id":"on"},{"type":"separator"}]}"#,
        );
        assert_eq!(duplicate_widget(&mut s, 0), Some(1));
        assert_eq!(ids(&s), ["name", "name2", "on", "-"]);
        assert_eq!(duplicate_widget(&mut s, 1), Some(2));
        assert_eq!(ids(&s)[2], "name3", "the number is not stacked: name22");
        assert_eq!(
            duplicate_widget(&mut s, 4),
            Some(5),
            "a line has no id to change"
        );
        assert!(duplicate_widget(&mut s, 99).is_none());
    }

    #[test]
    fn widgets_move_one_place_and_stop_at_the_ends() {
        let mut s = spec(
            r#"{"widgets":[{"type":"toggle","id":"a"},{"type":"toggle","id":"b"},{"type":"toggle","id":"c"}]}"#,
        );
        assert_eq!(move_widget(&mut s, 0, -1), None);
        assert_eq!(move_widget(&mut s, 2, 1), None);
        assert_eq!(move_widget(&mut s, 0, 1), Some(1));
        assert_eq!(ids(&s), ["b", "a", "c"]);
        assert_eq!(move_widget(&mut s, 2, -1), Some(1));
        assert_eq!(ids(&s), ["b", "c", "a"]);
        assert_eq!(move_widget(&mut s, 9, 1), None);
    }

    #[test]
    fn removing_a_widget_frees_the_ones_that_waited_for_it_and_picks_a_neighbour() {
        let mut s = spec(
            r#"{"widgets":[{"type":"toggle","id":"adv"},
            {"type":"input","id":"x","show_if":{"id":"adv"}},
            {"type":"input","id":"y","show_if":{"id":"x"}}]}"#,
        );
        assert_eq!(remove_widget(&mut s, 0), Some(0));
        assert!(s.widgets[0].show_if().is_none(), "x was waiting for adv");
        assert!(s.widgets[1].show_if().is_some(), "y still waits for x");
        assert!(problem(&s).is_none());
        assert_eq!(
            remove_widget(&mut s, 1),
            Some(0),
            "the last one: the one before"
        );
        assert_eq!(remove_widget(&mut s, 0), None, "nothing left");
        assert_eq!(remove_widget(&mut s, 0), None);
    }

    #[test]
    fn renaming_a_field_carries_the_conditions_along() {
        let mut s = spec(
            r#"{"widgets":[{"type":"choice","id":"mode","options":["a","b"]},
            {"type":"text","text":"t","show_if":{"id":"mode","is":"b","not":true}}]}"#,
        );
        rename_widget(&mut s, 0, "kind");
        assert_eq!(s.widgets[0].id(), Some("kind"));
        let c = s.widgets[1].show_if().unwrap();
        assert_eq!(
            (c.id.as_str(), c.is.as_deref(), c.not),
            ("kind", Some("b"), true)
        );
        assert!(problem(&s).is_none());
        rename_widget(&mut s, 1, "x");
        assert_eq!(s.widgets[1].id(), Some("x"), "a text may get an id");
        rename_widget(&mut s, 7, "x");
    }

    #[test]
    fn a_widget_may_wait_for_any_field_but_never_for_one_that_waits_for_it() {
        let s = spec(
            r#"{"widgets":[{"type":"toggle","id":"a"},
            {"type":"toggle","id":"b","show_if":{"id":"a"}},
            {"type":"toggle","id":"c","show_if":{"id":"b"}},
            {"type":"text","text":"not a field"},
            {"type":"progress","id":"p"}]}"#,
        );
        assert_eq!(
            conditions_for(&s, 0),
            Vec::<String>::new(),
            "b waits for a, c for b"
        );
        assert_eq!(conditions_for(&s, 1), ["a"]);
        assert_eq!(conditions_for(&s, 2), ["a", "b"]);
        assert_eq!(conditions_for(&s, 3), ["a", "b", "c"]);
    }

    #[test]
    fn options_are_lines_of_value_label_and_hint() {
        let options = text_to_options("red\n\n green | Green \nblue|Blue|the sea\n | nothing\n");
        assert_eq!(options.len(), 3);
        assert_eq!(
            (options[1].value.as_str(), options[1].label.as_str()),
            ("green", "Green")
        );
        assert_eq!(options[2].hint, "the sea");
        let text = options_to_text(&options);
        assert_eq!(text, "red\ngreen | Green\nblue | Blue | the sea");
        assert_eq!(
            text_to_options(&text),
            options,
            "text and options round trip"
        );
        // an option whose label is its value is written without it
        assert_eq!(
            options_to_text(&[Opt {
                value: "a".into(),
                label: "a".into(),
                hint: String::new()
            }]),
            "a"
        );
        assert_eq!(
            options_to_text(&[Opt {
                value: "a".into(),
                label: String::new(),
                hint: "h".into()
            }]),
            "a | a | h"
        );
        let many = (0..MAX_OPTIONS + 10)
            .map(|i| format!("o{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(text_to_options(&many).len(), MAX_OPTIONS);
    }

    #[test]
    fn buttons_stop_at_the_limit_and_get_free_ids() {
        let mut s = PanelSpec::default();
        while let Some(b) = new_button(&s) {
            assert!(!s.buttons.iter().any(|x| x.id == b.id));
            s.buttons.push(b);
        }
        assert_eq!(s.buttons.len(), MAX_BUTTONS);
        assert_eq!(s.buttons[1].id, "button2");
        assert!(!uses_default_buttons(&s));
        assert!(uses_default_buttons(&PanelSpec::default()));
        assert!(problem(&s).is_none());
    }

    #[test]
    fn every_template_is_a_valid_panel_with_a_name_a_hint_and_nothing_to_remember() {
        assert!(TEMPLATES.len() >= 4);
        for (i, t) in TEMPLATES.iter().enumerate() {
            assert!(!t.name.is_empty() && !t.hint.is_empty());
            let loose = template_spec(i).unwrap_or_else(|| panic!("{} is not JSON", t.name));
            assert!(
                problem(&loose).is_none(),
                "{}: {:?}",
                t.name,
                problem(&loose)
            );
            // and it goes through the text of a flow block and back
            let text = to_flow_text(&loose).unwrap();
            assert_eq!(from_flow_text(&text).unwrap(), loose, "{}", t.name);
        }
        assert!(template_spec(TEMPLATES.len()).is_none());
    }

    #[test]
    fn the_text_of_a_block_keeps_variables_and_refuses_what_is_too_long() {
        assert_eq!(from_flow_text("  \n").unwrap(), PanelSpec::default());
        assert!(from_flow_text("{ not json").is_err());
        let s =
            from_flow_text(r#"{"title":"Hi {{who}}","widgets":[{"type":"text","text":"{{x}}"}]}"#)
                .unwrap();
        assert_eq!(s.title, "Hi {{who}}");
        assert!(to_flow_text(&s).unwrap().contains("{{who}}"));
        // too long for a block even without indentation: the widgets hold 1000 characters each
        let mut big = PanelSpec::default();
        for _ in 0..MAX_WIDGETS {
            big.widgets.push(Widget::Text {
                id: String::new(),
                text: "x".repeat(900),
                style: TextStyle::Body,
                fmt: TextFmt::default(),
                show_if: None,
            });
        }
        assert!(to_flow_text(&big).is_err());
    }

    #[test]
    fn a_line_in_the_list_is_the_label_or_the_id_or_the_start_of_the_text() {
        let s = spec(
            r#"{"widgets":[{"type":"input","id":"name","label":"Your name"},{"type":"toggle","id":"on"},
            {"type":"text","text":"first line\nsecond"},{"type":"text","text":"this is a text that is far too long to fit in the list"},{"type":"separator"}]}"#,
        );
        let lines: Vec<String> = s.widgets.iter().map(summary).collect();
        assert_eq!(lines[0], "Your name");
        assert_eq!(lines[1], "on");
        assert_eq!(lines[2], "first line");
        assert!(lines[3].ends_with('\u{2026}') && lines[3].chars().count() <= 34);
        assert_eq!(lines[4], "");
    }
}

#[cfg(test)]
mod clip_tests {
    use super::*;

    fn spec(json: &str) -> PanelSpec {
        serde_json::from_str(json).expect("json")
    }

    fn ids(spec: &PanelSpec) -> Vec<String> {
        spec.widgets
            .iter()
            .map(|w| w.id().unwrap_or("-").to_string())
            .collect()
    }

    fn four() -> PanelSpec {
        spec(
            r#"{"widgets":[{"type":"toggle","id":"a"},{"type":"toggle","id":"b"},
            {"type":"toggle","id":"c"},{"type":"toggle","id":"d"}]}"#,
        )
    }

    #[test]
    fn pasted_widgets_get_ids_of_their_own_and_keep_their_order() {
        let mut s = four();
        let text = copy_widgets(&s, &[2, 0, 0]).unwrap();
        assert!(text.starts_with(CLIP_WIDGETS));
        assert_eq!(
            paste_widgets(&mut s, 4, &text),
            PasteWidgets::Done(vec![4, 5])
        );
        assert_eq!(ids(&s), ["a", "b", "c", "d", "a2", "c2"]);
        // pasted into a panel without them, the ids stay
        let mut other = spec(r#"{"widgets":[{"type":"toggle","id":"z"}]}"#);
        assert_eq!(
            paste_widgets(&mut other, 0, &text),
            PasteWidgets::Done(vec![0, 1])
        );
        assert_eq!(ids(&other), ["a", "c", "z"]);
    }

    #[test]
    fn a_show_if_follows_a_copied_widget_and_is_dropped_when_nothing_is_there() {
        let mut s = spec(
            r#"{"widgets":[{"type":"toggle","id":"a"},
            {"type":"toggle","id":"b","show_if":{"id":"a"}}]}"#,
        );
        // both copied, into the same panel: the copy waits for the copy
        let both = copy_widgets(&s, &[0, 1]).unwrap();
        assert_eq!(
            paste_widgets(&mut s, 2, &both),
            PasteWidgets::Done(vec![2, 3])
        );
        assert_eq!(s.widgets[3].show_if().map(|c| c.id.as_str()), Some("a2"));
        // only the one that waits, into the same panel: it still waits for the widget it had
        let only = copy_widgets(&s, &[1]).unwrap();
        assert_eq!(paste_widgets(&mut s, 4, &only), PasteWidgets::Done(vec![4]));
        assert_eq!(s.widgets[4].show_if().map(|c| c.id.as_str()), Some("a"));
        // only the one that waits, into a panel that has no `a`: the condition goes
        let mut empty = PanelSpec::default();
        assert_eq!(
            paste_widgets(&mut empty, 0, &only),
            PasteWidgets::Done(vec![0])
        );
        assert!(empty.widgets[0].show_if().is_none());
    }

    #[test]
    fn the_buttons_of_a_copied_grid_answer_with_ids_of_their_own() {
        let mut s = PanelSpec::default();
        let grid = new_widget(Kind::Buttons, &taken_ids(&s));
        assert_eq!(insert_widget(&mut s, 0, grid), Some(0));
        assert_eq!(duplicate_widgets(&mut s, &[0]), Some(vec![1]));
        let all: Vec<String> = s.all_buttons().map(|b| b.id.clone()).collect();
        let distinct: BTreeSet<&String> = all.iter().collect();
        assert!(all.len() >= 2);
        assert_eq!(distinct.len(), all.len(), "{all:?}");
    }

    #[test]
    fn other_text_is_not_widgets_and_a_full_panel_takes_nothing() {
        let mut s = four();
        let before = s.clone();
        assert_eq!(paste_widgets(&mut s, 0, "hello"), PasteWidgets::NotWidgets);
        let broken = format!("{CLIP_WIDGETS}[{{\"type\":");
        assert_eq!(paste_widgets(&mut s, 0, &broken), PasteWidgets::NotWidgets);
        let none = format!("{CLIP_WIDGETS}[]");
        assert_eq!(paste_widgets(&mut s, 0, &none), PasteWidgets::NotWidgets);
        assert_eq!(copy_widgets(&s, &[]), None);
        assert_eq!(copy_widgets(&s, &[9]), None);
        assert_eq!(s, before);
        let text = copy_widgets(&s, &[0, 1]).unwrap();
        while s.widgets.len() < MAX_WIDGETS - 1 {
            let w = new_widget(Kind::Separator, &taken_ids(&s));
            insert_widget(&mut s, 0, w);
        }
        let count = s.widgets.len();
        assert_eq!(paste_widgets(&mut s, 0, &text), PasteWidgets::Full);
        assert_eq!(s.widgets.len(), count);
    }

    #[test]
    fn several_widgets_are_removed_and_moved_together() {
        let mut s = four();
        assert_eq!(remove_widgets(&mut s, &[2, 0]), Some(0));
        assert_eq!(ids(&s), ["b", "d"]);
        assert_eq!(
            remove_widgets(&mut s, &[0, 1]),
            None,
            "an empty panel selects nothing"
        );
        assert_eq!(remove_widgets(&mut s, &[3]), None);

        let mut s = four();
        assert_eq!(move_widgets(&mut s, &[1, 2], -1), Some(vec![0, 1]));
        assert_eq!(ids(&s), ["b", "c", "a", "d"]);
        assert_eq!(move_widgets(&mut s, &[0, 1], -1), None, "already first");
        assert_eq!(move_widgets(&mut s, &[0, 1], 1), Some(vec![1, 2]));
        assert_eq!(ids(&s), ["a", "b", "c", "d"]);
        // two that are apart stay apart, and the one at the end stays
        assert_eq!(move_widgets(&mut s, &[0, 3], 1), Some(vec![1, 3]));
        assert_eq!(ids(&s), ["b", "a", "c", "d"]);
    }

    #[test]
    fn removing_a_widget_frees_the_ones_that_waited_for_it() {
        let mut s = spec(
            r#"{"widgets":[{"type":"toggle","id":"a"},
            {"type":"toggle","id":"b","show_if":{"id":"a"}}]}"#,
        );
        assert_eq!(remove_widgets(&mut s, &[0]), Some(0));
        assert!(s.widgets[0].show_if().is_none());
    }
}
