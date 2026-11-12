//! How to start an app and how to ask the user for a file. Pure decisions; the backend runs them.

use super::desktop_entry::Index;
use crate::platform::{FileKind, IMAGE_EXTENSIONS};

/// How an "Open app" action is started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launch {
    /// Through the desktop entry at this path (`gio launch`), so startup notification and the
    /// entry's own `Exec` line apply.
    Desktop(String),
    /// Run this program (a name on `PATH` or an absolute path) directly.
    Program(String),
}

/// Decides how to start `app`: a path is run directly, a known desktop id goes through its entry,
/// anything else is tried as a program name.
pub fn resolve(app: &str, index: &Index) -> Launch {
    let a = app.trim();
    if a.contains('/') {
        return Launch::Program(a.to_string());
    }
    match index.find(a) {
        Some(e) => Launch::Desktop(e.path.clone()),
        None => Launch::Program(a.to_string()),
    }
}

/// The external dialog program used to choose a file (there is no native API on Linux).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chooser {
    Zenity,
    Kdialog,
}

impl Chooser {
    /// The command name.
    pub fn program(self) -> &'static str {
        match self {
            Chooser::Zenity => "zenity",
            Chooser::Kdialog => "kdialog",
        }
    }
}

/// Command line arguments that make `tool` ask for one file of `kind`. The chosen path is printed
/// on standard output, and the exit status is non-zero if the user cancels.
pub fn chooser_args(tool: Chooser, kind: FileKind, prompt: &str) -> Vec<String> {
    let label: &str = match kind {
        FileKind::Image => "Images",
        FileKind::Json => "JSON files",
        FileKind::App => "Applications",
    };
    let patterns: &[&str] = match kind {
        FileKind::Image => &[],
        FileKind::Json => &["*.json", "*.JSON"],
        FileKind::App => &["*.desktop"],
    };
    // the choosers match case-sensitively, so every image extension is given in both cases
    let patterns = if kind == FileKind::Image {
        let lower = IMAGE_EXTENSIONS.iter().map(|e| format!("*.{e}"));
        let upper = IMAGE_EXTENSIONS
            .iter()
            .map(|e| format!("*.{}", e.to_ascii_uppercase()));
        lower.chain(upper).collect::<Vec<_>>().join(" ")
    } else {
        patterns.join(" ")
    };
    match tool {
        Chooser::Zenity => {
            let mut args = vec![
                "--file-selection".to_string(),
                format!("--title={prompt}"),
                format!("--file-filter={label} | {patterns}"),
            ];
            if kind == FileKind::App {
                args.push("--filename=/usr/share/applications/".to_string());
            }
            args
        }
        Chooser::Kdialog => {
            let start = if kind == FileKind::App {
                "/usr/share/applications"
            } else {
                "."
            };
            vec![
                "--getopenfilename".to_string(),
                start.to_string(),
                format!("{patterns}|{label}"),
                "--title".to_string(),
                prompt.to_string(),
            ]
        }
    }
}

/// The app id of a file picked with the chooser: the stem of a `.desktop` file, or the lower-case
/// file name of an executable.
pub fn app_id_of_path(path: &str) -> Option<String> {
    let name = std::path::Path::new(path.trim()).file_name()?.to_str()?;
    let id = super::desktop_entry::id_key(name);
    if id.is_empty() { None } else { Some(id) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::linux::desktop_entry::Entry;

    fn index() -> Index {
        Index::new(vec![Entry {
            id: "firefox".into(),
            name: "Firefox".into(),
            wm_class: None,
            exec_name: None,
            path: "/usr/share/applications/firefox.desktop".into(),
            icon: None,
            no_display: false,
            single_window: false,
        }])
    }

    #[test]
    fn known_ids_launch_through_their_entry() {
        assert_eq!(
            resolve("Firefox", &index()),
            Launch::Desktop("/usr/share/applications/firefox.desktop".into())
        );
        assert_eq!(
            resolve("firefox.desktop", &index()),
            Launch::Desktop("/usr/share/applications/firefox.desktop".into())
        );
    }

    #[test]
    fn paths_and_unknown_names_run_directly() {
        assert_eq!(
            resolve("/opt/app/run", &index()),
            Launch::Program("/opt/app/run".into())
        );
        assert_eq!(resolve(" htop ", &index()), Launch::Program("htop".into()));
    }

    #[test]
    fn zenity_arguments() {
        let a = chooser_args(Chooser::Zenity, FileKind::Image, "Pick");
        assert_eq!(a[0], "--file-selection");
        assert_eq!(a[1], "--title=Pick");
        assert!(a[2].starts_with("--file-filter=Images | *.png"));
        let app = chooser_args(Chooser::Zenity, FileKind::App, "Pick");
        assert!(app.contains(&"--filename=/usr/share/applications/".to_string()));
        assert!(app[2].contains("*.desktop"));
    }

    #[test]
    fn kdialog_arguments() {
        let a = chooser_args(Chooser::Kdialog, FileKind::Json, "Open");
        assert_eq!(a[0], "--getopenfilename");
        assert_eq!(a[2], "*.json *.JSON|JSON files");
        assert_eq!(&a[3..], ["--title", "Open"]);
        let app = chooser_args(Chooser::Kdialog, FileKind::App, "Open");
        assert_eq!(app[1], "/usr/share/applications");
        assert_eq!(Chooser::Zenity.program(), "zenity");
        assert_eq!(Chooser::Kdialog.program(), "kdialog");
    }

    #[test]
    fn a_picked_file_becomes_an_app_id() {
        assert_eq!(
            app_id_of_path("/usr/share/applications/Firefox.desktop").as_deref(),
            Some("firefox")
        );
        assert_eq!(app_id_of_path(" /usr/bin/Htop\n").as_deref(), Some("htop"));
        assert_eq!(app_id_of_path("/"), None);
        assert_eq!(app_id_of_path(""), None);
    }
}
