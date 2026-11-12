//! The apps every desktop has, by role: the default web browser, file manager and terminal. A slice
//! names one as `@browser`, `@files` or `@terminal`, and CursorPie asks the system which app that is
//! each time it runs. That app is then opened like any other "Open app" slice, so a window that
//! already exists is raised instead of a second copy being started. The shell commands in this
//! module are what runs when the system cannot say which app it is.

use super::desktop_entry::{Index, id_key};
use crate::config::Action;

/// Starts the default web browser (a new window).
pub const BROWSER_COMMAND: &str = "b=$(xdg-settings get default-web-browser); \
gtk-launch \"$b\" 2>/dev/null || gio launch \"/usr/share/applications/$b\" 2>/dev/null || xdg-open about:blank";

/// Opens the home folder in the default file manager.
pub const FILES_COMMAND: &str = "xdg-open \"$HOME\"";

/// Starts the user's terminal: the desktop's default, else the first of a list that is installed.
pub const TERMINAL_COMMAND: &str = "if command -v xdg-terminal-exec >/dev/null; then exec xdg-terminal-exec; \
elif command -v x-terminal-emulator >/dev/null; then exec x-terminal-emulator; \
else for t in kgx gnome-terminal konsole xfce4-terminal alacritty kitty foot xterm; do \
command -v \"$t\" >/dev/null && exec \"$t\"; done; fi";

/// Terminals to look for, in order, when the desktop does not name a default one.
const TERMINALS: [&str; 9] = [
    "org.gnome.console",
    "org.gnome.terminal",
    "org.kde.konsole",
    "xfce4-terminal",
    "alacritty",
    "kitty",
    "foot",
    "konsole",
    "xterm",
];

/// An app that is named by what it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Browser,
    Files,
    Terminal,
}

impl Role {
    /// The role a slice's "Open app" text stands for (`@browser`, any case). `None` for an app id.
    pub fn parse(text: &str) -> Option<Role> {
        match text.trim().to_ascii_lowercase().as_str() {
            "@browser" => Some(Role::Browser),
            "@files" => Some(Role::Files),
            "@terminal" => Some(Role::Terminal),
            _ => None,
        }
    }

    /// The text stored in the slice.
    pub fn text(self) -> &'static str {
        match self {
            Role::Browser => "@browser",
            Role::Files => "@files",
            Role::Terminal => "@terminal",
        }
    }

    /// The shell command that starts the app when the system cannot say which one it is.
    pub fn command(self) -> &'static str {
        match self {
            Role::Browser => BROWSER_COMMAND,
            Role::Files => FILES_COMMAND,
            Role::Terminal => TERMINAL_COMMAND,
        }
    }

    /// The program and arguments that print the desktop file of the default app. `None` when the
    /// system has no such setting (terminals).
    pub fn query(self) -> Option<(&'static str, &'static [&'static str])> {
        match self {
            Role::Browser => Some(("xdg-settings", &["get", "default-web-browser"])),
            Role::Files => Some(("xdg-mime", &["query", "default", "inode/directory"])),
            Role::Terminal => None,
        }
    }
}

/// The app id in the output of a [`Role::query`] (`org.mozilla.firefox.desktop` gives
/// `org.mozilla.firefox`). `None` if it printed nothing.
pub fn app_from_query(output: &str) -> Option<String> {
    let id = id_key(output.lines().next()?);
    if id.is_empty() { None } else { Some(id) }
}

/// Programs of the browsers that take `--new-window <link>` (the Firefox and Chromium families).
const NEW_WINDOW_BROWSERS: [&str; 24] = [
    "firefox",
    "firefox-esr",
    "firefox-bin",
    "librewolf",
    "waterfox",
    "floorp",
    "zen",
    "zen-browser",
    "google-chrome",
    "google-chrome-stable",
    "google-chrome-beta",
    "google-chrome-unstable",
    "chromium",
    "chromium-browser",
    "chrome",
    "brave",
    "brave-browser",
    "brave-browser-stable",
    "vivaldi",
    "vivaldi-stable",
    "microsoft-edge",
    "microsoft-edge-stable",
    "opera",
    "thorium-browser",
];

/// The arguments that make the browser started by `program` (an `Exec` program, see
/// `desktop_entry::exec_program`) open `url` in a **new window**. `None` for a browser that is
/// not known to take `--new-window`, and for a link that could be read as an option: the caller then
/// hands the link to `xdg-open`, which most browsers answer with a new tab.
pub fn new_window_args(program: &str, url: &str) -> Option<Vec<String>> {
    if url.starts_with('-') || !NEW_WINDOW_BROWSERS.contains(&program) {
        return None;
    }
    Some(vec!["--new-window".to_string(), url.to_string()])
}

/// The first of the well-known terminals that is installed.
pub fn terminal_app(index: &Index) -> Option<String> {
    TERMINALS
        .iter()
        .find_map(|id| index.find(id).map(|e| e.id.clone()))
}

/// Turns one of the launcher commands that earlier default configs held into its role, so an
/// existing `config.json` gets the new behavior too. Any other action is left alone.
pub fn upgrade(action: &mut Action) {
    let role = match action {
        Action::Command(c) if c.as_str() == BROWSER_COMMAND => Role::Browser,
        Action::Command(c) if c.as_str() == FILES_COMMAND => Role::Files,
        Action::Command(c) if c.as_str() == TERMINAL_COMMAND => Role::Terminal,
        _ => return,
    };
    *action = Action::OpenApp(role.text().to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::linux::desktop_entry::Entry;

    fn entry(id: &str) -> Entry {
        Entry {
            id: id.into(),
            name: id.into(),
            wm_class: None,
            exec_name: None,
            path: format!("/usr/share/applications/{id}.desktop"),
            icon: None,
            no_display: false,
            single_window: false,
            search: String::new(),
        }
    }

    #[test]
    fn a_known_browser_gets_a_new_window_and_others_do_not() {
        let url = "https://example.com/a?b=c";
        for program in [
            "firefox",
            "google-chrome-stable",
            "brave-browser",
            "chromium",
        ] {
            assert_eq!(
                new_window_args(program, url),
                Some(vec!["--new-window".to_string(), url.to_string()]),
                "{program}"
            );
        }
        assert_eq!(new_window_args("epiphany", url), None, "not known");
        assert_eq!(new_window_args("", url), None);
        assert_eq!(
            new_window_args("firefox", "--profile=/tmp/x"),
            None,
            "a link is never an option"
        );
    }

    #[test]
    fn roles_are_written_with_an_at_sign() {
        assert_eq!(Role::parse("@browser"), Some(Role::Browser));
        assert_eq!(Role::parse("  @Files "), Some(Role::Files));
        assert_eq!(Role::parse("@TERMINAL"), Some(Role::Terminal));
        assert_eq!(Role::parse("browser"), None);
        assert_eq!(Role::parse("firefox"), None);
        assert_eq!(Role::parse(""), None);
        for role in [Role::Browser, Role::Files, Role::Terminal] {
            assert_eq!(Role::parse(role.text()), Some(role));
        }
    }

    #[test]
    fn the_query_prints_a_desktop_file() {
        assert_eq!(
            app_from_query("org.mozilla.firefox.desktop\n").as_deref(),
            Some("org.mozilla.firefox")
        );
        assert_eq!(
            app_from_query(" Google-Chrome.desktop \nignored").as_deref(),
            Some("google-chrome")
        );
        assert_eq!(app_from_query(""), None);
        assert_eq!(app_from_query("\n"), None);
        assert_eq!(app_from_query("  \n"), None);
        assert!(Role::Browser.query().is_some());
        assert!(Role::Files.query().is_some());
        assert!(Role::Terminal.query().is_none());
    }

    #[test]
    fn the_first_installed_terminal_wins() {
        let idx = Index::new(vec![entry("xterm"), entry("org.kde.konsole")]);
        assert_eq!(terminal_app(&idx).as_deref(), Some("org.kde.konsole"));
        assert_eq!(terminal_app(&Index::default()), None);
    }

    #[test]
    fn old_launcher_commands_become_roles() {
        let mut files = Action::Command(FILES_COMMAND.into());
        upgrade(&mut files);
        assert_eq!(files, Action::OpenApp("@files".into()));
        let mut browser = Action::Command(BROWSER_COMMAND.into());
        upgrade(&mut browser);
        assert_eq!(browser, Action::OpenApp("@browser".into()));
        let mut terminal = Action::Command(TERMINAL_COMMAND.into());
        upgrade(&mut terminal);
        assert_eq!(terminal, Action::OpenApp("@terminal".into()));
    }

    #[test]
    fn other_actions_are_not_touched() {
        let mut mine = Action::Command("xdg-open /tmp".into());
        upgrade(&mut mine);
        assert_eq!(mine, Action::Command("xdg-open /tmp".into()));
        let mut app = Action::OpenApp("firefox".into());
        upgrade(&mut app);
        assert_eq!(app, Action::OpenApp("firefox".into()));
        let mut nothing = Action::Nothing;
        upgrade(&mut nothing);
        assert_eq!(nothing, Action::Nothing);
    }
}
