//! "Launch at login" on Linux: an XDG autostart entry, `~/.config/autostart/cursorpie.desktop`.
//! This module builds and reads the file text; the backend reads and writes the file.
//!
//! The entry asks the session to wait a few seconds (`X-GNOME-Autostart-Delay`, after the panel on
//! KDE): at the very start of a session the X server (XWayland) or the input devices may not be
//! ready yet, and the program would start without a working display.

/// File name of the autostart entry.
pub const FILE_NAME: &str = "cursorpie.desktop";

/// An `Exec` value for `path`, quoted as the Desktop Entry specification requires. Reserved
/// characters make the path a double-quoted argument; inside it `"`, `` ` ``, `$` and `\` are
/// escaped, then every backslash is doubled (the value is also a string), and `%` becomes `%%`.
pub fn quote_exec(path: &str) -> String {
    const RESERVED: &[char] = &[
        ' ', '\t', '\n', '"', '\'', '\\', '>', '<', '~', '|', '&', ';', '$', '*', '?', '#', '(',
        ')', '`', '%',
    ];
    if !path.contains(RESERVED) {
        return path.to_string();
    }
    let mut arg = String::with_capacity(path.len() + 2);
    for c in path.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                arg.push('\\');
                arg.push(c);
            }
            '%' => arg.push_str("%%"),
            _ => arg.push(c),
        }
    }
    let doubled = arg.replace('\\', "\\\\");
    format!("\"{doubled}\"")
}

/// The text of the autostart entry that starts the program at `exec_path`.
pub fn contents(exec_path: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=CursorPie\n\
         Comment=Radial pie menu for the mouse\n\
         Exec={}\n\
         Icon=cursorpie\n\
         Terminal=false\n\
         StartupNotify=false\n\
         StartupWMClass=cursorpie\n\
         X-GNOME-Autostart-enabled=true\n\
         X-GNOME-Autostart-Delay=5\n\
         X-KDE-autostart-after=panel\n",
        quote_exec(exec_path)
    )
}

/// Whether the text of an autostart entry means "start at login": desktops switch an entry off by
/// writing `Hidden=true` or `X-GNOME-Autostart-enabled=false` into it.
pub fn is_enabled(text: &str) -> bool {
    let mut in_main = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let off = value.trim().eq_ignore_ascii_case("true");
            match key.trim() {
                "Hidden" if off => return false,
                "X-GNOME-Autostart-enabled" if value.trim().eq_ignore_ascii_case("false") => {
                    return false;
                }
                _ => {}
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_paths_are_not_quoted() {
        assert_eq!(quote_exec("/usr/bin/cursorpie"), "/usr/bin/cursorpie");
    }

    #[test]
    fn paths_with_spaces_and_specials_are_quoted_and_escaped() {
        assert_eq!(
            quote_exec("/opt/my app/cursorpie"),
            "\"/opt/my app/cursorpie\""
        );
        assert_eq!(quote_exec("/a$b"), "\"/a\\\\$b\"");
        assert_eq!(quote_exec("/a\"b"), "\"/a\\\\\"b\"");
        assert_eq!(quote_exec("/100%"), "\"/100%%\"");
        assert_eq!(quote_exec("/a\\b"), "\"/a\\\\\\\\b\"");
    }

    #[test]
    fn the_entry_runs_the_program_at_login() {
        let t = contents("/usr/bin/cursorpie");
        assert!(t.starts_with("[Desktop Entry]\n"));
        assert!(t.contains("\nExec=/usr/bin/cursorpie\n"));
        assert!(t.contains("\nType=Application\n"));
        assert!(is_enabled(&t));
    }

    #[test]
    fn desktops_can_switch_the_entry_off() {
        let off = "[Desktop Entry]\nType=Application\nHidden=true\n";
        assert!(!is_enabled(off));
        let gnome = "[Desktop Entry]\nExec=x\nX-GNOME-Autostart-enabled=false\n";
        assert!(!is_enabled(gnome));
        assert!(is_enabled("[Desktop Entry]\nHidden=false\n"));
        assert!(is_enabled(
            "[Desktop Action x]\nHidden=true\n[Desktop Entry]\nExec=x\n"
        ));
        assert!(is_enabled(""));
    }
}
