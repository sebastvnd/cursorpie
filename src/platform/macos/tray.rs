//! The tray icon (the menu bar on macOS): Settings, About, Welcome guide, Pause, Launch at login,
//! Quit.
//!
//! tray-icon must be created on the main thread once the event loop is running; eframe calls the
//! app creator from `resumed`, which satisfies that.

use crate::about::APP_NAME;
use crate::msg::Msg;
use crate::msg::Notifier;
use crate::platform;
use crate::ui::tray::icon_rgba;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// Tray handles the app keeps: the icon (dropping it removes it) and the two check items it updates.
pub struct TrayHandles {
    /// keep alive: dropping it removes the icon
    pub _icon: TrayIcon,
    pause: CheckMenuItem,
    login: CheckMenuItem,
}

impl TrayHandles {
    /// Whether "Pause" is ticked.
    pub fn pause_checked(&self) -> bool {
        self.pause.is_checked()
    }
    /// Whether "Launch at login" is ticked.
    pub fn login_checked(&self) -> bool {
        self.login.is_checked()
    }
    /// Ticks or unticks "Launch at login".
    pub fn set_login_checked(&self, on: bool) {
        self.login.set_checked(on);
    }
}

/// Creates the tray icon and its menu. Menu clicks are forwarded as `Msg::Tray(id)`.
/// Returns `None` (after logging) if the icon cannot be created.
pub fn build_tray(n: &Notifier) -> Option<TrayHandles> {
    let menu = Menu::new();
    let settings = MenuItem::with_id("settings", "Settings...", true, None);
    let about = MenuItem::with_id("about", format!("About {APP_NAME}"), true, None);
    let welcome = MenuItem::with_id("welcome", "Welcome guide...", true, None);
    let swap = MenuItem::with_id("swap_side", "Switch side-button pie", true, None);
    let pause = CheckMenuItem::with_id("pause", format!("Pause {APP_NAME}"), true, false, None);
    let login = CheckMenuItem::with_id(
        "login",
        "Launch at login",
        true,
        platform::launch_at_login_enabled(),
        None,
    );
    let quit = MenuItem::with_id("quit", format!("Quit {APP_NAME}"), true, None);

    let _ = menu.append(&settings);
    let _ = menu.append(&about);
    let _ = menu.append(&welcome);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&swap);
    let _ = menu.append(&pause);
    let _ = menu.append(&login);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&quit);

    // forward menu clicks to the UI thread (this also wakes it up)
    let notifier = n.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        notifier.send(Msg::Tray(e.id.0.clone()));
    }));

    let rgb = [0, 0, 0];
    let icon = match Icon::from_rgba(icon_rgba(44, rgb), 44, 44) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("CursorPie: could not create tray icon image: {e}");
            return None;
        }
    };
    match TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(APP_NAME)
        .with_icon(icon)
        .with_icon_as_template(true) // macOS only; ignored on Windows
        .build()
    {
        Ok(tray) => Some(TrayHandles {
            _icon: tray,
            pause,
            login,
        }),
        Err(e) => {
            eprintln!(
                "CursorPie: could not create the {} icon: {e}",
                crate::platform::OS.tray_name()
            );
            None
        }
    }
}
