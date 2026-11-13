//! The GNOME Shell adapter. Mutter has no call a program can use to ask for the pointer or the
//! windows, and `org.gnome.Shell.Eval` is closed to programs, so the only way is code that runs
//! inside GNOME Shell: a small extension, shipped in `packaging/linux/gnome-extension/`.
//!
//! The extension reports to the same D-Bus service the KWin script does (`kwin::SERVICE`, served
//! by `bus.rs`, same JSON, same range checks) and answers one call, [`BRIDGE_SYNC`] on
//! [`BRIDGE_NAME`], which makes it report at once. It must be installed and enabled by the user
//! (a new extension is only found after a log out on Wayland), which is why the report text
//! ([`enable_steps`]) says how. CursorPie never writes into GNOME's configuration.
//!
//! **Not run.** See `docs/developer/WAYLAND_SUPPORT.md`.

/// The extension's id: the name of its directory and what `gnome-extensions enable` is given.
pub(crate) const EXTENSION_UUID: &str = "cursorpie-adapter@sebastvnd.github.io";
/// The name, object and interface the extension owns on the session bus.
pub(crate) const BRIDGE_NAME: &str = "org.cursorpie.GnomeBridge";
pub(crate) const BRIDGE_PATH: &str = "/org/cursorpie/GnomeBridge";
pub(crate) const BRIDGE_IFACE: &str = "org.cursorpie.GnomeBridge";
/// The method that makes the extension report now.
pub(crate) const BRIDGE_SYNC: &str = "Sync";

/// The files of the extension, compiled into the program so that the copy the person installed
/// can be brought up to date after an update (see `platform::linux::refresh`).
pub(crate) const SCRIPT: &str = include_str!(
    "../../../../packaging/linux/gnome-extension/cursorpie-adapter@sebastvnd.github.io/extension.js"
);
pub(crate) const METADATA: &str = include_str!(
    "../../../../packaging/linux/gnome-extension/cursorpie-adapter@sebastvnd.github.io/metadata.json"
);

/// What to do to turn the extension on, for the report text.
pub(crate) fn enable_steps() -> String {
    format!(
        "Copy the folder {EXTENSION_UUID} (in the package, or in packaging/linux/gnome-extension/ \
         of the source) to ~/.local/share/gnome-shell/extensions/, log out and in, then run \
         `gnome-extensions enable {EXTENSION_UUID}`."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extension_and_the_program_agree_on_every_name() {
        use super::super::kwin::{IFACE, PATH, SERVICE};
        for name in [
            SERVICE,
            PATH,
            IFACE,
            BRIDGE_NAME,
            BRIDGE_PATH,
            BRIDGE_IFACE,
            BRIDGE_SYNC,
        ] {
            assert!(SCRIPT.contains(name), "extension.js lacks {name}");
        }
        assert!(METADATA.contains(EXTENSION_UUID));
    }

    #[test]
    fn the_extension_does_only_what_it_says() {
        for forbidden in [
            "eval(",
            "Function(",
            "spawn",
            "Gio.File",
            "Gio.Subprocess",
            "GLib.file_",
            "get_title",
        ] {
            assert!(!SCRIPT.contains(forbidden), "{forbidden}");
        }
    }

    #[test]
    fn the_steps_name_the_extension_and_the_command() {
        let s = enable_steps();
        assert!(s.contains(EXTENSION_UUID));
        assert!(s.contains("gnome-extensions enable"));
    }
}
