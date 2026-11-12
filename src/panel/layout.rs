//! How big a panel is. The window has to be the right size *before* the first frame (a window that
//! is resized after it is shown flashes on some systems, see `App::show_window`), so the height is
//! worked out from the description instead of measured while drawing: every widget has a known
//! height, a long list scrolls inside a fixed box, and the drawing code reads the same constants.
//!
//! The numbers are in points and match what `ui::settings::panel_form` draws.

use super::spec::{Layout, Opt, PanelSpec, SPACER_DEFAULT, TextStyle, Widget};

/// Space between the edge of the card and its content.
pub const PAD: f32 = 18.0;
/// Space between two widgets.
pub const GAP: f32 = 12.0;
/// Height of the header: the badge and the title.
pub const HEADER_H: f32 = 40.0;
/// Extra height of the line under the title.
pub const SUBTITLE_H: f32 = 20.0;
/// Height of a button, a text box and a switch row (the controls of Settings).
pub const CONTROL_H: f32 = 36.0;
/// Height of the label above a field.
pub const LABEL_H: f32 = 24.0;
/// Height of one line of text.
pub const LINE_H: f32 = 20.0;
/// Height of a box with several lines.
pub const MULTILINE_H: f32 = 92.0;
/// Height of a row of a list: without a hint line, and with one.
pub const ROW_H: f32 = 36.0;
pub const ROW_HINT_H: f32 = 52.0;
/// Rows of a list that are visible before it scrolls, for a choice and for a multi-choice.
pub const LIST_ROWS: usize = 8;
pub const MULTI_ROWS: usize = 6;
/// Height of a round chip and of the space between two lines of chips.
pub const CHIP_H: f32 = 32.0;
pub const CHIP_GAP: f32 = 8.0;
/// Height of the bar of a progress widget, and of a separator.
pub const BAR_H: f32 = 8.0;
pub const SEPARATOR_H: f32 = 9.0;
/// Height of the line that says why a button did nothing.
pub const NOTICE_H: f32 = 22.0;
/// A list with more options than this gets a search box by default.
pub const SEARCH_FROM: usize = 8;
/// Space between two buttons of a grid, across and down, and between the buttons of the bottom row.
pub const BTN_GAP: f32 = 8.0;
/// Buttons per row of a grid that does not say (as many as there are, at most this).
pub const AUTO_COLUMNS: usize = 4;

/// Buttons per row of a grid of `count` buttons whose description says `columns` (0: not said).
/// Never more than there are buttons, never less than 1.
pub fn group_columns(columns: u8, count: usize) -> usize {
    let wanted = if columns == 0 {
        count.min(AUTO_COLUMNS)
    } else {
        usize::from(columns)
    };
    wanted.min(count).max(1)
}

/// Rows a grid of `count` buttons takes in `columns` columns. An empty grid still takes one row,
/// where the designer and the panel say that it has no buttons.
pub fn group_rows(columns: u8, count: usize) -> usize {
    count.div_ceil(group_columns(columns, count)).max(1)
}

/// The width in points that a character of the body text takes, on average (for guessing lines).
const CHAR_W: f32 = 7.2;

/// Whether the list `widget` has a search box.
pub fn has_search(search: Option<bool>, options: usize) -> bool {
    search.unwrap_or(options > SEARCH_FROM)
}

fn row_height(o: &Opt) -> f32 {
    if o.hint.is_empty() { ROW_H } else { ROW_HINT_H }
}

/// The height of the scrolling box of a list with `options` and `rows` visible rows.
pub fn list_height(options: &[Opt], rows: usize) -> f32 {
    options.iter().take(rows).map(row_height).sum::<f32>()
        + options.iter().take(rows).count().saturating_sub(1) as f32 * 2.0
}

/// How many lines of chips `options` take in a box `inner` wide.
fn chip_lines(options: &[Opt], inner: f32) -> usize {
    let mut lines = 1;
    let mut x = 0.0;
    for o in options {
        let w = o.label.chars().count() as f32 * CHAR_W + 28.0;
        if x > 0.0 && x + w > inner {
            lines += 1;
            x = 0.0;
        }
        x += w + CHIP_GAP;
    }
    lines
}

/// How many lines a text takes in a box `inner` wide.
pub fn text_lines(text: &str, inner: f32, style: TextStyle) -> usize {
    let char_w = match style {
        TextStyle::Heading => CHAR_W * 1.2,
        TextStyle::Code => CHAR_W * 1.05,
        _ => CHAR_W,
    };
    let per_line = ((inner / char_w).floor() as usize).max(8);
    text.split('\n')
        .map(|p| p.chars().count().div_ceil(per_line).max(1))
        .sum()
}

/// The height of one widget when the card is `width` wide.
pub fn widget_height(w: &Widget, width: f32) -> f32 {
    let inner = width - 2.0 * PAD;
    match w {
        Widget::Separator { .. } => SEPARATOR_H,
        Widget::Text { text, style, .. } => {
            text_lines(text, inner, *style) as f32 * LINE_H
                + if *style == TextStyle::Heading {
                    6.0
                } else {
                    0.0
                }
        }
        Widget::Input {
            label, multiline, ..
        } => {
            let label_h = if label.is_empty() { 0.0 } else { LABEL_H };
            label_h + if *multiline { MULTILINE_H } else { CONTROL_H }
        }
        Widget::Toggle { .. } => CONTROL_H,
        Widget::Number { label, .. } | Widget::Color { label, .. } => {
            (if label.is_empty() { 0.0 } else { LABEL_H }) + CONTROL_H
        }
        Widget::Slider { label, .. } => {
            let label_h = if label.is_empty() { 0.0 } else { LABEL_H };
            label_h + 46.0
        }
        Widget::Choice {
            label,
            options,
            layout,
            search,
            ..
        } => {
            let label_h = if label.is_empty() { 0.0 } else { LABEL_H };
            let search_h = if has_search(*search, options.len()) && *layout != Layout::Dropdown {
                CONTROL_H + 6.0
            } else {
                0.0
            };
            let body = match layout {
                Layout::Dropdown => CONTROL_H,
                Layout::List => list_height(options, LIST_ROWS),
                Layout::Chips => {
                    let n = chip_lines(options, inner);
                    n as f32 * CHIP_H + n.saturating_sub(1) as f32 * CHIP_GAP
                }
            };
            label_h + search_h + body
        }
        Widget::Multi { label, options, .. } => {
            let label_h = if label.is_empty() { 0.0 } else { LABEL_H };
            let search_h = if options.len() > SEARCH_FROM {
                CONTROL_H + 6.0
            } else {
                0.0
            };
            label_h + search_h + list_height(options, MULTI_ROWS)
        }
        Widget::Progress { label, .. } => (if label.is_empty() { 0.0 } else { LABEL_H }) + BAR_H,
        Widget::Buttons {
            label,
            columns,
            buttons,
            ..
        } => {
            let rows = group_rows(*columns, buttons.len());
            (if label.is_empty() { 0.0 } else { LABEL_H })
                + rows as f32 * CONTROL_H
                + rows.saturating_sub(1) as f32 * BTN_GAP
        }
        Widget::Spacer { height, .. } => height.unwrap_or(SPACER_DEFAULT),
    }
}

/// The height of the header.
pub fn header_height(spec: &PanelSpec) -> f32 {
    HEADER_H
        + if spec.subtitle.is_empty() {
            0.0
        } else {
            SUBTITLE_H
        }
}

/// The height of everything between the header and the buttons.
pub fn body_height(spec: &PanelSpec) -> f32 {
    let width = spec.width_or_default();
    let n = spec.widgets.len();
    spec.widgets
        .iter()
        .map(|w| widget_height(w, width))
        .sum::<f32>()
        + n.saturating_sub(1) as f32 * GAP
}

/// The height of the row of buttons, with the line for a notice under it.
pub const fn footer_height() -> f32 {
    CONTROL_H + NOTICE_H
}

/// The height the content needs, with the header, the buttons and the padding.
pub fn content_height(spec: &PanelSpec) -> f32 {
    let body = body_height(spec);
    let gap_after_header = if spec.widgets.is_empty() {
        0.0
    } else {
        GAP + 4.0
    };
    let h = PAD + header_height(spec) + gap_after_header + body + GAP + footer_height() + PAD;
    h.ceil()
}

/// The size of the card: its width, and its height: the one the panel asks for (`height`), or
/// else what all of its content needs.
pub fn card_size(spec: &PanelSpec) -> (f32, f32) {
    let h = spec.height.unwrap_or_else(|| content_height(spec));
    (spec.width_or_default(), h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(json: &str) -> PanelSpec {
        PanelSpec::parse(json).expect("a good panel")
    }

    #[test]
    fn a_fixed_height_wins_and_without_one_the_content_decides() {
        let auto = spec(r#"{"title":"x","widgets":[{"type":"input","id":"a","label":"A"}]}"#);
        assert_eq!(card_size(&auto).1, content_height(&auto));
        let fixed = spec(r#"{"title":"x","height":600,"widgets":[{"type":"input","id":"a"}]}"#);
        assert_eq!(card_size(&fixed), (fixed.width_or_default(), 600.0));
        let short = spec(r#"{"title":"x","height":10}"#);
        assert_eq!(card_size(&short).1, crate::panel::spec::HEIGHT_MIN);
    }

    #[test]
    fn every_widget_adds_height_and_the_card_never_shrinks_below_its_chrome() {
        let none = card_size(&spec(r#"{"title":"x"}"#)).1;
        let one = card_size(&spec(
            r#"{"widgets":[{"type":"input","id":"a","label":"A"}]}"#,
        ))
        .1;
        let two = card_size(&spec(
            r#"{"widgets":[{"type":"input","id":"a","label":"A"},{"type":"toggle","id":"b"}]}"#,
        ))
        .1;
        assert!(none > 100.0, "header and buttons");
        assert!(one > none && two > one);
    }

    #[test]
    fn a_long_list_scrolls_inside_a_box_of_fixed_height() {
        let few = widget_height(
            &spec(r#"{"widgets":[{"type":"choice","id":"c","options":["a","b","c"],"search":false}]}"#).widgets[0],
            380.0,
        );
        assert_eq!(few, 3.0 * ROW_H + 2.0 * 2.0);
        let many: Vec<String> = (0..50).map(|i| format!("\"o{i}\"")).collect();
        let s = spec(&format!(
            r#"{{"widgets":[{{"type":"choice","id":"c","options":[{}]}}]}}"#,
            many.join(",")
        ));
        let h = widget_height(&s.widgets[0], 380.0);
        assert_eq!(
            h,
            CONTROL_H + 6.0 + list_height(&vec![Opt::new("x"); 8], LIST_ROWS)
        );
    }

    #[test]
    fn text_height_follows_the_number_of_lines() {
        assert_eq!(text_lines("short", 300.0, TextStyle::Body), 1);
        assert_eq!(text_lines("a\nb\nc", 300.0, TextStyle::Body), 3);
        assert!(text_lines(&"word ".repeat(100), 300.0, TextStyle::Body) > 5);
    }

    #[test]
    fn chips_wrap_onto_more_lines_in_a_narrow_card() {
        let options: Vec<Opt> = (0..10).map(|i| Opt::new(format!("option {i}"))).collect();
        let mut opts = options.clone();
        for o in &mut opts {
            o.label = o.value.clone();
        }
        assert!(chip_lines(&opts, 200.0) > chip_lines(&opts, 600.0));
        assert_eq!(chip_lines(&[], 300.0), 1);
    }

    #[test]
    fn the_search_box_is_automatic_for_long_lists_only() {
        assert!(!has_search(None, SEARCH_FROM));
        assert!(has_search(None, SEARCH_FROM + 1));
        assert!(has_search(Some(true), 2));
        assert!(!has_search(Some(false), 50));
    }

    #[test]
    fn a_grid_of_buttons_is_as_tall_as_its_rows() {
        let one = widget_height(
            &spec(r#"{"widgets":[{"type":"buttons","buttons":[{"id":"a"},{"id":"b"}]}]}"#).widgets
                [0],
            380.0,
        );
        assert_eq!(one, CONTROL_H, "two buttons are one row");
        let many: Vec<String> = (0..9).map(|i| format!(r#"{{"id":"b{i}"}}"#)).collect();
        let s = spec(&format!(
            r#"{{"widgets":[{{"type":"buttons","columns":3,"label":"L","buttons":[{}]}}]}}"#,
            many.join(",")
        ));
        let three = widget_height(&s.widgets[0], 380.0);
        assert_eq!(three, LABEL_H + 3.0 * CONTROL_H + 2.0 * BTN_GAP);
        assert_eq!(group_columns(0, 9), AUTO_COLUMNS);
        assert_eq!(group_columns(0, 2), 2);
        assert_eq!(group_columns(5, 2), 2);
        assert_eq!(group_columns(0, 0), 1);
        assert_eq!(group_rows(0, 0), 1, "an empty grid says so on one row");
        assert_eq!(group_rows(4, 9), 3);
    }

    #[test]
    fn a_spacer_is_as_tall_as_it_says() {
        let s = spec(r#"{"widgets":[{"type":"spacer","height":30},{"type":"spacer"}]}"#);
        assert_eq!(widget_height(&s.widgets[0], 380.0), 30.0);
        assert_eq!(widget_height(&s.widgets[1], 380.0), SPACER_DEFAULT);
    }

    #[test]
    fn a_dropdown_and_a_number_are_one_control_high() {
        let drop = widget_height(
            &spec(r#"{"widgets":[{"type":"choice","id":"c","layout":"dropdown","options":["a","b","c","d","e","f","g","h","i","j"]}]}"#).widgets[0],
            380.0,
        );
        assert_eq!(
            drop, CONTROL_H,
            "no label, no search box, however many options"
        );
        let num = widget_height(
            &spec(r#"{"widgets":[{"type":"number","id":"n","label":"N"}]}"#).widgets[0],
            380.0,
        );
        assert_eq!(num, LABEL_H + CONTROL_H);
        let col = widget_height(
            &spec(r#"{"widgets":[{"type":"color","id":"c"}]}"#).widgets[0],
            380.0,
        );
        assert_eq!(col, CONTROL_H);
    }
}
