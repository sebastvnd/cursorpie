//! The live state of a panel on screen: what the person has typed, picked and switched, and how it
//! becomes an answer ([`PanelResult`]).
//!
//! The form has no drawing code: [`ui::settings::panel_form`](crate::ui) reads and writes
//! [`Form::fields`] while it draws, and calls [`Form::submit`] or [`Form::pick`] when a button or a
//! menu entry is pressed. Everything that has a rule (what an empty required field does, what a
//! patch changes, how a number is snapped to its step) is here, where it can be tested.

use super::spec::{
    Button, MAX_LABEL_CHARS, MAX_TEXT_CHARS, MAX_TITLE_CHARS, MAX_VALUE_CHARS, Opt, PanelSpec,
    Tone, Widget, clean_line, clean_options, format_hex, number_bounds, parse_hex,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What a panel ended with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// A button that confirms was pressed (or a menu entry picked).
    Submitted,
    /// A cancel button, Esc, or the panel was closed or replaced.
    Cancelled,
    /// Nobody answered in time.
    Timeout,
    /// The panel could not be shown (switched off, Settings open, too many waiting...).
    Unavailable,
}

/// One value of the answer.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Value {
    Text(String),
    Bool(bool),
    Number(f64),
    List(Vec<String>),
}

impl Value {
    /// The value as the text of a flow variable: a switch is `1` or `0` like every flag of a flow,
    /// a number has no useless `.0`, a list is a JSON array like the lists of a flow.
    pub fn to_flow_text(&self) -> String {
        match self {
            Value::Text(t) => t.clone(),
            Value::Bool(b) => (if *b { "1" } else { "0" }).to_string(),
            Value::Number(n) => number_text(*n),
            Value::List(items) => serde_json::to_string(items).unwrap_or_else(|_| "[]".into()),
        }
    }
}

/// `3` for 3.0, `0.25` for 0.25.
pub fn number_text(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// The answer of a panel.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PanelResult {
    /// The `id` of the panel (empty when it had none).
    pub panel: String,
    pub outcome: Outcome,
    /// The id of the button that was pressed (`pick` for a menu entry; empty when there was none).
    pub button: String,
    /// What the fields held, by id. Empty unless the panel was submitted.
    pub values: BTreeMap<String, Value>,
    /// Why, for [`Outcome::Unavailable`] and a panel that was replaced or closed from outside.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

impl PanelResult {
    fn without_values(panel: &str, outcome: Outcome, button: &str, reason: &str) -> Self {
        Self {
            panel: panel.to_string(),
            outcome,
            button: button.to_string(),
            values: BTreeMap::new(),
            reason: reason.to_string(),
        }
    }

    /// The panel could not be shown.
    pub fn unavailable(panel: &str, reason: impl AsRef<str>) -> Self {
        Self::without_values(panel, Outcome::Unavailable, "", reason.as_ref())
    }

    /// The panel was closed without an answer.
    pub fn cancelled(panel: &str, reason: impl AsRef<str>) -> Self {
        Self::without_values(panel, Outcome::Cancelled, "", reason.as_ref())
    }

    /// Nobody answered in time.
    pub fn timeout(panel: &str) -> Self {
        Self::without_values(panel, Outcome::Timeout, "", "")
    }

    /// Whether a confirming button was pressed.
    pub fn submitted(&self) -> bool {
        self.outcome == Outcome::Submitted
    }

    /// The values as `(variable, text)` pairs, for a flow.
    pub fn flow_pairs(&self) -> Vec<(String, String)> {
        self.values
            .iter()
            .map(|(k, v)| (k.clone(), v.to_flow_text()))
            .collect()
    }

    /// The answer as JSON, what the browser extension receives.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

/// What a widget holds while the panel is open. One per widget, in the same order.
#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    /// A widget that holds nothing (text, separator, progress).
    None,
    Text(String),
    Bool(bool),
    Number(f64),
    One(String),
    Many(Vec<String>),
}

/// A change to a panel that is open: its title, or one widget by its `id`. Sent by a flow, by an
/// extension (`panel_update`) or by the code that started the panel, to show progress or new
/// options without closing and opening again.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Patch {
    /// The id of the widget; empty to change the panel itself (`text` is the title).
    #[serde(default)]
    pub widget: String,
    /// The paragraph of a text widget, or the title of the panel.
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub subtitle: Option<String>,
    /// The label of a widget.
    #[serde(default)]
    pub label: Option<String>,
    /// The value: a number (progress, slider), a text (input, choice), a bool (toggle) or a list of
    /// texts (multi).
    #[serde(default)]
    pub value: Option<serde_json::Value>,
    /// New options for a list.
    #[serde(default)]
    pub options: Option<Vec<Opt>>,
}

/// A panel on screen.
#[derive(Clone, Debug)]
pub struct Form {
    pub spec: PanelSpec,
    /// One per widget of the spec.
    pub fields: Vec<Field>,
    /// What was typed in the search box of each list (one per widget).
    pub filters: Vec<String>,
    /// A line under the buttons: why the last press did nothing.
    pub notice: Option<String>,
    /// Show every widget, whatever its `show_if` says. For the panel designer, which has to draw
    /// (and let the person pick) the widgets that a real panel would still hide.
    pub reveal_all: bool,
}

impl Form {
    pub fn new(spec: PanelSpec) -> Self {
        let fields = spec
            .widgets
            .iter()
            .map(|w| match w {
                Widget::Input { value, .. } => Field::Text(value.clone()),
                Widget::Toggle { value, .. } => Field::Bool(*value),
                Widget::Slider { value, min, .. } => Field::Number(value.unwrap_or(*min)),
                Widget::Number {
                    value, min, max, ..
                } => {
                    let (lo, hi) = number_bounds(*min, *max);
                    Field::Number(value.unwrap_or(0.0).clamp(lo, hi))
                }
                Widget::Color { value, .. } => Field::Text(value.clone()),
                Widget::Choice { value, .. } => Field::One(value.clone()),
                Widget::Multi { values, .. } => Field::Many(values.clone()),
                Widget::Text { .. }
                | Widget::Separator { .. }
                | Widget::Progress { .. }
                | Widget::Buttons { .. }
                | Widget::Spacer { .. } => Field::None,
            })
            .collect();
        let filters = vec![String::new(); spec.widgets.len()];
        Self {
            spec,
            fields,
            filters,
            notice: None,
            reveal_all: false,
        }
    }

    /// Whether widget `index` is on screen now: it has no `show_if`, or the field it waits for
    /// satisfies it. A widget that waits for a hidden field is hidden too (so a whole group can
    /// hang from one switch), and a condition that loops back on itself hides what it guards.
    pub fn is_visible(&self, index: usize) -> bool {
        self.reveal_all || self.visible_at(index, 0)
    }

    fn visible_at(&self, index: usize, depth: usize) -> bool {
        const MAX_DEPTH: usize = 8;
        let Some(cond) = self.spec.widgets.get(index).and_then(Widget::show_if) else {
            return true;
        };
        if depth >= MAX_DEPTH {
            return false;
        }
        let Some(at) = self
            .spec
            .widgets
            .iter()
            .position(|w| w.is_field() && w.id() == Some(cond.id.as_str()))
        else {
            // a description that went through `checked` cannot get here; show rather than hide
            return true;
        };
        if !self.visible_at(at, depth + 1) {
            return false;
        }
        field_satisfies(&self.fields[at], cond.is.as_deref()) != cond.not
    }

    /// The id of the panel.
    pub fn panel_id(&self) -> &str {
        &self.spec.id
    }

    /// The button that Enter presses: the first primary one, else the first that does not cancel.
    /// `None` for a menu.
    pub fn default_button(&self) -> Option<&Button> {
        if self.spec.is_menu() {
            return None;
        }
        self.spec
            .buttons
            .iter()
            .find(|b| b.tone == Tone::Primary && !b.cancel)
            .or_else(|| self.spec.buttons.iter().find(|b| !b.cancel))
    }

    /// The button that Esc presses: the first one that cancels.
    pub fn cancel_button(&self) -> Option<&Button> {
        self.spec.buttons.iter().find(|b| b.cancel)
    }

    /// The label of the first required box that is empty.
    pub fn missing(&self) -> Option<String> {
        for (i, (w, f)) in self.spec.widgets.iter().zip(&self.fields).enumerate() {
            if let (
                Widget::Input {
                    label,
                    id,
                    required: true,
                    ..
                },
                Field::Text(t),
            ) = (w, f)
                && t.trim().is_empty()
                && self.is_visible(i)
            {
                return Some(if label.is_empty() {
                    id.clone()
                } else {
                    label.clone()
                });
            }
        }
        None
    }

    /// Presses the button `button`. A button that cancels closes the panel without values; any other
    /// is refused (`Err` says why, and is also kept in [`Form::notice`]) while a required box is
    /// empty.
    pub fn submit(&mut self, button: &str) -> Result<PanelResult, String> {
        // the row at the bottom, or a button of a grid
        let Some(b) = self.spec.find_button(button).cloned() else {
            return Err(format!("there is no button `{button}`"));
        };
        if b.cancel {
            let mut r = PanelResult::cancelled(&self.spec.id, "");
            r.button = b.id;
            return Ok(r);
        }
        if let Some(what) = self.missing() {
            let why = format!("Fill in \u{201c}{what}\u{201d} first");
            self.notice = Some(why.clone());
            return Err(why);
        }
        self.notice = None;
        Ok(self.answer(&b.id))
    }

    /// Whether the panel stays on screen after the button `button` answered (`pick` is the answer of
    /// a menu): the panel asked for it as a whole, or that button did. A button that cancels never
    /// does.
    pub fn keeps_open(&self, button: &str) -> bool {
        match self.spec.find_button(button) {
            Some(b) => !b.cancel && (self.spec.keep_open || b.keep_open),
            None => button == "pick" && self.spec.keep_open,
        }
    }

    /// An option of the menu `widget` was picked: sets it and answers with the button `pick`.
    pub fn pick(&mut self, widget: usize, value: &str) -> Option<PanelResult> {
        match (self.spec.widgets.get(widget), self.fields.get_mut(widget)) {
            (
                Some(Widget::Choice {
                    submit: true,
                    options,
                    ..
                }),
                Some(field),
            ) if options.iter().any(|o| o.value == value) => {
                *field = Field::One(value.to_string());
            }
            _ => return None,
        }
        Some(self.answer("pick"))
    }

    /// The panel as it is now, answered with `button`.
    fn answer(&self, button: &str) -> PanelResult {
        let mut values = BTreeMap::new();
        for (w, f) in self.spec.widgets.iter().zip(&self.fields) {
            let (Some(id), true) = (w.id(), w.is_field()) else {
                continue;
            };
            let value = match (w, f) {
                (_, Field::Text(t)) => Value::Text(t.clone()),
                (_, Field::Bool(b)) => Value::Bool(*b),
                (Widget::Slider { min, max, step, .. }, Field::Number(n)) => {
                    Value::Number(snap(*n, *min, *max, *step))
                }
                (Widget::Number { min, max, step, .. }, Field::Number(n)) => {
                    Value::Number(snap_number(*n, *min, *max, *step))
                }
                (_, Field::Number(n)) => Value::Number(*n),
                (_, Field::One(v)) => Value::Text(v.clone()),
                (_, Field::Many(v)) => Value::List(v.clone()),
                (_, Field::None) => continue,
            };
            values.insert(id.to_string(), value);
        }
        PanelResult {
            panel: self.spec.id.clone(),
            outcome: Outcome::Submitted,
            button: button.to_string(),
            values,
            reason: String::new(),
        }
    }

    /// The indexes of the options of list `widget` that match what was typed in its search box.
    pub fn matching(&self, widget: usize) -> Vec<usize> {
        let options = match self.spec.widgets.get(widget) {
            Some(Widget::Choice { options, .. } | Widget::Multi { options, .. }) => options,
            _ => return Vec::new(),
        };
        let query = self
            .filters
            .get(widget)
            .map(|q| q.trim().to_lowercase())
            .unwrap_or_default();
        options
            .iter()
            .enumerate()
            .filter(|(_, o)| {
                query.is_empty()
                    || o.label.to_lowercase().contains(&query)
                    || o.hint.to_lowercase().contains(&query)
                    || o.value.to_lowercase().contains(&query)
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Applies a change. Returns whether anything changed.
    pub fn apply(&mut self, patch: &Patch) -> bool {
        if patch.widget.is_empty() {
            let mut changed = false;
            if let Some(t) = &patch.text {
                self.spec.title = clean_line(t, MAX_TITLE_CHARS);
                changed = true;
            }
            if let Some(s) = &patch.subtitle {
                self.spec.subtitle = clean_line(s, MAX_LABEL_CHARS);
                changed = true;
            }
            return changed;
        }
        let Some(i) = self
            .spec
            .widgets
            .iter()
            .position(|w| w.id() == Some(patch.widget.as_str()))
        else {
            return false;
        };
        let label = patch
            .label
            .as_deref()
            .map(|l| clean_line(l, MAX_LABEL_CHARS));
        let value = patch.value.as_ref();
        match (&mut self.spec.widgets[i], &mut self.fields[i]) {
            (Widget::Text { text, .. }, _) => match &patch.text {
                Some(t) => {
                    *text = super::spec::clean_text(t, MAX_TEXT_CHARS);
                    true
                }
                None => false,
            },
            (
                Widget::Progress {
                    value: v, label: l, ..
                },
                _,
            ) => {
                let mut changed = false;
                if let Some(n) = value.and_then(serde_json::Value::as_f64) {
                    *v = if n.is_finite() && n >= 0.0 {
                        n.min(1.0)
                    } else {
                        -1.0
                    };
                    changed = true;
                }
                if let Some(new) = label {
                    *l = new;
                    changed = true;
                }
                changed
            }
            (
                Widget::Input {
                    max_chars,
                    label: l,
                    ..
                },
                Field::Text(t),
            ) => {
                let mut changed = false;
                if let Some(s) = value.and_then(serde_json::Value::as_str) {
                    let cap = max_chars.map_or(MAX_VALUE_CHARS, |n| n as usize);
                    *t = s.chars().take(cap).collect();
                    changed = true;
                }
                if let Some(new) = label {
                    *l = new;
                    changed = true;
                }
                changed
            }
            (
                Widget::Number {
                    min,
                    max,
                    step,
                    label: l,
                    ..
                },
                Field::Number(n),
            ) => {
                let mut changed = false;
                if let Some(new) = value.and_then(serde_json::Value::as_f64)
                    && new.is_finite()
                {
                    *n = snap_number(new, *min, *max, *step);
                    changed = true;
                }
                if let Some(new) = label {
                    *l = new;
                    changed = true;
                }
                changed
            }
            (Widget::Color { .. }, Field::Text(t)) => {
                match value
                    .and_then(serde_json::Value::as_str)
                    .and_then(parse_hex)
                {
                    Some(rgb) => {
                        *t = format_hex(rgb);
                        true
                    }
                    None => false,
                }
            }
            (Widget::Toggle { .. }, Field::Bool(b)) => {
                match value.and_then(serde_json::Value::as_bool) {
                    Some(new) => {
                        *b = new;
                        true
                    }
                    None => false,
                }
            }
            (Widget::Slider { min, max, .. }, Field::Number(n)) => {
                match value.and_then(serde_json::Value::as_f64) {
                    Some(new) if new.is_finite() => {
                        *n = new.clamp(*min, *max);
                        true
                    }
                    _ => false,
                }
            }
            (Widget::Choice { options, .. }, Field::One(current)) => {
                let mut changed = false;
                if let Some(new) = &patch.options {
                    let mut new = new.clone();
                    if clean_options(&mut new).is_ok() {
                        *options = new;
                        if !options.iter().any(|o| o.value == *current) {
                            current.clear();
                        }
                        changed = true;
                    }
                }
                if let Some(s) = value.and_then(serde_json::Value::as_str)
                    && options.iter().any(|o| o.value == s)
                {
                    *current = s.to_string();
                    changed = true;
                }
                changed
            }
            (Widget::Multi { options, .. }, Field::Many(current)) => {
                let mut changed = false;
                if let Some(new) = &patch.options {
                    let mut new = new.clone();
                    if clean_options(&mut new).is_ok() {
                        *options = new;
                        current.retain(|v| options.iter().any(|o| o.value == *v));
                        changed = true;
                    }
                }
                if let Some(list) = value.and_then(serde_json::Value::as_array) {
                    let mut kept: Vec<String> = Vec::new();
                    for item in list {
                        if let Some(s) = item.as_str()
                            && options.iter().any(|o| o.value == s)
                            && !kept.iter().any(|k| k == s)
                        {
                            kept.push(s.to_string());
                        }
                    }
                    *current = kept;
                    changed = true;
                }
                changed
            }
            _ => false,
        }
    }
}

/// Whether `field` meets what a `show_if` asks: without `is` the field only has to be on, with it
/// the field has to hold that value (a list holds it among its picks).
fn field_satisfies(field: &Field, is: Option<&str>) -> bool {
    let truthy = |t: &str| matches!(t.trim(), "1" | "true" | "yes" | "on");
    match (field, is) {
        (Field::Text(t) | Field::One(t), None) => !t.trim().is_empty(),
        (Field::Text(t) | Field::One(t), Some(v)) => t == v,
        (Field::Bool(b), None) => *b,
        (Field::Bool(b), Some(v)) => *b == truthy(v),
        (Field::Number(n), None) => *n != 0.0,
        (Field::Number(n), Some(v)) => v.trim().parse::<f64>().is_ok_and(|v| v == *n),
        (Field::Many(list), None) => !list.is_empty(),
        (Field::Many(list), Some(v)) => list.iter().any(|x| x == v),
        (Field::None, _) => false,
    }
}

/// [`snap`] for a [`Widget::Number`], whose range and step are all optional: the grid starts at
/// `min` (at 0 without one), the step is 1 when the description gives none.
pub fn snap_number(n: f64, min: Option<f64>, max: Option<f64>, step: Option<f64>) -> f64 {
    let (lo, hi) = number_bounds(min, max);
    let origin = min.unwrap_or(0.0).clamp(lo, hi);
    let n = n.clamp(lo, hi);
    let s = step.filter(|s| s.is_finite() && *s > 0.0).unwrap_or(1.0);
    let snapped = origin + ((n - origin) / s).round() * s;
    ((snapped * 1e9).round() / 1e9).clamp(lo, hi)
}

/// `n` moved onto the grid of `step` (counted from `min`) and kept inside `min..=max`.
pub fn snap(n: f64, min: f64, max: f64, step: Option<f64>) -> f64 {
    let n = n.clamp(min, max);
    match step {
        Some(s) if s > 0.0 => {
            let snapped = min + ((n - min) / s).round() * s;
            // 0.1 * 3 is 0.30000000000000004: drop that noise, but keep every digit a step has
            ((snapped * 1e9).round() / 1e9).clamp(min, max)
        }
        _ => n,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(json: &str) -> Form {
        Form::new(PanelSpec::parse(json).expect("a good panel"))
    }

    const FORM: &str = r#"{"id":"p","widgets":[
        {"type":"input","id":"name","label":"Name","required":true},
        {"type":"toggle","id":"loud","value":true},
        {"type":"slider","id":"vol","min":0,"max":1,"step":0.1,"value":0.34},
        {"type":"choice","id":"color","options":["red","green"],"value":"green"},
        {"type":"multi","id":"tags","options":["a","b","c"],"values":["b"]},
        {"type":"text","text":"hello"}],
        "buttons":[{"id":"go","tone":"primary"},{"id":"no","cancel":true}]}"#;

    #[test]
    fn a_form_starts_with_the_values_of_its_spec() {
        let f = form(FORM);
        assert_eq!(f.fields.len(), f.spec.widgets.len());
        assert_eq!(f.fields[1], Field::Bool(true));
        assert_eq!(f.fields[3], Field::One("green".into()));
        assert_eq!(f.fields[5], Field::None);
    }

    #[test]
    fn an_empty_required_box_refuses_the_confirming_button_but_not_cancel() {
        let mut f = form(FORM);
        let err = f.submit("go").unwrap_err();
        assert!(err.contains("Name"), "{err}");
        assert!(f.notice.is_some());
        let r = f.submit("no").unwrap();
        assert_eq!(r.outcome, Outcome::Cancelled);
        assert_eq!(r.button, "no");
        assert!(r.values.is_empty());
    }

    #[test]
    fn a_submitted_form_answers_with_every_field_by_id() {
        let mut f = form(FORM);
        f.fields[0] = Field::Text("Ann".into());
        let r = f.submit("go").unwrap();
        assert!(r.submitted());
        assert_eq!((r.panel.as_str(), r.button.as_str()), ("p", "go"));
        assert_eq!(r.values["name"], Value::Text("Ann".into()));
        assert_eq!(r.values["loud"], Value::Bool(true));
        assert_eq!(r.values["vol"], Value::Number(0.3), "snapped to the step");
        assert_eq!(r.values["color"], Value::Text("green".into()));
        assert_eq!(r.values["tags"], Value::List(vec!["b".into()]));
        assert!(!r.values.contains_key("hello"));
    }

    #[test]
    fn values_become_the_text_of_flow_variables() {
        assert_eq!(Value::Bool(true).to_flow_text(), "1");
        assert_eq!(Value::Bool(false).to_flow_text(), "0");
        assert_eq!(Value::Number(3.0).to_flow_text(), "3");
        assert_eq!(Value::Number(0.25).to_flow_text(), "0.25");
        assert_eq!(
            Value::List(vec!["a".into(), "b\"c".into()]).to_flow_text(),
            r#"["a","b\"c"]"#
        );
    }

    #[test]
    fn a_result_is_json_for_the_extension() {
        let mut f = form(FORM);
        f.fields[0] = Field::Text("Ann".into());
        let j = f.submit("go").unwrap().to_json();
        assert_eq!(j["outcome"], "submitted");
        assert_eq!(j["button"], "go");
        assert_eq!(j["values"]["name"], "Ann");
        assert_eq!(j["values"]["loud"], true);
        assert!(j.get("reason").is_none(), "an empty reason is left out");
        let u = PanelResult::unavailable("x", "busy").to_json();
        assert_eq!(u["outcome"], "unavailable");
        assert_eq!(u["reason"], "busy");
    }

    #[test]
    fn enter_presses_the_primary_button_and_a_menu_has_none() {
        let f = form(FORM);
        assert_eq!(f.default_button().map(|b| b.id.as_str()), Some("go"));
        assert_eq!(f.cancel_button().map(|b| b.id.as_str()), Some("no"));
        let menu =
            form(r#"{"widgets":[{"type":"choice","id":"c","options":["a","b"],"submit":true}]}"#);
        assert!(menu.default_button().is_none());
    }

    #[test]
    fn picking_from_a_menu_answers_at_once() {
        let mut m = form(
            r#"{"id":"m","widgets":[{"type":"choice","id":"c","options":["a","b"],"submit":true}]}"#,
        );
        assert!(m.pick(0, "zzz").is_none(), "not an option");
        let r = m.pick(0, "b").unwrap();
        assert_eq!(r.button, "pick");
        assert_eq!(r.values["c"], Value::Text("b".into()));
        // a list that does not answer on its own has nothing to pick
        let mut f = form(FORM);
        assert!(f.pick(3, "red").is_none());
    }

    #[test]
    fn the_search_box_filters_the_options_by_label_hint_or_value() {
        let mut f = form(
            r#"{"widgets":[{"type":"choice","id":"c","options":[
                {"value":"1","label":"Inbox","hint":"mail.example"},{"value":"2","label":"News"},{"value":"3x","label":"Zed"}]}]}"#,
        );
        assert_eq!(f.matching(0), [0, 1, 2]);
        f.filters[0] = "  MAIL ".into();
        assert_eq!(f.matching(0), [0]);
        f.filters[0] = "3x".into();
        assert_eq!(f.matching(0), [2]);
        assert!(f.matching(9).is_empty());
    }

    #[test]
    fn a_patch_changes_the_title_and_widgets_by_id() {
        let mut f = form(
            r#"{"title":"T","widgets":[{"type":"text","id":"t","text":"a"},{"type":"progress","id":"p"},
               {"type":"choice","id":"c","options":["x","y"],"value":"y"},{"type":"input","id":"i","max_chars":3},
               {"type":"toggle","id":"g"},{"type":"slider","id":"s","min":0,"max":10},
               {"type":"multi","id":"m","options":["a","b"]}]}"#,
        );
        let p = |json: &str| -> Patch { serde_json::from_str(json).unwrap() };
        assert!(f.apply(&p(r#"{"text":"New title","subtitle":"sub"}"#)));
        assert_eq!(
            (f.spec.title.as_str(), f.spec.subtitle.as_str()),
            ("New title", "sub")
        );
        assert!(f.apply(&p(r#"{"widget":"t","text":"b"}"#)));
        assert!(f.apply(&p(r#"{"widget":"p","value":0.5,"label":"Half"}"#)));
        assert!(f.apply(&p(r#"{"widget":"p","value":-1}"#)));
        assert!(f.apply(&p(r#"{"widget":"c","options":["x","z"]}"#)));
        assert_eq!(
            f.fields[2],
            Field::One(String::new()),
            "the old pick is gone"
        );
        assert!(f.apply(&p(r#"{"widget":"c","value":"z"}"#)));
        assert!(!f.apply(&p(r#"{"widget":"c","value":"nope"}"#)));
        assert!(f.apply(&p(r#"{"widget":"i","value":"abcdef"}"#)));
        assert_eq!(f.fields[3], Field::Text("abc".into()), "max_chars holds");
        assert!(f.apply(&p(r#"{"widget":"g","value":true}"#)));
        assert!(f.apply(&p(r#"{"widget":"s","value":99}"#)));
        assert_eq!(f.fields[5], Field::Number(10.0));
        assert!(f.apply(&p(r#"{"widget":"m","value":["b","q","b"]}"#)));
        assert_eq!(f.fields[6], Field::Many(vec!["b".into()]));
        assert!(!f.apply(&p(r#"{"widget":"nobody","text":"x"}"#)));
        assert!(!f.apply(&p(r#"{"widget":"g","value":"not a bool"}"#)));
    }

    #[test]
    fn snapping_keeps_to_the_grid_and_the_range() {
        assert_eq!(snap(0.34, 0.0, 1.0, Some(0.1)), 0.3);
        assert_eq!(snap(7.0, 0.0, 10.0, Some(5.0)), 5.0);
        assert_eq!(snap(99.0, 0.0, 10.0, None), 10.0);
        assert_eq!(snap(0.5, 0.0, 1.0, Some(0.0)), 0.5);
    }

    #[test]
    fn a_number_is_snapped_to_its_step_from_its_minimum() {
        assert_eq!(snap_number(7.0, Some(1.0), Some(20.0), Some(5.0)), 6.0);
        assert_eq!(
            snap_number(7.4, None, None, None),
            7.0,
            "a step of 1 without one"
        );
        assert_eq!(snap_number(0.34, Some(0.0), Some(1.0), Some(0.1)), 0.3);
        assert_eq!(snap_number(-5.0, Some(0.0), None, None), 0.0);
        assert_eq!(snap_number(f64::MAX, None, None, None), 1e12);
    }

    #[test]
    fn a_number_and_a_color_are_part_of_the_answer_and_can_be_patched() {
        let mut f = form(
            r##"{"widgets":[{"type":"number","id":"n","min":0,"max":10,"step":2,"value":3},
                {"type":"color","id":"c","value":"#000000"}]}"##,
        );
        assert_eq!(f.fields[0], Field::Number(3.0));
        let p = |json: &str| -> Patch { serde_json::from_str(json).unwrap() };
        assert!(f.apply(&p(r#"{"widget":"n","value":99}"#)));
        assert_eq!(f.fields[0], Field::Number(10.0));
        assert!(f.apply(&p(r##"{"widget":"c","value":"FF0000"}"##)));
        assert!(!f.apply(&p(r#"{"widget":"c","value":"red"}"#)));
        let r = f.submit("ok").unwrap();
        assert_eq!(r.values["n"], Value::Number(10.0));
        assert_eq!(r.values["c"], Value::Text("#ff0000".into()));
    }

    const SHOWN: &str = r#"{"widgets":[
        {"type":"toggle","id":"adv"},
        {"type":"input","id":"path","label":"Path","required":true,"show_if":{"id":"adv"}},
        {"type":"choice","id":"mode","options":["fast","custom"],"value":"fast"},
        {"type":"input","id":"other","show_if":{"id":"mode","is":"custom"}},
        {"type":"input","id":"plain","show_if":{"id":"mode","is":"custom","not":true}},
        {"type":"toggle","id":"deep","show_if":{"id":"path"}},
        {"type":"text","text":"deeper","show_if":{"id":"deep"}}]}"#;

    #[test]
    fn a_widget_is_shown_while_the_field_it_waits_for_says_so() {
        let mut f = form(SHOWN);
        let shown = |f: &Form| {
            (0..f.spec.widgets.len())
                .map(|i| f.is_visible(i))
                .collect::<Vec<_>>()
        };
        assert_eq!(shown(&f), [true, false, true, false, true, false, false]);
        f.fields[0] = Field::Bool(true);
        f.fields[2] = Field::One("custom".into());
        assert_eq!(
            shown(&f),
            [true, true, true, true, false, false, false],
            "an empty box is not on"
        );
        f.fields[1] = Field::Text("x".into());
        f.fields[5] = Field::Bool(true);
        assert_eq!(shown(&f), [true, true, true, true, false, true, true]);
        // what hangs from a hidden field is hidden too
        f.fields[0] = Field::Bool(false);
        assert_eq!(shown(&f), [true, false, true, true, false, false, false]);
        f.reveal_all = true;
        assert_eq!(shown(&f), [true; 7], "the designer sees everything");
    }

    #[test]
    fn a_hidden_required_box_does_not_stop_the_confirming_button() {
        let mut f = form(SHOWN);
        assert!(
            f.submit("ok").is_ok(),
            "the path is hidden while the switch is off"
        );
        f.fields[0] = Field::Bool(true);
        assert!(f.submit("ok").unwrap_err().contains("Path"));
    }

    #[test]
    fn show_if_compares_what_the_field_holds() {
        assert!(field_satisfies(&Field::Bool(true), Some("1")));
        assert!(field_satisfies(&Field::Bool(false), Some("0")));
        assert!(!field_satisfies(&Field::Bool(true), Some("0")));
        assert!(field_satisfies(&Field::Number(3.0), Some("3.0")));
        assert!(!field_satisfies(&Field::Number(0.0), None));
        assert!(field_satisfies(
            &Field::Many(vec!["a".into(), "b".into()]),
            Some("b")
        ));
        assert!(!field_satisfies(&Field::Many(Vec::new()), None));
        assert!(!field_satisfies(&Field::None, None));
    }

    #[test]
    fn a_condition_that_loops_hides_what_it_guards() {
        let mut spec = PanelSpec::parse(
            r#"{"widgets":[{"type":"toggle","id":"a"},{"type":"toggle","id":"b"}]}"#,
        )
        .unwrap();
        let cond = |id: &str| {
            Some(crate::panel::spec::ShowIf {
                id: id.into(),
                is: None,
                not: false,
            })
        };
        spec.widgets[0].set_show_if(cond("b"));
        spec.widgets[1].set_show_if(cond("a"));
        let f = Form::new(spec);
        assert!(!f.is_visible(0) && !f.is_visible(1));
    }

    #[test]
    fn a_button_of_a_grid_answers_like_one_of_the_bottom_row() {
        let mut f = form(
            r#"{"id":"p","widgets":[{"type":"buttons","buttons":[{"id":"play","icon":"play"},{"id":"stop","label":"Stop"}]}]}"#,
        );
        let r = f.submit("play").expect("a button of the grid");
        assert!(r.submitted());
        assert_eq!(r.button, "play");
        assert!(f.submit("nothing").is_err());
        // only a way out at the bottom: the grid is what answers
        assert_eq!(f.spec.buttons.len(), 1);
        assert!(f.spec.buttons[0].cancel);
    }

    #[test]
    fn keeping_the_panel_open_is_asked_by_the_panel_or_by_the_button() {
        let f = form(
            r#"{"widgets":[{"type":"buttons","buttons":[{"id":"a","keep_open":true},{"id":"b"}]}],
                "buttons":[{"id":"close","cancel":true,"keep_open":true},{"id":"go"}]}"#,
        );
        assert!(f.keeps_open("a"));
        assert!(!f.keeps_open("b"));
        assert!(
            !f.keeps_open("close"),
            "a button that cancels always closes"
        );
        assert!(!f.keeps_open("go"));
        let g = form(
            r#"{"keep_open":true,"widgets":[{"type":"choice","id":"c","submit":true,"options":["x"]}],
                "buttons":[{"id":"go"}]}"#,
        );
        assert!(g.keeps_open("go") && g.keeps_open("pick"));
    }
}
