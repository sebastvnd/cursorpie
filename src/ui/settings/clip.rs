//! Cut, copy and paste in the editors of Settings (the canvas of a flow, the panel designer).
//!
//! egui does not report Ctrl/Cmd + X, C and V as keys: the window turns them into events of their
//! own (`Event::Cut`, `Event::Copy` and `Event::Paste(text)`, the last one carrying what the system
//! clipboard holds), so `consume_key(COMMAND, Key::C)` never fires. [`take`] reads those events.
//!
//! What an editor copies goes to the *system* clipboard as text that starts with a marker of its
//! own (see [`Flow::copy_nodes`](crate::config::flow::Flow::copy_nodes) and
//! [`design::copy_widgets`](crate::panel::design::copy_widgets)). That is what lets a copy cross
//! from one flow to another, or from one panel to another, and a paste tell its own text from
//! anything else the person copied elsewhere. A paste event only comes when the clipboard holds
//! text, so a copy that could not reach the clipboard cannot be pasted either.

use eframe::egui;

/// What the keyboard asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui::settings) enum Clip {
    Copy,
    Cut,
    /// The text of the system clipboard.
    Paste(String),
}

/// Takes the cut, copy or paste of this frame, so nothing else reacts to it. Call it only while
/// the editor owns the keyboard (no text field is being typed in): a text field handles these
/// events itself.
pub(in crate::ui::settings) fn take(ctx: &egui::Context) -> Option<Clip> {
    ctx.input_mut(|i| {
        let at = i.events.iter().position(|e| {
            matches!(
                e,
                egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_)
            )
        })?;
        match i.events.remove(at) {
            egui::Event::Copy => Some(Clip::Copy),
            egui::Event::Cut => Some(Clip::Cut),
            egui::Event::Paste(text) => Some(Clip::Paste(text)),
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_with(events: Vec<egui::Event>) -> Vec<Option<Clip>> {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut got = Vec::new();
        ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            got.push(take(&ctx));
            got.push(take(&ctx));
        })
        .drop_without_applying_deltas();
        got
    }

    #[test]
    fn copy_cut_and_paste_events_are_taken_once_and_in_order() {
        let got = run_with(vec![egui::Event::Paste("abc".into()), egui::Event::Copy]);
        assert_eq!(got, vec![Some(Clip::Paste("abc".into())), Some(Clip::Copy)]);
        let got = run_with(vec![egui::Event::Cut]);
        assert_eq!(got, vec![Some(Clip::Cut), None]);
        let got = run_with(vec![]);
        assert_eq!(got, vec![None, None]);
    }
}
