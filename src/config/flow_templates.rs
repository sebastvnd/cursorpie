//! Ready-made flows for the browser extension, offered by **New flow > Start from a template**.
//!
//! A flow that controls the browser needs three things that a newcomer has to get right at once: a
//! panel (or a question) to choose what to do, a *Branch* per button, and a *Browser command* block
//! with the exact name of a command. These templates are complete, working flows to start from: the
//! person adds one, sees how it is wired, and changes what they want.
//!
//! Every template is a flow file of `docs/examples/flows/` (so it can also be imported by hand),
//! built into the program. A test loads each one, checks it like the editor does (no warning, no
//! error), and checks that every browser command and every panel in it is valid.

use super::flow::{Flow, fresh_id, parse_flow, unique_name};

/// One template: what the sheet shows, and the flow file it makes a flow from.
#[derive(Clone, Copy, Debug)]
pub struct Template {
    pub title: &'static str,
    /// One line under the title.
    pub about: &'static str,
    /// The flow file.
    pub json: &'static str,
}

/// Every template, in the order of the sheet.
pub const TEMPLATES: [Template; 3] = [
    Template {
        title: "Browser remote",
        about: "A panel with play, next, previous and volume that stays open between presses.",
        json: include_str!("../../docs/examples/flows/browser-remote-panel.cursorpie-flow.json"),
    },
    Template {
        title: "Pause the music only if it plays",
        about: "Asks the browser whether something plays, and pauses it or says that nothing does.",
        json: include_str!(
            "../../docs/examples/flows/browser-pause-if-playing.cursorpie-flow.json"
        ),
    },
    Template {
        title: "Go to the tab that has a word",
        about: "Asks for a word, finds the tab with it in its title or address, and says when none has.",
        json: include_str!("../../docs/examples/flows/browser-go-to-tab.cursorpie-flow.json"),
    },
];

/// A new flow from `template`, with an id and a name that no flow of `existing` has.
pub fn build(template: &Template, existing: &[Flow]) -> Result<Flow, String> {
    let mut flow = parse_flow(template.json)?;
    flow.id = fresh_id(existing);
    flow.name = unique_name(existing, &flow.name);
    Ok(flow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::flow::{AskMode, NativeOp, NodeKind};
    use crate::config::flow_check::check_flow;
    use crate::panel::PanelSpec;

    #[test]
    fn every_template_is_a_valid_flow_with_nothing_to_warn_about() {
        for t in TEMPLATES {
            let flow = parse_flow(t.json).unwrap_or_else(|e| panic!("{}: {e}", t.title));
            assert_eq!(
                flow.reachable().len(),
                flow.nodes.len(),
                "{}: an unreachable block",
                t.title
            );
            let issues = check_flow(&flow, &[]);
            assert!(issues.is_empty(), "{}: {issues:?}", t.title);
            assert!(!t.about.is_empty());
        }
    }

    #[test]
    fn every_browser_command_in_a_template_is_one_the_extension_knows() {
        for t in TEMPLATES {
            let flow = parse_flow(t.json).unwrap();
            let mut seen = 0;
            for n in &flow.nodes {
                if let NodeKind::Native {
                    op: NativeOp::Browser,
                    a,
                    ..
                } = &n.kind
                {
                    seen += 1;
                    // a command written out is checked with its text; `{{word}}` stands for one
                    let line = a.replace("{{word}}", "x");
                    crate::bridge::commands::parse(&line)
                        .unwrap_or_else(|e| panic!("{}: `{a}`: {e}", t.title));
                }
            }
            assert!(seen >= 1, "{}: it talks to the browser", t.title);
        }
    }

    #[test]
    fn the_panel_of_the_remote_is_valid_and_every_button_of_it_has_its_branch_and_command() {
        let flow = parse_flow(TEMPLATES[0].json).unwrap();
        let message = flow
            .nodes
            .iter()
            .find_map(|n| match &n.kind {
                NodeKind::Ask {
                    mode: AskMode::Panel,
                    message,
                    ..
                } => Some(message.clone()),
                _ => None,
            })
            .expect("an Ask block in Panel mode");
        let spec = PanelSpec::parse(&message).expect("a valid panel");
        assert!(spec.keep_open, "a remote stays open");
        let branches: Vec<String> = flow
            .nodes
            .iter()
            .filter_map(|n| match &n.kind {
                NodeKind::Branch(c) if c.a == "button" => Some(c.b.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(branches.len(), 6, "{branches:?}");
        // each button of the grids is answered by a branch with its id
        for id in ["prev", "toggle", "next", "vol_down", "mute", "vol_up"] {
            assert!(branches.iter().any(|b| b == id), "no branch for {id}");
            assert!(message.contains(&format!("\"{id}\"")), "no button {id}");
        }
    }

    #[test]
    fn a_template_makes_a_new_flow_with_its_own_id_and_name() {
        let mut flows = Vec::new();
        for _ in 0..2 {
            let f = build(&TEMPLATES[1], &flows).unwrap();
            assert!(
                flows
                    .iter()
                    .all(|o: &Flow| o.id != f.id && o.name != f.name)
            );
            flows.push(f);
        }
        assert_eq!(flows[0].name, "Pause the music only if it plays");
        assert_eq!(flows[1].name, "Pause the music only if it plays (2)");
    }
}
