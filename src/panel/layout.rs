//! How big a panel is. The window has to be the right size *before* the first frame (a window that
//! is resized after it is shown flashes on some systems, see `App::show_window`), so the height is
//! worked out from the description instead of measured while drawing: every widget has a known
//! height, a long list scrolls inside a fixed box, and the drawing code reads the same constants.
//!
//! The numbers are in points and match what `ui::settings::panel_form` draws.

use super::spec::{
    Button, ButtonAlign, ButtonSide, Layout, Opt, PADDING_MAX, PanelSpec, SPACER_DEFAULT, TextFmt,
    TextStyle, Widget,
};

/// Space between the edge of the card and its content, on a side that says nothing else.
pub const PAD: f32 = 18.0;

/// The space between the edge of the card and its content on each side (`PanelSpec::padding`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Insets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Insets {
    /// Left and right together.
    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }
}

/// The padding of `spec`, side by side: what it says, else [`PAD`].
pub fn insets(spec: &PanelSpec) -> Insets {
    let side = |v: Option<f32>| {
        v.filter(|x| x.is_finite())
            .map_or(PAD, |x| x.clamp(0.0, PADDING_MAX))
    };
    let p = &spec.padding;
    Insets {
        top: side(p.top),
        right: side(p.right),
        bottom: side(p.bottom),
        left: side(p.left),
    }
}
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
/// The size of the text inside a box when its `value_fmt` says no size (points).
pub const BOX_TEXT_SIZE: f32 = 14.0;
/// The usual space between the edge of an input box and its text: at the sides, above and below a
/// line, above and below several lines (points).
pub const BOX_PAD_X: f32 = 12.0;
pub const BOX_PAD_Y_ONE: f32 = 4.0;
pub const BOX_PAD_Y_MANY: f32 = 8.0;
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

/// How tall `button` is: what it says, or the height of the other controls.
pub fn button_height(button: &Button) -> f32 {
    if button.height == 0 {
        CONTROL_H
    } else {
        f32::from(button.height)
    }
}

/// The height of each row of a grid of `buttons` in `columns` columns: the tallest button of the
/// row (an empty grid is one row of the usual height, where the designer says it has no buttons).
pub fn grid_row_heights(columns: u8, buttons: &[Button]) -> Vec<f32> {
    let cols = group_columns(columns, buttons.len());
    let rows = group_rows(columns, buttons.len());
    (0..rows)
        .map(|r| {
            buttons
                .iter()
                .skip(r * cols)
                .take(cols)
                .map(button_height)
                .fold(0.0_f32, f32::max)
                .max(if buttons.is_empty() { CONTROL_H } else { 0.0 })
        })
        .collect()
}

/// The height of a whole grid of buttons (without its label).
pub fn grid_height(columns: u8, buttons: &[Button]) -> f32 {
    let rows = grid_row_heights(columns, buttons);
    rows.iter().sum::<f32>() + rows.len().saturating_sub(1) as f32 * BTN_GAP
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

/// The size a text widget is drawn at when its `fmt` says no size (points).
pub fn style_size(style: TextStyle) -> f32 {
    match style {
        TextStyle::Body => 14.0,
        TextStyle::Heading => 17.0,
        TextStyle::Muted | TextStyle::Code => 13.0,
    }
}

/// The usual size of the title, of the subtitle and of a label above a field (points).
pub const TITLE_SIZE: f32 = 16.0;
pub const SUBTITLE_SIZE: f32 = 13.0;
pub const LABEL_SIZE: f32 = 13.0;
/// The biggest a title, a subtitle or a label is drawn, whatever its `fmt` asks for: the header and
/// the label rows have a fixed height.
pub const TITLE_SIZE_MAX: f32 = 28.0;
pub const SUBTITLE_SIZE_MAX: f32 = 20.0;
pub const LABEL_SIZE_MAX: f32 = 20.0;

/// The size a text of `fmt` is drawn at: what it asks for, or `usual`, and never more than `most`.
pub fn text_size(fmt: &TextFmt, usual: f32, most: f32) -> f32 {
    fmt.size.map_or(usual, f32::from).min(most)
}

/// How much bigger (or smaller) than `style` a text widget is drawn because of its `fmt`.
fn text_scale(style: TextStyle, fmt: &TextFmt) -> f32 {
    fmt.size.map_or(1.0, |s| f32::from(s) / style_size(style))
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
        Widget::Text {
            text, style, fmt, ..
        } => {
            // a bigger font has fewer characters per line and taller lines
            let scale = text_scale(*style, fmt);
            text_lines(text, inner / scale, *style) as f32 * LINE_H * scale
                + if *style == TextStyle::Heading {
                    6.0
                } else {
                    0.0
                }
        }
        Widget::Input {
            label,
            multiline,
            value_fmt,
            height,
            ..
        } => {
            let label_h = if label.is_empty() { 0.0 } else { LABEL_H };
            // a box that says its height has it; otherwise a bigger text makes the box taller (a
            // box never gets lower than the usual one)
            let size = value_fmt.size.map_or(BOX_TEXT_SIZE, f32::from);
            let line = size * 1.3;
            label_h
                + match height {
                    Some(h) => *h,
                    None if *multiline => MULTILINE_H.max(4.0 * line + 16.0),
                    None => CONTROL_H.max(line + 8.0),
                }
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
        } => (if label.is_empty() { 0.0 } else { LABEL_H }) + grid_height(*columns, buttons),
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
                + title_extra(spec)
                + (text_size(&spec.subtitle_fmt, SUBTITLE_SIZE, SUBTITLE_SIZE_MAX) - SUBTITLE_SIZE)
                    .max(0.0)
                    * 1.3
        }
}

/// How much room a title bigger than the usual one takes from the subtitle under it.
pub fn title_extra(spec: &PanelSpec) -> f32 {
    (text_size(&spec.title_fmt, TITLE_SIZE, TITLE_SIZE_MAX) - TITLE_SIZE).max(0.0) * 1.3
}

/// The height of everything between the header and the buttons.
pub fn body_height(spec: &PanelSpec) -> f32 {
    // `widget_height` takes the width of the card and removes the usual padding from it; the card
    // is `left + right` of padding narrower here, so what it works with is that much less wide
    let width = spec.width_or_default() - insets(spec).horizontal() + 2.0 * PAD;
    let n = spec.widgets.len();
    spec.widgets
        .iter()
        .map(|w| widget_height(w, width))
        .sum::<f32>()
        + n.saturating_sub(1) as f32 * GAP
}

/// The height of the row of buttons at the bottom: its tallest button.
pub fn footer_row_height(spec: &PanelSpec) -> f32 {
    spec.buttons.iter().map(button_height).fold(0.0, f32::max)
}

/// Whether the card has a row of buttons at the bottom. A panel that asked for none
/// (`no_buttons`) has no row, no room for it and no gap above it.
pub fn has_footer(spec: &PanelSpec) -> bool {
    !spec.buttons.is_empty()
}

/// The room the footer takes under the body: the gap above it and the footer itself, or nothing.
pub fn footer_block(spec: &PanelSpec) -> f32 {
    if has_footer(spec) {
        GAP + footer_height(spec)
    } else {
        0.0
    }
}

/// Whether a press of a button can be refused with a notice (a required box is empty).
pub fn can_refuse(spec: &PanelSpec) -> bool {
    spec.widgets
        .iter()
        .any(|w| matches!(w, Widget::Input { required: true, .. }))
}

/// The height of the row of buttons, with the line for a notice under it when the panel can have
/// one (without such a line the buttons are as close to the bottom as the padding says).
pub fn footer_height(spec: &PanelSpec) -> f32 {
    if !has_footer(spec) {
        return 0.0;
    }
    footer_row_height(spec) + if can_refuse(spec) { NOTICE_H } else { 0.0 }
}

/// Where each button of the row at the bottom goes in a row `row_w` wide: `(x, width)` from the
/// left edge of the row, in the order of `spec.buttons`. `natural` is how wide each one is when it
/// takes what its text needs (measured by the code that draws; a button with its own `width` does
/// not use it).
///
/// The buttons that sit on the same side (`Button::side`, else what the panel's `buttons_align`
/// says) form a group: the left one against the left edge, the right one against the right edge,
/// the middle one in the middle (moved aside when it would touch another). With `Stretch` and no
/// side of their own the buttons share the row. When everything is too wide for the row the
/// widths shrink alike.
pub fn footer_slots(spec: &PanelSpec, natural: &[f32], row_w: f32) -> Vec<(f32, f32)> {
    let buttons = &spec.buttons;
    let n = buttons.len();
    let own = |i: usize| (buttons[i].width != 0).then(|| f32::from(buttons[i].width));
    let natural_of = |i: usize| natural.get(i).copied().unwrap_or(CONTROL_H);
    let stretch = spec.buttons_align == ButtonAlign::Stretch;
    if n == 0 {
        return Vec::new();
    }
    // sides: a button that says nothing follows `buttons_align` (which has no side when it stretches)
    let default_side = match spec.buttons_align {
        ButtonAlign::Start => Some(ButtonSide::Start),
        ButtonAlign::Center => Some(ButtonSide::Center),
        ButtonAlign::End => Some(ButtonSide::End),
        ButtonAlign::Stretch => None,
    };
    let side_of = |i: usize| buttons[i].side.or(default_side);
    let gaps = BTN_GAP * n.saturating_sub(1) as f32;
    let mut widths: Vec<f32> = if stretch {
        let fixed: f32 = (0..n).filter_map(own).sum();
        let free = (0..n).filter(|i| own(*i).is_none()).count();
        let share = if free == 0 {
            0.0
        } else {
            ((row_w - gaps - fixed) / free as f32).max(24.0)
        };
        (0..n).map(|i| own(i).unwrap_or(share)).collect()
    } else {
        (0..n)
            .map(|i| own(i).unwrap_or_else(|| natural_of(i)))
            .collect()
    };
    // the groups, left to right
    let groups: [Vec<usize>; 3] =
        [ButtonSide::Start, ButtonSide::Center, ButtonSide::End].map(|want| {
            (0..n)
                .filter(|&i| side_of(i).unwrap_or(ButtonSide::Start) == want)
                .collect()
        });
    let used = groups.iter().filter(|g| !g.is_empty()).count();
    let spans = |widths: &[f32]| -> [f32; 3] {
        let mut out = [0.0; 3];
        for (k, g) in groups.iter().enumerate() {
            out[k] = g.iter().map(|&i| widths[i]).sum::<f32>()
                + g.len().saturating_sub(1) as f32 * BTN_GAP;
        }
        out
    };
    let total = |w: &[f32]| spans(w).iter().sum::<f32>() + used.saturating_sub(1) as f32 * BTN_GAP;
    let all = total(&widths);
    if all > row_w && all > 0.0 {
        // too many words for the row: all of them shrink alike
        let room = row_w - (all - widths.iter().sum::<f32>());
        let scale = (room / widths.iter().sum::<f32>()).clamp(0.2, 1.0);
        for w in &mut widths {
            *w *= scale;
        }
    }
    let span = spans(&widths);
    let left_end = if groups[0].is_empty() {
        0.0
    } else {
        span[0] + BTN_GAP
    };
    let right_start = if groups[2].is_empty() {
        row_w
    } else {
        row_w - span[2] - BTN_GAP
    };
    let center_at = ((row_w - span[1]) / 2.0)
        .min(right_start - span[1])
        .max(left_end);
    let starts = [0.0, center_at, row_w - span[2]];
    let mut out = vec![(0.0, 0.0); n];
    for (k, g) in groups.iter().enumerate() {
        let mut x = starts[k];
        for &i in g {
            out[i] = (x, widths[i]);
            x += widths[i] + BTN_GAP;
        }
    }
    out
}

/// The height the content needs, with the header, the buttons and the padding.
pub fn content_height(spec: &PanelSpec) -> f32 {
    let body = body_height(spec);
    let gap_after_header = if spec.widgets.is_empty() {
        0.0
    } else {
        GAP + 4.0
    };
    let ins = insets(spec);
    let h =
        ins.top + header_height(spec) + gap_after_header + body + footer_block(spec) + ins.bottom;
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
    use crate::panel::Padding;

    fn spec(json: &str) -> PanelSpec {
        PanelSpec::parse(json).expect("a good panel")
    }

    #[test]
    fn the_padding_of_each_side_decides_the_height_and_the_room_of_the_content() {
        let base = spec(r#"{"title":"x","widgets":[{"type":"input","id":"a"}]}"#);
        let tight = spec(
            r#"{"title":"x","padding":{"top":0,"bottom":0},"widgets":[{"type":"input","id":"a"}]}"#,
        );
        assert_eq!(content_height(&base) - content_height(&tight), 2.0 * PAD);
        let ins = insets(&tight);
        assert_eq!(
            (ins.top, ins.right, ins.bottom, ins.left),
            (0.0, PAD, 0.0, PAD)
        );
        // only what is said changes, and a wild number is held to the limits
        let wild = spec(r#"{"padding":{"left":500,"right":-3}}"#);
        let ins = insets(&wild);
        assert_eq!((ins.left, ins.right, ins.top), (PADDING_MAX, 0.0, PAD));
        assert_eq!(insets(&base), insets(&PanelSpec::default()));
    }

    #[test]
    fn a_panel_without_buttons_has_no_footer_and_is_shorter() {
        let with = spec(r#"{"title":"x"}"#);
        let without = spec(r#"{"title":"x","no_buttons":true}"#);
        assert!(has_footer(&with) && !has_footer(&without));
        assert_eq!(footer_height(&without), 0.0);
        assert_eq!(footer_block(&without), 0.0);
        assert!(content_height(&with) > content_height(&without));
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
    fn a_bigger_font_makes_a_text_taller() {
        let text = |size: Option<u8>| Widget::Text {
            id: String::new(),
            text: "word ".repeat(30),
            style: TextStyle::Body,
            fmt: TextFmt {
                size,
                ..TextFmt::default()
            },
            show_if: None,
        };
        let usual = widget_height(&text(None), 380.0);
        assert!(widget_height(&text(Some(28)), 380.0) > usual);
        assert!(widget_height(&text(Some(9)), 380.0) < usual);
        // asking for the size the style has anyway changes nothing
        assert_eq!(widget_height(&text(Some(14)), 380.0), usual);
    }

    #[test]
    fn a_bigger_text_makes_a_box_taller_but_never_lower_than_usual() {
        let input = |multiline: bool, size: Option<u8>| Widget::Input {
            id: "i".into(),
            label: String::new(),
            hint: String::new(),
            value: String::new(),
            secret: false,
            multiline,
            required: false,
            max_chars: None,
            value_fmt: TextFmt {
                size,
                ..TextFmt::default()
            },
            width: None,
            height: None,
            text_padding: Padding::default(),
            show_if: None,
        };
        for multiline in [false, true] {
            let usual = widget_height(&input(multiline, None), 380.0);
            assert!(widget_height(&input(multiline, Some(40)), 380.0) > usual);
            assert_eq!(widget_height(&input(multiline, Some(9)), 380.0), usual);
        }
        assert_eq!(widget_height(&input(true, None), 380.0), MULTILINE_H);
        assert_eq!(widget_height(&input(false, None), 380.0), CONTROL_H);
        // a box that says its height has exactly that
        let tall = Widget::Input {
            id: "i".into(),
            label: String::new(),
            hint: String::new(),
            value: String::new(),
            secret: false,
            multiline: true,
            required: false,
            max_chars: None,
            value_fmt: TextFmt::default(),
            width: Some(200.0),
            height: Some(150.0),
            text_padding: Padding::default(),
            show_if: None,
        };
        assert_eq!(widget_height(&tall, 380.0), 150.0);
    }

    #[test]
    fn a_bigger_title_makes_the_header_taller_only_with_a_subtitle() {
        let mut spec = PanelSpec::default();
        spec.title_fmt.size = Some(28);
        assert_eq!(header_height(&spec), HEADER_H);
        spec.subtitle = "under".into();
        let big = header_height(&spec);
        spec.title_fmt.size = None;
        assert!(big > header_height(&spec));
        assert_eq!(header_height(&spec), HEADER_H + SUBTITLE_H);
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
    fn buttons_that_say_their_height_make_their_row_that_tall() {
        let s = spec(
            r#"{"widgets":[{"type":"buttons","columns":2,"buttons":[
                {"id":"a","label":"A","height":48},{"id":"b","label":"B"},
                {"id":"c","label":"C","height":28}]}],
                "buttons":[{"id":"ok","label":"OK","height":52}]}"#,
        );
        let Widget::Buttons {
            columns, buttons, ..
        } = &s.widgets[0]
        else {
            panic!("a grid");
        };
        assert_eq!(grid_row_heights(*columns, buttons), vec![48.0, 28.0]);
        assert_eq!(grid_height(*columns, buttons), 48.0 + BTN_GAP + 28.0);
        assert_eq!(footer_row_height(&s), 52.0);
        assert_eq!(
            footer_height(&s),
            52.0,
            "no required box, no line for a notice"
        );
        let required = spec(
            r#"{"widgets":[{"type":"input","id":"a","required":true}],"buttons":[{"id":"ok"}]}"#,
        );
        assert_eq!(footer_height(&required), CONTROL_H + NOTICE_H);
        let plain = spec(r#"{"buttons":[{"id":"ok","label":"OK"}]}"#);
        assert_eq!(footer_row_height(&plain), CONTROL_H);
    }

    #[test]
    fn the_row_at_the_bottom_puts_each_button_on_its_side() {
        // by default everything is at the right, as it always was
        let s = spec(r#"{"buttons":[{"id":"a","label":"A"},{"id":"b","label":"B"}]}"#);
        let slots = footer_slots(&s, &[60.0, 80.0], 400.0);
        assert_eq!(slots, vec![(400.0 - 148.0, 60.0), (400.0 - 80.0, 80.0)]);
        // one on each side, one in the middle
        let s = spec(
            r#"{"buttons":[{"id":"a","label":"A","side":"end"},
                {"id":"b","label":"B","side":"start"},{"id":"c","label":"C","side":"center"}]}"#,
        );
        let slots = footer_slots(&s, &[60.0, 70.0, 80.0], 400.0);
        assert_eq!(slots[1], (0.0, 70.0), "left");
        assert_eq!(slots[2], (160.0, 80.0), "middle");
        assert_eq!(slots[0], (340.0, 60.0), "right");
    }

    #[test]
    fn a_button_that_says_its_width_keeps_it_and_the_middle_makes_room() {
        let s = spec(
            r#"{"buttons":[{"id":"a","label":"A","side":"start","width":150},
                {"id":"b","label":"B","side":"center","width":100}]}"#,
        );
        let slots = footer_slots(&s, &[40.0, 40.0], 300.0);
        assert_eq!(slots[0], (0.0, 150.0));
        // the middle would be at 100, on the first: it goes beside it
        assert_eq!(slots[1], (150.0 + BTN_GAP, 100.0));
    }

    #[test]
    fn stretched_buttons_share_what_the_wide_ones_leave() {
        let s = spec(
            r#"{"buttons_align":"stretch","buttons":[{"id":"a","label":"A","width":100},
                {"id":"b","label":"B"},{"id":"c","label":"C"}]}"#,
        );
        let slots = footer_slots(&s, &[40.0; 3], 316.0);
        assert_eq!(slots[0], (0.0, 100.0));
        let share = (316.0 - 2.0 * BTN_GAP - 100.0) / 2.0;
        assert_eq!(slots[1], (100.0 + BTN_GAP, share));
        assert_eq!(slots[2], (100.0 + BTN_GAP + share + BTN_GAP, share));
    }

    #[test]
    fn a_row_that_is_too_full_shrinks_and_stays_inside() {
        let s = spec(r#"{"buttons":[{"id":"a","label":"A"},{"id":"b","label":"B"}]}"#);
        let slots = footer_slots(&s, &[300.0, 300.0], 200.0);
        let end = slots.iter().map(|(x, w)| x + w).fold(0.0_f32, f32::max);
        assert!(end <= 200.0 + 0.01, "{slots:?}");
        assert!(slots[0].0 >= 0.0);
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
