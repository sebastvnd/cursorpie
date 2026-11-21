//! **Flow Hub**: flows the website hands to the app through the browser extension.
//!
//! The website (`flowhub/` of the repository) lists flows the author wrote. Its *Install* button talks to
//! the CursorPie Bridge extension (`assets/browser-extension/flowhub-page.js`), which sends the text of
//! the flow file to the app over the same native-messaging link as everything else
//! ([`FromBrowser::FlowInstall`](crate::bridge::proto::FromBrowser::FlowInstall)). Without the extension,
//! or with the app not running, the page downloads the file and the person imports it by hand, as always.
//!
//! ```text
//! page --postMessage--> content script --runtime--> extension --native messaging--> app
//!                                                                                     |
//!   installed / declined / ...  <------------------ flow_result <---------------------+
//! ```
//!
//! What this module decides, in order:
//!
//! 1. **May it?** [`allowed`]: the browser link is on and so is *Flow Hub installs* (Settings > Behavior).
//! 2. **Is it a flow?** [`review`] reads the text exactly like **Import** in the Flow tab
//!    ([`parse_flow_report`]): the same limits, the same repairs, the same notes.
//! 3. **Does the person agree?** [`confirm_spec`] describes a panel that names the flow, what it can do
//!    ([`risks`]: shell commands, files, the browser) and what the import repaired. Nothing is added
//!    before the person presses *Install*; a page cannot skip that, it only asks.
//! 4. **Add it.** The UI thread adds the flow as a **new** one ([`add_flow`](crate::config::flow::add_flow):
//!    new id, unique name) and saves, or, while Settings is open, adds it to the Settings copy where it
//!    waits for **Save** like any import (`ui::app::messages`).
//!
//! The flow never runs by itself: it only runs when a slice is set to it.

use crate::config::flow::{ImportReport, MAX_FLOW_FILE_BYTES, parse_flow_report};
use crate::config::{NativeOp, NodeKind, cfg};
use crate::msg::{Msg, Notifier};
use crate::panel::{Outcome, PanelResult, PanelSpec, Source};
use serde_json::json;
use std::sync::OnceLock;
use std::sync::mpsc;
use std::time::Duration;

/// The most text of a flow file taken from a page: the limit of an import.
pub const MAX_FLOW_TEXT: usize = MAX_FLOW_FILE_BYTES as usize;

/// The Flow Hub page of the website: where the flows are listed (and where the extension serves
/// its content script, see `assets/browser-extension/manifest.*.json`).
pub const URL: &str = "https://sebastvnd.github.io/cursorpie/flowhub/";

/// What **New flow** says about the way Flow Hub will behave: whether *Install* will reach this app,
/// and a sentence for it. `browsers` are the connected browsers (see [`crate::bridge::Status`]).
pub fn link_line(running: bool, browsers: &[String], install_allowed: bool) -> (bool, String) {
    if browsers.is_empty() {
        let why = if running {
            "Extension not connected"
        } else {
            "Browser link is off"
        };
        return (false, format!("{why}: Flow Hub will download the file."));
    }
    if !install_allowed {
        return (
            false,
            "Extension connected, but installs from Flow Hub are off (Settings > Behavior).".into(),
        );
    }
    (
        true,
        format!(
            "Extension connected ({}): Install adds a flow in one tap.",
            browsers.join(", ")
        ),
    )
}

/// The `id` of the confirmation panel. A second offer replaces the first one, which answers as
/// declined.
pub const PANEL_ID: &str = "flowhub-install";

/// The id of the button that confirms.
pub const INSTALL_BUTTON: &str = "install";

/// How long the UI thread has to add a confirmed flow before the browser is told it did not.
const UI_WAIT: Duration = Duration::from_secs(10);

static NOTIFIER: OnceLock<Notifier> = OnceLock::new();

/// Gives [`offer`] a way to reach the UI. Call once, when the UI exists.
pub fn set_notifier(n: Notifier) {
    let _ = NOTIFIER.set(n);
}

/// What became of an offer. [`Answer::state`] and [`Answer::message`] are what the browser (and the
/// page) get.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    /// Added and saved under this name.
    Installed(String),
    /// Added to the Settings that are open, under this name; it is kept when the person presses Save.
    InSettings(String),
    /// The person said no (or did not answer in time).
    Declined,
    /// Installing from the website is switched off.
    Disabled(String),
    /// What came is not a usable flow file; the sentence says why.
    Invalid(String),
    /// Something else went wrong (too many flows, the window was busy...).
    Failed(String),
}

impl Answer {
    /// The word the page switches on.
    pub fn state(&self) -> &'static str {
        match self {
            Answer::Installed(_) => "installed",
            Answer::InSettings(_) => "in_settings",
            Answer::Declined => "declined",
            Answer::Disabled(_) => "disabled",
            Answer::Invalid(_) => "invalid",
            Answer::Failed(_) => "failed",
        }
    }

    /// A sentence for the person.
    pub fn message(&self) -> String {
        match self {
            Answer::Installed(name) => format!("\"{name}\" is in CursorPie now."),
            Answer::InSettings(name) => {
                format!("\"{name}\" was added to the open Settings. Press Save there to keep it.")
            }
            Answer::Declined => "You said no in CursorPie, so nothing was added.".into(),
            Answer::Disabled(why) | Answer::Invalid(why) | Answer::Failed(why) => why.clone(),
        }
    }

    /// Whether the flow is in (or waiting in Settings for Save).
    pub fn is_in(&self) -> bool {
        matches!(self, Answer::Installed(_) | Answer::InSettings(_))
    }
}

/// Whether the website may offer a flow now; the reason when it may not.
pub fn allowed() -> Result<(), String> {
    let c = cfg();
    check(c.browser_bridge, c.flowhub_install)
}

/// The rule of [`allowed`], for the two settings it reads.
fn check(browser_bridge: bool, flowhub_install: bool) -> Result<(), String> {
    if !browser_bridge {
        Err("The browser extension is switched off in CursorPie (Settings > Behavior).".into())
    } else if !flowhub_install {
        Err("Installing from Flow Hub is switched off in CursorPie (Settings > Behavior).".into())
    } else {
        Ok(())
    }
}

/// Reads the text of a flow file the way **Import** does. The error is the answer to give.
pub fn review(text: &str) -> Result<ImportReport, Answer> {
    if text.len() > MAX_FLOW_TEXT {
        return Err(Answer::Invalid(format!(
            "That file is too large to be a flow ({} KB; the limit is {} KB).",
            text.len() / 1024,
            MAX_FLOW_TEXT / 1024
        )));
    }
    parse_flow_report(text).map_err(Answer::Invalid)
}

/// What a flow can do to the computer or the person, in short phrases (empty for a flow that only
/// presses keys, opens things, waits and calculates). The same blocks an import warns about
/// ([`NodeKind::is_sensitive`]), told apart.
pub fn risks(flow: &crate::config::Flow) -> Vec<&'static str> {
    let (mut shell, mut files, mut browser) = (false, false, false);
    for node in &flow.nodes {
        if !node.kind.is_sensitive() {
            continue;
        }
        match &node.kind {
            NodeKind::Command(_) | NodeKind::Run { .. } => shell = true,
            NodeKind::Native {
                op: NativeOp::Browser,
                ..
            } => browser = true,
            _ => files = true,
        }
    }
    let mut out = Vec::new();
    if shell {
        out.push("run shell commands");
    }
    if files {
        out.push("write, copy, move or delete files");
    }
    if browser {
        out.push("send messages or close tabs in your browser");
    }
    out
}

/// Joins phrases the way a sentence does: `a`, `a and b`, `a, b and c`.
fn join_phrases(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The description of the confirmation panel (what [`PanelSpec::from_value`] reads). The title is the
/// name the **file** gives the flow; `author` is what the page says and is only quoted.
pub fn confirm_spec(report: &ImportReport, author: &str) -> serde_json::Value {
    let flow = &report.flow;
    let danger = risks(flow);
    let author = author.trim();
    let listed = if author.is_empty() {
        String::new()
    } else {
        format!(" Listed as made by {author}.")
    };
    let mut widgets = vec![
        json!({
            "type": "text",
            "text": format!(
                "Flow Hub, on the website, wants to add this flow to CursorPie as a new flow. \
                 Nothing of yours is changed, and it only runs when you give it to a slice.{listed}"
            ),
        }),
        json!({
            "type": "text",
            "style": "muted",
            "text": format!("{} blocks.", flow.nodes.len()),
        }),
    ];
    if !danger.is_empty() {
        widgets.push(json!({
            "type": "text",
            "style": "heading",
            "text": format!("Heads up: it can {}.", join_phrases(&danger)),
        }));
        widgets.push(json!({
            "type": "text",
            "style": "muted",
            "text": "Open it in Settings > Flow and read its blocks before you give it to a slice.",
        }));
    }
    for note in report.notes.iter().take(3) {
        widgets.push(json!({ "type": "text", "style": "muted", "text": note }));
    }
    json!({
        "id": PANEL_ID,
        "title": format!("Install \"{}\"?", flow.name),
        "subtitle": "Flow Hub",
        "icon": "flow",
        "place": "center",
        "width": 440,
        "timeout_s": 120,
        "widgets": widgets,
        "buttons": [
            { "id": "cancel", "label": "Cancel", "cancel": true },
            {
                "id": INSTALL_BUTTON,
                "label": if danger.is_empty() { "Install" } else { "Install anyway" },
                "tone": if danger.is_empty() { "primary" } else { "danger" },
            },
        ],
    })
}

/// Whether the answer of the confirmation panel is a yes.
pub fn confirmed(result: &PanelResult) -> bool {
    result.outcome == Outcome::Submitted && result.button == INSTALL_BUTTON
}

/// The whole offer, from the text of the file to the answer for the browser. **Blocks** (the panel
/// waits for the person): call it from a thread of its own, never from the one that reads the
/// browser.
pub fn offer(author: &str, text: &str) -> Answer {
    if let Err(why) = allowed() {
        return Answer::Disabled(why);
    }
    let report = match review(text) {
        Ok(r) => r,
        Err(answer) => return answer,
    };
    let spec = match PanelSpec::from_value(confirm_spec(&report, author)) {
        Ok(spec) => spec,
        Err(e) => return Answer::Failed(format!("CursorPie could not ask you: {e}.")),
    };
    let result = crate::panel::ask(spec, Source::Hub);
    match result.outcome {
        Outcome::Unavailable => {
            return Answer::Failed(format!(
                "CursorPie could not show its question ({}). If Settings is open, close it and try \
                 again.",
                if result.reason.is_empty() {
                    "no reason given"
                } else {
                    result.reason.trim()
                }
            ));
        }
        _ if !confirmed(&result) => return Answer::Declined,
        _ => {}
    }
    let Some(notifier) = NOTIFIER.get() else {
        return Answer::Failed("CursorPie is not ready yet.".into());
    };
    let (reply, answer) = mpsc::channel();
    notifier.send(Msg::FlowHubInstall {
        flow: Box::new(report.flow),
        reply,
    });
    answer
        .recv_timeout(UI_WAIT)
        .unwrap_or_else(|_| Answer::Failed("CursorPie did not answer in time.".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::flow::parse_flow;

    const SAFE: &str = include_str!("../docs/examples/flows/copy-switch-paste.cursorpie-flow.json");
    const SHELL: &str =
        include_str!("../docs/examples/flows/link-wait-command.cursorpie-flow.json");
    const BROWSER: &str =
        include_str!("../docs/examples/flows/telegram-saved-reply.cursorpie-flow.json");

    fn report(text: &str) -> ImportReport {
        review(text).expect("an example flow loads")
    }

    #[test]
    fn answers_name_their_state_and_say_it_in_words() {
        let installed = Answer::Installed("Copy".into());
        assert_eq!(installed.state(), "installed");
        assert!(installed.message().contains("\"Copy\""));
        assert!(installed.is_in());
        let waiting = Answer::InSettings("Copy".into());
        assert_eq!(waiting.state(), "in_settings");
        assert!(waiting.message().contains("Save"));
        assert!(waiting.is_in());
        for (a, state) in [
            (Answer::Declined, "declined"),
            (Answer::Disabled("off".into()), "disabled"),
            (Answer::Invalid("bad".into()), "invalid"),
            (Answer::Failed("no".into()), "failed"),
        ] {
            assert_eq!(a.state(), state);
            assert!(!a.is_in());
            assert!(!a.message().is_empty());
        }
    }

    #[test]
    fn a_page_cannot_get_around_the_checks_of_an_import() {
        assert!(matches!(review("not json"), Err(Answer::Invalid(_))));
        assert!(matches!(review("{}"), Err(Answer::Invalid(_))));
        // a profile is not a flow
        assert!(matches!(
            review(r#"{"cursorpie_profile": 1}"#),
            Err(Answer::Invalid(_))
        ));
        // a flow from a newer CursorPie
        assert!(matches!(
            review(r#"{"cursorpie_flow": 99, "flow": {}}"#),
            Err(Answer::Invalid(_))
        ));
        let huge = " ".repeat(MAX_FLOW_TEXT + 1);
        let Err(Answer::Invalid(why)) = review(&huge) else {
            panic!("too large")
        };
        assert!(why.contains("too large"));
        assert!(review(SAFE).is_ok());
    }

    #[test]
    fn a_flow_that_only_presses_keys_has_no_risk_and_the_others_say_what_they_can_do() {
        assert!(risks(&parse_flow(SAFE).unwrap()).is_empty());
        assert_eq!(
            risks(&parse_flow(SHELL).unwrap()),
            ["run shell commands"],
            "a command block"
        );
        assert_eq!(
            risks(&parse_flow(BROWSER).unwrap()),
            ["send messages or close tabs in your browser"]
        );
    }

    #[test]
    fn the_new_flow_sheet_says_whether_install_will_work() {
        let chrome = vec!["Chrome".to_string()];
        let (ok, text) = link_line(true, &chrome, true);
        assert!(ok);
        assert!(text.contains("Chrome") && text.contains("one tap"));
        let (ok, text) = link_line(true, &chrome, false);
        assert!(!ok);
        assert!(text.contains("off"));
        let (ok, text) = link_line(true, &[], true);
        assert!(!ok);
        assert!(text.contains("not connected") && text.contains("download"));
        let (ok, text) = link_line(false, &[], true);
        assert!(!ok);
        assert!(text.contains("link is off"));
        assert!(!link_line(false, &chrome, true).1.is_empty());
    }

    #[test]
    fn phrases_are_joined_like_a_sentence() {
        assert_eq!(join_phrases(&[]), "");
        assert_eq!(join_phrases(&["a"]), "a");
        assert_eq!(join_phrases(&["a", "b"]), "a and b");
        assert_eq!(join_phrases(&["a", "b", "c"]), "a, b and c");
    }

    #[test]
    fn the_question_is_a_valid_panel_for_every_kind_of_flow() {
        for (text, danger) in [(SAFE, false), (SHELL, true), (BROWSER, true)] {
            let r = report(text);
            let spec = PanelSpec::from_value(confirm_spec(&r, "sebastvnd"))
                .expect("the description passes the checks of every panel");
            assert_eq!(spec.id, PANEL_ID);
            assert!(spec.title.starts_with("Install \""));
            assert!(
                spec.title
                    .contains(&r.flow.name[..r.flow.name.len().min(20)])
            );
            let ids: Vec<_> = spec.buttons.iter().map(|b| b.id.as_str()).collect();
            assert_eq!(ids, ["cancel", INSTALL_BUTTON]);
            assert_eq!(
                spec.buttons[1].label == "Install anyway",
                danger,
                "a flow with risks asks more clearly"
            );
        }
    }

    #[test]
    fn the_author_the_page_gives_is_only_quoted_and_may_be_missing() {
        let r = report(SAFE);
        let with = confirm_spec(&r, "  someone  ").to_string();
        assert!(with.contains("Listed as made by someone."));
        let without = confirm_spec(&r, "   ").to_string();
        assert!(!without.contains("Listed as made by"));
    }

    #[test]
    fn a_long_or_odd_name_still_gives_a_valid_question() {
        let mut r = report(SAFE);
        r.flow.name = "x\n".repeat(200);
        r.notes = vec!["note ".repeat(100); 5];
        assert!(PanelSpec::from_value(confirm_spec(&r, &"a".repeat(500))).is_ok());
    }

    #[test]
    fn only_a_press_of_install_is_a_yes() {
        let yes = PanelResult {
            panel: PANEL_ID.into(),
            outcome: Outcome::Submitted,
            button: INSTALL_BUTTON.into(),
            values: Default::default(),
            reason: String::new(),
        };
        assert!(confirmed(&yes));
        let mut other = yes.clone();
        other.button = "cancel".into();
        assert!(!confirmed(&other));
        assert!(!confirmed(&PanelResult::cancelled(PANEL_ID, "")));
        assert!(!confirmed(&PanelResult::timeout(PANEL_ID)));
        assert!(!confirmed(&PanelResult::unavailable(PANEL_ID, "off")));
    }

    #[test]
    fn nothing_is_asked_unless_the_link_and_the_setting_are_on() {
        assert!(check(true, true).is_ok());
        let Err(link) = check(false, true) else {
            panic!("the link is off")
        };
        assert!(link.contains("browser extension"));
        let Err(setting) = check(true, false) else {
            panic!("the setting is off")
        };
        assert!(setting.contains("Flow Hub"));
        assert!(check(false, false).is_err());
    }
}
