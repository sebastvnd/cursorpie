//! The commands a slice can send to the browser ("Browser" action): what they are called, what they
//! take, and how a line like `chat.send_to Anna :: hi` is read.
//!
//! The extension does the work (see `assets/browser-extension/cmd-*.js` and
//! `docs/BROWSER_EXTENSION.md`); this list is what the app offers in the editor and checks before
//! anything is sent. A test reads the extension's source and fails when the two lists differ.

use std::fmt;

/// The longest argument sent to the browser (a message to type, a chat name).
pub const MAX_ARG_CHARS: usize = 2_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Media,
    Tab,
    Window,
    Page,
    Chat,
    /// Commands that ask in a panel of CursorPie ([`crate::panel`]) and then act.
    Ui,
}

impl Group {
    /// How the group is written in the editor.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Media => "Media",
            Self::Tab => "Tabs",
            Self::Window => "Window",
            Self::Page => "Page",
            Self::Chat => "Chats",
            Self::Ui => "Panels",
        }
    }
}

/// What a command does with the text after its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arg {
    /// Nothing.
    None,
    /// It may be given; the text says what it is.
    Optional(&'static str),
    /// It must be given.
    Required(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Command {
    /// What is sent to the extension: `group.verb`.
    pub id: &'static str,
    pub group: Group,
    /// How the editor names it.
    pub label: &'static str,
    pub arg: Arg,
}

const fn c(id: &'static str, group: Group, label: &'static str, arg: Arg) -> Command {
    Command {
        id,
        group,
        label,
        arg,
    }
}

use Arg::{None as No, Optional as Opt, Required as Req};
use Group::{Chat, Media, Page, Tab, Ui, Window};

/// Every command, in the order of the editor's list.
pub const CATALOG: &[Command] = &[
    c("media.toggle", Media, "Play / pause", No),
    c("media.play", Media, "Play", No),
    c("media.pause", Media, "Pause", No),
    c("media.next", Media, "Next track", No),
    c("media.prev", Media, "Previous track", No),
    c("media.restart", Media, "Restart the track", No),
    c("media.seek_fwd", Media, "Skip forward", Opt("seconds")),
    c("media.seek_back", Media, "Skip back", Opt("seconds")),
    c("media.volume_up", Media, "Volume up", Opt("percent")),
    c("media.volume_down", Media, "Volume down", Opt("percent")),
    c("media.mute", Media, "Mute / unmute", No),
    c("media.speed_up", Media, "Faster", Opt("step, like 0.25")),
    c("media.speed_down", Media, "Slower", Opt("step, like 0.25")),
    c("media.speed_reset", Media, "Normal speed", No),
    c("media.like", Media, "Like", No),
    c("media.captions", Media, "Captions on / off", No),
    c("media.skip_ad", Media, "Skip ad or intro", No),
    c("media.shuffle", Media, "Shuffle", No),
    c("media.repeat", Media, "Repeat", No),
    c("media.now_playing", Media, "Show what is playing", No),
    c(
        "media.state",
        Media,
        "Say if the player plays or is paused (for a flow)",
        No,
    ),
    c("media.pause_all", Media, "Pause everything", No),
    c("media.focus", Media, "Go to the player's tab", No),
    c("tab.next", Tab, "Next tab", No),
    c("tab.prev", Tab, "Previous tab", No),
    c("tab.recent", Tab, "Back to the last tab used", No),
    c("tab.first", Tab, "First tab", No),
    c("tab.end", Tab, "Last tab", No),
    c("tab.goto_audible", Tab, "Go to the tab with sound", No),
    c(
        "tab.find",
        Tab,
        "Go to a tab with this text",
        Req("text in the title or address"),
    ),
    c("tab.move_left", Tab, "Move the tab left", No),
    c("tab.move_right", Tab, "Move the tab right", No),
    c("tab.close", Tab, "Close the tab", No),
    c("tab.reopen", Tab, "Reopen the closed tab", No),
    c("tab.new", Tab, "New tab", Opt("https:// address")),
    c("tab.duplicate", Tab, "Duplicate the tab", No),
    c("tab.to_window", Tab, "Move the tab to its own window", No),
    c("tab.close_others", Tab, "Close the other tabs", No),
    c("tab.close_right", Tab, "Close the tabs to the right", No),
    c("tab.close_dupes", Tab, "Close duplicate tabs", No),
    c(
        "tab.discard_others",
        Tab,
        "Unload the other tabs (free memory)",
        No,
    ),
    c("tab.pin", Tab, "Pin / unpin the tab", No),
    c("tab.mute", Tab, "Mute / unmute the tab", No),
    c("tab.reload", Tab, "Reload", No),
    c("tab.hard_reload", Tab, "Reload without the cache", No),
    c("tab.back", Tab, "Go back", No),
    c("tab.forward", Tab, "Go forward", No),
    c("tab.zoom_in", Tab, "Zoom in", No),
    c("tab.zoom_out", Tab, "Zoom out", No),
    c("tab.zoom_reset", Tab, "Zoom 100%", No),
    c("tab.url", Tab, "Tell the tab's address (for a flow)", No),
    c(
        "tab.count",
        Tab,
        "Tell how many tabs the window has (for a flow)",
        No,
    ),
    c("window.new", Window, "New window", No),
    c("window.private", Window, "New private window", No),
    c("window.minimize", Window, "Minimize the window", No),
    c(
        "window.maximize",
        Window,
        "Maximize / restore the window",
        No,
    ),
    c("window.fullscreen", Window, "Full screen on / off", No),
    c("window.close", Window, "Close the window", No),
    c("page.top", Page, "Scroll to the top", No),
    c("page.bottom", Page, "Scroll to the bottom", No),
    c("page.up", Page, "Scroll up a page", No),
    c("page.down", Page, "Scroll down a page", No),
    c(
        "chat.focus",
        Chat,
        "Go to WhatsApp / Telegram",
        Opt("@telegram"),
    ),
    c("chat.next_unread", Chat, "Next unread chat", No),
    c("chat.prev_unread", Chat, "Previous unread chat", No),
    c("chat.next", Chat, "Next chat", No),
    c("chat.prev", Chat, "Previous chat", No),
    c("chat.close", Chat, "Close the chat", No),
    c("chat.bottom", Chat, "Jump to the latest message", No),
    c("chat.status", Chat, "Show how many chats wait", No),
    c("chat.search", Chat, "Search the chats", Req("text")),
    c("chat.open", Chat, "Open a chat", Req("name")),
    c(
        "chat.draft",
        Chat,
        "Type a message (not sent)",
        Req("message"),
    ),
    c(
        "chat.draft_to",
        Chat,
        "Type a message in a chat (not sent)",
        Req("Name :: message"),
    ),
    c(
        "chat.send",
        Chat,
        "Send a message in the open chat",
        Req("message"),
    ),
    c(
        "chat.send_to",
        Chat,
        "Send a message to a chat",
        Req("Name :: message"),
    ),
    c(
        "chat.unread",
        Chat,
        "Tell how many chats wait (for a flow)",
        Opt("@telegram"),
    ),
    c("chat.who", Chat, "Tell which chat is open (for a flow)", No),
    c(
        "chat.unread_names",
        Chat,
        "Tell the names of the chats that wait (for a flow)",
        No,
    ),
    c(
        "chat.last",
        Chat,
        "Tell the last message that came (for a flow)",
        No,
    ),
    c(
        "chat.read",
        Chat,
        "Tell the last messages of the chat (for a flow)",
        Opt("how many, up to 20"),
    ),
    c(
        "chat.wait",
        Chat,
        "Wait for a message and tell it (for a flow)",
        Opt("seconds, up to 300"),
    ),
    c(
        "chat.folder",
        Chat,
        "Go to a Telegram folder",
        Req("folder name or number"),
    ),
    c(
        "chat.folders",
        Chat,
        "Tell the names of the Telegram folders (for a flow)",
        No,
    ),
    c(
        "chat.next_mention",
        Chat,
        "Next Telegram chat with a mention",
        No,
    ),
    c(
        "chat.template",
        Chat,
        "Send a saved reply",
        Req("reply, or reply :: Chat"),
    ),
    c(
        "chat.templates",
        Chat,
        "Tell the names of the saved replies (for a flow)",
        No,
    ),
    c("ui.tabs", Ui, "Pick a tab from a list", No),
    c(
        "ui.find_tab",
        Ui,
        "Ask for a word, then go to that tab",
        Opt("heading of the box"),
    ),
    c(
        "ui.confirm_close_others",
        Ui,
        "Ask, then close the other tabs",
        No,
    ),
    c("ui.media", Ui, "Media menu (play, next, volume...)", No),
    c(
        "ui.video",
        Ui,
        "Video remote (stays open: skip, next, pause, volume...)",
        No,
    ),
];

/// The command named `id`.
pub fn find(id: &str) -> Option<&'static Command> {
    CATALOG.iter().find(|c| c.id == id)
}

/// Why a line is not a command.
#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    Empty,
    Unknown(String),
    NeedsArgument(&'static str, &'static str),
    TooLong,
    BadCharacter,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "The Browser action has no command"),
            Self::Unknown(c) => write!(f, "'{c}' is not a browser command"),
            Self::NeedsArgument(id, what) => write!(f, "{id} needs {what} after its name"),
            Self::TooLong => write!(f, "The text after the command is too long"),
            Self::BadCharacter => write!(f, "The text after the command has a control character"),
        }
    }
}

/// Reads `name` or `name text...`: the command and its argument (trimmed; empty when none).
pub fn parse(spec: &str) -> Result<(&'static Command, String), ParseError> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err(ParseError::Empty);
    }
    let (name, rest) = spec
        .split_once(char::is_whitespace)
        .map_or((spec, ""), |(n, r)| (n, r.trim()));
    let cmd = find(name).ok_or_else(|| ParseError::Unknown(name.to_string()))?;
    if rest.chars().count() > MAX_ARG_CHARS {
        return Err(ParseError::TooLong);
    }
    // a line break is allowed (a message of two lines); other control characters are not
    if rest.chars().any(|ch| ch.is_control() && ch != '\n') {
        return Err(ParseError::BadCharacter);
    }
    if let (Arg::Required(what), true) = (cmd.arg, rest.is_empty()) {
        return Err(ParseError::NeedsArgument(cmd.id, what));
    }
    // "Name :: message": both halves, or the message would go nowhere or to the wrong chat
    if matches!(cmd.id, "chat.send_to" | "chat.draft_to")
        && !rest
            .split_once("::")
            .is_some_and(|(n, m)| !n.trim().is_empty() && !m.trim().is_empty())
    {
        return Err(ParseError::NeedsArgument(cmd.id, "Name :: message"));
    }
    Ok((cmd, rest.to_string()))
}

/// The commands that hand the text of a chat, the name of a chat or the name of a Telegram folder
/// to the app. The extension refuses them until the person turns on *Allow reading messages* in its
/// options.
pub const READS_CHATS: [&str; 6] = [
    "chat.who",
    "chat.unread_names",
    "chat.last",
    "chat.read",
    "chat.wait",
    "chat.folders",
];

/// Whether a command line can do something that cannot be taken back, that speaks for the person,
/// or that reads their chats: it sends a message (`chat.template` too: a saved reply is sent like
/// any other), closes a window, or reads what is in a chat. A
/// profile that has one is flagged when it is imported, like one that runs a shell command (a flow
/// that reads a chat and then opens a link with what it read could give it away).
pub fn is_sensitive(spec: &str) -> bool {
    let name = spec.split_whitespace().next().unwrap_or("");
    matches!(
        name,
        "chat.send" | "chat.send_to" | "chat.template" | "window.close"
    ) || READS_CHATS.contains(&name)
}

/// [`is_sensitive`] for a line written in a flow, where the command itself may be a
/// `{{variable}}` that nobody can read before the flow runs: such a line counts as sensitive,
/// because it can become any command, `chat.send_to` included.
pub fn is_sensitive_in_flow(spec: &str) -> bool {
    is_sensitive(spec) || spec.trim_start().starts_with("{{")
}

/// How long a flow waits for the browser to say how a command of this group went, in seconds. A
/// panel waits for a person, so it gets as long as an extension's panel may stay open (300 s) and
/// a little more; the others only talk to the browser and answer at once, or not at all.
pub const fn wait_secs(group: Group) -> u64 {
    match group {
        Group::Ui => 330,
        _ => 20,
    }
}

/// Seconds a flow waits beyond what `chat.wait` is told to wait, for the browser to answer.
const CHAT_WAIT_MARGIN_S: u64 = 15;

/// How long `chat.wait` is told to wait, in seconds: the number after it, from 1 to 300, and 60
/// without one (or with text that is not a number).
pub fn chat_wait_secs(arg: &str) -> u64 {
    arg.trim()
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .map_or(60, |n| n.round().clamp(1.0, 300.0) as u64)
}

/// How long a flow waits for the browser to say how `cmd` went: [`wait_secs`] for its group, except
/// for `chat.wait`, which is waited for as long as it was told to wait, and a little more.
pub fn wait_secs_for(cmd: &Command, arg: &str) -> u64 {
    if cmd.id == "chat.wait" {
        chat_wait_secs(arg) + CHAT_WAIT_MARGIN_S
    } else {
        wait_secs(cmd.group)
    }
}

/// Whether a flow that sends this command waits for a person or for a message from somebody else (a
/// panel, `chat.wait`): that time does not count against the
/// [`MAX_FLOW_RUN_SECS`](crate::config::MAX_FLOW_RUN_SECS) seconds a flow may run.
pub fn waits_for_others(spec: &str) -> bool {
    let name = spec.split_whitespace().next().unwrap_or("");
    name.starts_with("ui.") || name == "chat.wait"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    // what the extension registers, read from its source: `reg("media.next", ...)`
    fn registered() -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for src in [
            include_str!("../../assets/browser-extension/cmd-tab.js"),
            include_str!("../../assets/browser-extension/cmd-media.js"),
            include_str!("../../assets/browser-extension/cmd-chat.js"),
            include_str!("../../assets/browser-extension/cmd-ui.js"),
        ] {
            for line in src.lines() {
                let t = line.trim_start();
                if let Some(rest) = t.strip_prefix("reg(\"")
                    && let Some(end) = rest.find('"')
                {
                    out.insert(rest[..end].to_string());
                }
            }
        }
        out
    }

    #[test]
    fn the_app_and_the_extension_know_the_same_commands() {
        let theirs = registered();
        let ours: BTreeSet<String> = CATALOG.iter().map(|c| c.id.to_string()).collect();
        let only_app: Vec<_> = ours.difference(&theirs).collect();
        let only_ext: Vec<_> = theirs.difference(&ours).collect();
        assert!(
            only_app.is_empty() && only_ext.is_empty(),
            "only in the app: {only_app:?}; only in the extension: {only_ext:?}"
        );
        assert_eq!(ours.len(), CATALOG.len(), "a command is listed twice");
    }

    #[test]
    fn every_command_is_documented() {
        let doc = include_str!("../../docs/BROWSER_EXTENSION.md");
        for c in CATALOG {
            assert!(
                doc.contains(&format!("`{}`", c.id)),
                "{} is not in docs/BROWSER_EXTENSION.md",
                c.id
            );
        }
    }

    #[test]
    fn ids_are_group_dot_verb_and_the_group_matches() {
        for c in CATALOG {
            let (g, verb) = c.id.split_once('.').expect("group.verb");
            assert!(!verb.is_empty() && !verb.contains('.'));
            let want = match c.group {
                Group::Media => "media",
                Group::Tab => "tab",
                Group::Window => "window",
                Group::Page => "page",
                Group::Chat => "chat",
                Group::Ui => "ui",
            };
            assert_eq!(g, want, "{}", c.id);
        }
    }

    #[test]
    fn a_line_is_a_name_and_the_text_after_it() {
        let (c, a) = parse("  media.seek_fwd   30 ").unwrap();
        assert_eq!((c.id, a.as_str()), ("media.seek_fwd", "30"));
        let (c, a) = parse("chat.send_to Anna :: see you at 5").unwrap();
        assert_eq!((c.id, a.as_str()), ("chat.send_to", "Anna :: see you at 5"));
        let (_, a) = parse("media.next").unwrap();
        assert_eq!(a, "");
        // a tab character separates too
        assert_eq!(parse("tab.find\tinbox").unwrap().1, "inbox");
    }

    #[test]
    fn a_line_that_is_not_a_command_is_refused() {
        assert_eq!(parse("  "), Err(ParseError::Empty));
        assert_eq!(parse("rm -rf /"), Err(ParseError::Unknown("rm".into())));
        assert_eq!(
            parse("MEDIA.NEXT"),
            Err(ParseError::Unknown("MEDIA.NEXT".into()))
        );
        assert!(matches!(
            parse("chat.open"),
            Err(ParseError::NeedsArgument("chat.open", _))
        ));
        assert!(matches!(
            parse("chat.send_to   "),
            Err(ParseError::NeedsArgument(..))
        ));
        assert!(matches!(
            parse("chat.send_to Anna"),
            Err(ParseError::NeedsArgument(..))
        ));
        assert!(matches!(
            parse("chat.send_to Anna ::"),
            Err(ParseError::NeedsArgument(..))
        ));
        assert!(matches!(
            parse("chat.draft_to :: hi"),
            Err(ParseError::NeedsArgument(..))
        ));
        assert!(parse("chat.send_to Anna :: hi").is_ok());
        assert_eq!(
            parse(&format!("chat.draft {}", "x".repeat(MAX_ARG_CHARS + 1))),
            Err(ParseError::TooLong)
        );
        assert!(parse(&format!("chat.draft {}", "x".repeat(MAX_ARG_CHARS))).is_ok());
        assert_eq!(parse("chat.draft a\u{7}b"), Err(ParseError::BadCharacter));
        assert_eq!(parse("chat.draft a\u{0}b"), Err(ParseError::BadCharacter));
        assert!(parse("chat.draft two\nlines").is_ok());
    }

    #[test]
    fn a_flow_waits_longer_for_a_panel_than_for_a_command_that_answers_at_once() {
        assert_eq!(wait_secs(Group::Media), 20);
        assert_eq!(wait_secs(Group::Tab), 20);
        assert_eq!(wait_secs(Group::Chat), 20);
        assert!(wait_secs(Group::Ui) > 300, "a panel may stay open 300 s");
    }

    #[test]
    fn a_command_that_is_a_variable_counts_as_sensitive_in_a_flow() {
        assert!(is_sensitive_in_flow("chat.send_to Mom :: hi"));
        assert!(is_sensitive_in_flow("  {{cmd}}"));
        assert!(is_sensitive_in_flow("{{cmd}} 30"));
        assert!(!is_sensitive_in_flow("media.seek_fwd {{n}}"));
        assert!(!is_sensitive_in_flow("tab.url"));
        assert!(!is_sensitive_in_flow(""));
    }

    #[test]
    fn the_questions_for_flows_take_no_text() {
        for id in ["tab.url", "tab.count", "media.state"] {
            assert_eq!(find(id).map(|c| c.arg), Some(Arg::None), "{id}");
        }
    }

    #[test]
    fn what_can_speak_for_you_is_flagged() {
        assert!(is_sensitive("chat.send_to Mom :: hi"));
        assert!(is_sensitive("  chat.send ok"));
        assert!(is_sensitive("window.close"));
        assert!(!is_sensitive("chat.draft_to Mom :: hi"));
        assert!(!is_sensitive("media.next"));
        assert!(!is_sensitive(""));
    }

    #[test]
    fn what_reads_a_chat_is_flagged_and_what_only_counts_is_not() {
        for id in READS_CHATS {
            assert!(is_sensitive(id), "{id}");
            assert!(is_sensitive(&format!("  {id} 30")), "{id} with an argument");
            assert!(is_sensitive_in_flow(id), "{id} in a flow");
        }
        // the number of unread chats is not what anybody wrote
        assert!(!is_sensitive("chat.unread"));
        assert!(!is_sensitive("chat.unread @telegram"));
        // a name that only starts like one of them is another command
        assert!(!is_sensitive("chat.waiting"));
    }

    #[test]
    fn the_commands_that_read_a_chat_exist_and_are_chat_commands() {
        let mut seen = BTreeSet::new();
        for id in READS_CHATS {
            let c = find(id).unwrap_or_else(|| panic!("{id} is not in the catalog"));
            assert_eq!(c.group, Group::Chat, "{id}");
            assert!(seen.insert(id), "{id} is listed twice");
        }
    }

    #[test]
    fn the_new_chat_commands_take_what_the_extension_reads() {
        // the questions for a flow
        for id in ["chat.unread", "chat.who", "chat.unread_names", "chat.last"] {
            let c = find(id).unwrap_or_else(|| panic!("{id} is not in the catalog"));
            assert!(!matches!(c.arg, Arg::Required(_)), "{id} needs no text");
        }
        assert_eq!(find("chat.who").map(|c| c.arg), Some(Arg::None));
        assert!(matches!(
            find("chat.read").map(|c| c.arg),
            Some(Arg::Optional(_))
        ));
        assert!(matches!(
            find("chat.wait").map(|c| c.arg),
            Some(Arg::Optional(_))
        ));
        // a line with or without the number of seconds is a command
        assert_eq!(parse("chat.wait").unwrap().1, "");
        assert_eq!(parse("chat.wait  45 ").unwrap().1, "45");
        assert_eq!(parse("chat.read 3").unwrap().1, "3");
        assert_eq!(parse("chat.unread @telegram").unwrap().1, "@telegram");
    }

    #[test]
    fn the_telegram_commands_are_listed_and_only_what_speaks_or_reads_is_flagged() {
        for id in [
            "chat.folder",
            "chat.folders",
            "chat.next_mention",
            "chat.template",
            "chat.templates",
        ] {
            let c = find(id).unwrap_or_else(|| panic!("{id} is not in the catalog"));
            assert_eq!(c.group, Group::Chat, "{id}");
        }
        assert!(matches!(
            find("chat.folder").map(|c| c.arg),
            Some(Arg::Required(_))
        ));
        assert!(matches!(
            find("chat.template").map(|c| c.arg),
            Some(Arg::Required(_))
        ));
        assert_eq!(find("chat.folders").map(|c| c.arg), Some(Arg::None));
        assert_eq!(find("chat.next_mention").map(|c| c.arg), Some(Arg::None));
        assert_eq!(find("chat.templates").map(|c| c.arg), Some(Arg::None));
        // a saved reply is sent like chat.send, so it is flagged like it
        assert!(is_sensitive("chat.template omw"));
        assert!(is_sensitive("chat.template omw :: Anna"));
        assert!(is_sensitive_in_flow("chat.template {{key}}"));
        // the names of the folders are read like the names of chats
        assert!(READS_CHATS.contains(&"chat.folders"));
        assert!(is_sensitive("chat.folders"));
        // changing folder, looking for a mention and listing the names of the replies say nothing
        assert!(!is_sensitive("chat.folder Work"));
        assert!(!is_sensitive("chat.next_mention"));
        assert!(!is_sensitive("chat.templates"));
        // the lines are read as they are written
        assert_eq!(parse("chat.template omw :: Anna").unwrap().1, "omw :: Anna");
        assert_eq!(
            parse("chat.folder @telegram Work").unwrap().1,
            "@telegram Work"
        );
        assert!(matches!(
            parse("chat.folder"),
            Err(ParseError::NeedsArgument("chat.folder", _))
        ));
        assert!(matches!(
            parse("chat.template  "),
            Err(ParseError::NeedsArgument("chat.template", _))
        ));
        // none of them waits for anybody
        assert!(!waits_for_others("chat.folders"));
        assert!(!waits_for_others("chat.template omw"));
    }

    #[test]
    fn chat_wait_is_told_how_long_to_wait_from_one_second_to_five_minutes() {
        assert_eq!(chat_wait_secs(""), 60, "a minute without a number");
        assert_eq!(chat_wait_secs("   "), 60);
        assert_eq!(chat_wait_secs("45"), 45);
        assert_eq!(chat_wait_secs(" 45 "), 45);
        assert_eq!(chat_wait_secs("1"), 1);
        assert_eq!(chat_wait_secs("0"), 1, "never less than a second");
        assert_eq!(chat_wait_secs("-30"), 1);
        assert_eq!(chat_wait_secs("300"), 300);
        assert_eq!(chat_wait_secs("301"), 300, "never more than five minutes");
        assert_eq!(chat_wait_secs("9999999999999999999999"), 300);
        // a decimal, with a point or a comma, is rounded
        assert_eq!(chat_wait_secs("2.6"), 3);
        assert_eq!(chat_wait_secs("2,4"), 2);
        // text that is not a number is the default, not an error
        assert_eq!(chat_wait_secs("soon"), 60);
        assert_eq!(chat_wait_secs("30 s"), 60);
        assert_eq!(chat_wait_secs("NaN"), 60);
        assert_eq!(chat_wait_secs("inf"), 60);
    }

    #[test]
    fn a_flow_waits_for_chat_wait_as_long_as_it_was_told_and_a_little_more() {
        let wait = find("chat.wait").unwrap();
        assert_eq!(wait_secs_for(wait, "30"), 30 + CHAT_WAIT_MARGIN_S);
        assert_eq!(wait_secs_for(wait, ""), 60 + CHAT_WAIT_MARGIN_S);
        assert_eq!(wait_secs_for(wait, "9999"), 300 + CHAT_WAIT_MARGIN_S);
        assert!(
            wait_secs_for(wait, "9999") <= wait_secs(Group::Ui),
            "the longest wait is not longer than a panel's"
        );
        // every other command keeps the time of its group
        for id in [
            "chat.last",
            "chat.read",
            "chat.send_to",
            "media.next",
            "tab.url",
        ] {
            let c = find(id).unwrap();
            assert_eq!(wait_secs_for(c, "30"), wait_secs(c.group), "{id}");
        }
        let panel = find("ui.tabs").unwrap();
        assert_eq!(wait_secs_for(panel, ""), wait_secs(Group::Ui));
    }

    #[test]
    fn a_wait_for_a_person_or_a_message_does_not_count_against_the_run() {
        assert!(waits_for_others("chat.wait"));
        assert!(waits_for_others("chat.wait 120"));
        assert!(waits_for_others("  chat.wait   5 "));
        for panel in CATALOG.iter().filter(|c| c.group == Group::Ui) {
            assert!(waits_for_others(panel.id), "{}", panel.id);
        }
        assert!(!waits_for_others("chat.last"));
        assert!(!waits_for_others("chat.send_to Anna :: hi"));
        assert!(!waits_for_others("chat.waiting"));
        assert!(!waits_for_others("media.next"));
        assert!(!waits_for_others(""));
    }
}
