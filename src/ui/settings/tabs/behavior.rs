//! Behavior tab: trigger and timings, look, and how the app / side-button pies are chosen.
//!
//! One column of titled panels (`widgets::panel`), the way the pages of a large application's
//! settings are laid out: what you press, how the pie looks, which pie opens for which app, then
//! startup and memory.

use crate::config::{
    AppGesture, AppIconStyle, Appearance, Config, DesktopMode, HudPosition, IconShadow, PieStyle,
    SideTarget, TargetApp, TouchClickAction,
};
use crate::platform::Os;
use crate::ui::app::App;
use crate::ui::draw::rgb;
use crate::ui::pie::glass::Glass;
use crate::ui::settings::demo;
use crate::ui::settings::state::BehaviorPage;
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use eframe::egui::{self, Color32, Vec2};

/// How to put the extension in the browser: the files are written by the button above. A browser
/// only takes an extension that is signed by its store or that the person loads by hand.
const BRIDGE_INSTALL_HELP: &str = "Add the extension once: in Chrome, Edge, Brave or Vivaldi open the extensions page, turn on Developer mode, choose Load unpacked and pick the 'chromium' folder. In Firefox open about:debugging, choose This Firefox, then Load Temporary Add-on and pick manifest.json in the 'firefox' folder (Firefox forgets a temporary add-on when it closes; a signed copy from the release page stays). It connects within a minute.";

/// The line under the switch: which browsers are connected, or what is still missing.
fn bridge_status_text(st: &crate::bridge::Status) -> String {
    if !st.running {
        return "Not started yet: save the settings to start the link.".into();
    }
    if !st.browsers.is_empty() {
        let list: Vec<String> = st
            .browsers
            .iter()
            .map(|(b, n)| format!("{b} ({n} {})", if *n == 1 { "tab" } else { "tabs" }))
            .collect();
        let mut text = format!("Connected: {}.", list.join(", "));
        if st.playing > 0 {
            text += &format!(
                " Sound in {} {}.",
                st.playing,
                if st.playing == 1 { "tab" } else { "tabs" }
            );
        }
        for (site, unread) in &st.chats {
            let name = crate::bridge::chat_label(site);
            match unread {
                Some(n) if *n > 0 => text += &format!(" {name}: {n} unread."),
                _ => text += &format!(" {name} is open."),
            }
        }
        if !st.outdated.is_empty() {
            text += &format!(
                " The extension in {} is from another version: show the extension files and reload it, or the newer commands will not work.",
                st.outdated.join(", ")
            );
        }
        return text;
    }
    if let Some(e) = st.errors.first() {
        return format!("No browser connected yet. Could not register with a browser: {e}");
    }
    if st.registered.is_empty() {
        return "No browser connected yet, and no supported browser was found (Flatpak and Snap browsers cannot be reached).".into();
    }
    format!(
        "No browser connected yet. Ready for: {}. Add the extension below.",
        st.registered.join(", ")
    )
}

/// What the OS calls a desktop: a Space on macOS, a virtual desktop on Windows, a workspace on Linux.
fn desktop_noun() -> &'static str {
    match crate::platform::OS {
        crate::platform::Os::Mac => "Space",
        crate::platform::Os::Windows => "virtual desktop",
        crate::platform::Os::Linux => "workspace",
    }
}

/// Title of the panel that holds the [`DesktopMode`] choices.
fn desktops_title() -> &'static str {
    match crate::platform::OS {
        crate::platform::Os::Mac => "Other Spaces",
        crate::platform::Os::Windows => "Other desktops",
        crate::platform::Os::Linux => "Other workspaces",
    }
}

/// How a new window of an app that already runs is opened, for the note under the switch of the
/// "Open app" choice.
fn new_window_hint() -> &'static str {
    match crate::platform::OS {
        crate::platform::Os::Mac => {
            "A new window brings the app forward and presses Cmd+N, so it works in apps that use that shortcut."
        }
        crate::platform::Os::Windows | crate::platform::Os::Linux => {
            "A new window starts the app again, which most apps answer with a new window."
        }
    }
}

/// The text of one [`DesktopMode`] choice.
fn desktop_mode_label(mode: DesktopMode) -> String {
    let noun = desktop_noun();
    match mode {
        DesktopMode::SwitchToApp => format!("Switch to the {noun} where the app is"),
        DesktopMode::BringToCurrent => {
            format!("Bring the app's window to this {noun} (experimental)")
        }
        DesktopMode::DoNothing => format!("Do nothing: open the app on this {noun}, as before"),
    }
}

impl App {
    /// Draws the Behavior tab: a switch between its pages, then the panels of the page that is
    /// chosen. Widgets edit `self.edit`, so nothing changes until Save.
    pub(in crate::ui::settings) fn behavior_tab(&mut self, ui: &mut egui::Ui) {
        let accent = rgb(self.edit.accent);
        // the pages are chosen with a switch of icons that floats over the bottom of the page
        let items: Vec<(char, String)> = BehaviorPage::ALL
            .iter()
            .map(|p| (p.icon(), format!("{}: {}", p.label(), p.hint())))
            .collect();
        let at = BehaviorPage::ALL
            .iter()
            .position(|p| *p == self.ed.behavior_page)
            .unwrap_or(0);
        let view = ui.clip_rect();
        let mut picked = None;
        egui::Area::new(egui::Id::new("behavior_pages"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_BOTTOM)
            .fixed_pos(egui::pos2(view.center().x, view.max.y - 20.0))
            .constrain(false)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(pal().card)
                    .inner_margin(egui::Margin::same(1))
                    .stroke(egui::Stroke::new(1.0, pal().border))
                    .corner_radius(PILL)
                    .shadow(popup_shadow())
                    .show(ui, |ui| {
                        picked = icon_switch(ui, &items, at, accent, pal().card, 40.0);
                    });
            });
        if let Some(i) = picked {
            self.ed.behavior_page = BehaviorPage::ALL[i];
        }
        match self.ed.behavior_page {
            BehaviorPage::Input => self.behavior_input(ui, accent),
            BehaviorPage::Look => self.behavior_look(ui, accent),
            BehaviorPage::Apps => self.behavior_apps(ui, accent),
            BehaviorPage::System => self.behavior_system(ui, accent),
        }
        // room under the last panel for the floating switch
        ui.add_space(100.0);
    }

    /// The "Input" page of the Behavior tab: trigger button and timings, the side buttons and the trackpad.
    fn behavior_input(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let cfg = &mut self.edit;
        panel(ui, "Trigger", |ui| {
            demo::hold_demo(ui, accent, cfg.pie_hold_ms, cfg.settings_hold_ms, cfg.flick);
            let name = |b: i64| match b {
                2 => "Middle button",
                5 => "Extra button 6",
                6 => "Extra button 7",
                _ => "Extra button 8",
            };
            row(
                ui,
                "Trigger button",
                "The mouse button you hold to open the pie. A trackpad has its own hold in the Trackpad section.",
                |ui| {
                    let buttons = crate::platform::OS.trigger_buttons();
                    let labels: Vec<&str> = buttons.iter().map(|&b| name(b)).collect();
                    let at = buttons
                        .iter()
                        .position(|&b| b == cfg.trigger_button)
                        .unwrap_or(0);
                    if let Some(i) = select(ui, 190.0, name(cfg.trigger_button), &labels, at) {
                        cfg.trigger_button = buttons[i];
                    }
                },
            );
            slider_row(
                ui,
                "Hold to open the pie",
                &mut cfg.pie_hold_ms,
                50..=1500,
                " ms",
                accent,
            );
            slider_row(
                ui,
                "Hold to open settings",
                &mut cfg.settings_hold_ms,
                500..=4000,
                " ms",
                accent,
            );
            toggle_row(
                ui,
                &mut cfg.flick,
                "Open the pie instantly when I drag",
                "Disables middle-drag in apps.",
                accent,
            );
        });
        panel(ui, "Side buttons", |ui| {
            demo::side_demo(
                ui,
                accent,
                rgb(cfg.inactive_color),
                cfg.side_default,
                cfg.right_click_switch,
                cfg.side_indicator,
            );
            note(
                ui,
                "In an app that has its own pie, the side buttons follow:",
            );
            choice_row(
                ui,
                &mut cfg.side_default,
                SideTarget::App,
                "The app's pie",
                "",
                accent,
            );
            choice_row(
                ui,
                &mut cfg.side_default,
                SideTarget::General,
                "The general pie",
                "",
                accent,
            );
            toggle_row(
                ui,
                &mut cfg.side_adopt,
                "Follow the pie I choose a mode in",
                "Choosing a side-button mode in a pie makes the side buttons follow that pie.",
                accent,
            );
            toggle_row(
                ui,
                &mut cfg.side_forget,
                "Forget that choice on another app",
                "When I switch to another app.",
                accent,
            );
            toggle_row(
                ui,
                &mut cfg.right_click_switch,
                "Right-click switches the pie",
                "While the pie is open, right-click switches which pie the side buttons follow.",
                accent,
            );
            toggle_row(
                ui,
                &mut cfg.side_indicator,
                "Color the pie ring",
                "Accent = the side buttons follow this pie; the other color = the other pie.",
                accent,
            );
            row_wide(ui, "Other-pie color", "", 40.0, |ui| {
                ui.color_edit_button_srgb(&mut cfg.inactive_color);
            });
            slider_row(
                ui,
                &format!(
                    "Show the pie after switching ({})",
                    crate::platform::OS.tray_name()
                ),
                &mut cfg.preview_ms,
                0..=2000,
                " ms",
                accent,
            );
        });
        if crate::platform::OS.has_trackpad() {
            trackpad_panel(ui, cfg, accent, crate::tap::touch_status());
        }
    }

    /// The "Look" page of the Behavior tab: the look of the pie and the notice on the screen.
    fn behavior_look(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let cfg = &mut self.edit;
        panel(ui, "Look & feel", |ui| {
            demo::look_demo(
                ui,
                accent,
                cfg.pie_size,
                cfg.show_labels,
                cfg.center_app_icon,
                cfg.app_icon_style,
                Glass::of(cfg),
            );
            row_wide(ui, "Appearance", "Settings and the pie.", 300.0, |ui| {
                let modes = [
                    (Appearance::System, symbols::MONITOR, "System"),
                    (Appearance::Light, symbols::SUN, "Light"),
                    (Appearance::Dark, symbols::MOON, "Dark"),
                ];
                let items: Vec<(Option<char>, &str)> = modes
                    .iter()
                    .map(|&(_, icon, label)| (Some(icon), label))
                    .collect();
                let at = modes
                    .iter()
                    .position(|m| m.0 == cfg.appearance)
                    .unwrap_or(0);
                if let Some(i) = segmented(ui, &items, at) {
                    cfg.appearance = modes[i].0;
                }
            });
            slider_row(
                ui,
                "Pie size",
                &mut cfg.pie_size,
                140.0..=360.0,
                " px",
                accent,
            );
            row_wide(ui, "Accent color", "", 330.0, |ui| {
                accent_picker(ui, &mut cfg.accent)
            });
            toggle_row(
                ui,
                &mut cfg.sound,
                "Play a soft tick",
                "When the selection changes.",
                accent,
            );
            toggle_row(
                ui,
                &mut cfg.show_labels,
                "Show names under the icons",
                "Off: the pie shows icons only.",
                accent,
            );
            toggle_row(
                ui,
                &mut cfg.center_app_icon,
                "Show the app's icon in the center",
                "In an app's own pie.",
                accent,
            );
        });
        panel(ui, "Pie material", |ui| {
            row_wide(
                ui,
                "Style",
                "Glass is see-through and frosted, always smoked dark.",
                230.0,
                |ui| {
                    let styles = [
                        (PieStyle::Solid, symbols::CHART_PIE_SLICE, "Solid"),
                        (PieStyle::Glass, symbols::SPARKLE, "Glass"),
                    ];
                    let items: Vec<(Option<char>, &str)> = styles
                        .iter()
                        .map(|&(_, icon, label)| (Some(icon), label))
                        .collect();
                    let at = styles
                        .iter()
                        .position(|m| m.0 == cfg.pie_style)
                        .unwrap_or(0);
                    if let Some(i) = segmented(ui, &items, at) {
                        cfg.pie_style = styles[i].0;
                    }
                },
            );
            if cfg.pie_style == PieStyle::Glass {
                glass_preview(ui, cfg);
                slider_row(ui, "Clearness", &mut cfg.glass_clear, 0..=100, " %", accent);
                slider_row(ui, "Frost", &mut cfg.glass_frost, 0..=100, " %", accent);
                slider_row(ui, "Shine", &mut cfg.glass_shine, 0..=100, " %", accent);
            }
        });
        panel(ui, "App icon in the pie", |ui| {
            demo::app_icon_demo(ui, accent, cfg.app_icon_style, cfg.center_app_icon);
            ui.add_enabled_ui(cfg.center_app_icon, |ui| {
                choice_row(
                    ui,
                    &mut cfg.app_icon_style,
                    AppIconStyle::Glyph,
                    "One color",
                    "A silhouette of the icon in the ink of the pie, like the other icons.",
                    accent,
                );
                choice_row(
                    ui,
                    &mut cfg.app_icon_style,
                    AppIconStyle::Color,
                    "Full color",
                    "The app's own icon, in its own colors.",
                    accent,
                );
            });
            ui.add_enabled_ui(cfg.center_app_icon, |ui| {
                ui.add_space(GAP / 2.0);
                section_title(ui, "Shadow under the icon");
                choice_row(
                    ui,
                    &mut cfg.app_icon_shadow,
                    IconShadow::Auto,
                    "Automatic",
                    "A soft shadow on a plain light center, none where the icon already stands out.",
                    accent,
                );
                choice_row(
                    ui,
                    &mut cfg.app_icon_shadow,
                    IconShadow::Soft,
                    "Soft",
                    "Always a light shadow.",
                    accent,
                );
                choice_row(
                    ui,
                    &mut cfg.app_icon_shadow,
                    IconShadow::Strong,
                    "Strong",
                    "A deeper shadow for pale icons on a white center.",
                    accent,
                );
                choice_row(
                    ui,
                    &mut cfg.app_icon_shadow,
                    IconShadow::Off,
                    "None",
                    "No shadow.",
                    accent,
                );
            });
            if !cfg.center_app_icon {
                note(
                    ui,
                    "Off: turn on \"Show the app's icon in the center\" under Look & feel to choose.",
                );
            }
        });
        panel(ui, "Action notice", |ui| {
            toggle_row(
                ui,
                &mut cfg.hud,
                "Show what CursorPie does",
                "A small notice on the screen when an app opens, a link opens, a shortcut is sent or a flow runs.",
                accent,
            );
            if cfg.hud {
                hud_preview(ui, cfg);
                row_wide(
                    ui,
                    "Position",
                    "On the display the pointer is on.",
                    300.0,
                    |ui| {
                        let places = [
                            (HudPosition::Bottom, symbols::ARROW_DOWN, "Bottom"),
                            (HudPosition::Top, symbols::ARROW_UP, "Top"),
                            (HudPosition::Cursor, symbols::CURSOR_CLICK, "Pointer"),
                        ];
                        let items: Vec<(Option<char>, &str)> = places
                            .iter()
                            .map(|&(_, icon, label)| (Some(icon), label))
                            .collect();
                        let at = places
                            .iter()
                            .position(|m| m.0 == cfg.hud_position)
                            .unwrap_or(0);
                        if let Some(i) = segmented(ui, &items, at) {
                            cfg.hud_position = places[i].0;
                        }
                    },
                );
                slider_row(
                    ui,
                    "Stays on screen",
                    &mut cfg.hud_ms,
                    crate::config::HUD_MS_MIN..=3000,
                    " ms",
                    accent,
                );
                slider_row(
                    ui,
                    "Size",
                    &mut cfg.hud_size,
                    crate::config::HUD_SIZE_MIN..=crate::config::HUD_SIZE_MAX,
                    "%",
                    accent,
                );
            }
        });
    }

    /// The "Apps" page of the Behavior tab: which pie opens for which app, how apps are found and the desktops.
    fn behavior_apps(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let cfg = &mut self.edit;
        panel(ui, "Pie of the app in front", |ui| {
            demo::gesture_demo(ui, accent, cfg.app_gesture, cfg.double_click_ms);
            choice_row(
                ui,
                &mut cfg.app_gesture,
                AppGesture::DoubleClick,
                "Double-click opens the app's pie",
                "Hold opens the general pie; double-click opens the pie of the app (e.g. Photoshop).",
                accent,
            );
            choice_row(
                ui,
                &mut cfg.app_gesture,
                AppGesture::Merged,
                "Classic",
                "Hold opens the app's own pie if it has one.",
                accent,
            );
            if cfg.app_gesture == AppGesture::DoubleClick {
                slider_row(
                    ui,
                    "Double-click speed",
                    &mut cfg.double_click_ms,
                    150..=500,
                    " ms",
                    accent,
                );
            }
        });
        panel(ui, "App detection", |ui| {
            demo::target_demo(ui, accent, cfg.target_app, cfg.focus_target);
            choice_row(
                ui,
                &mut cfg.target_app,
                TargetApp::UnderCursor,
                "The app under the pointer",
                "No need to click its window first.",
                accent,
            );
            choice_row(
                ui,
                &mut cfg.target_app,
                TargetApp::Focused,
                "The app in front",
                "The one with keyboard focus.",
                accent,
            );
            ui.add_enabled_ui(cfg.target_app == TargetApp::UnderCursor, |ui| {
                toggle_row(
                    ui,
                    &mut cfg.focus_target,
                    "Bring that app to the front first",
                    "Before a slice or side button sends it a shortcut.",
                    accent,
                );
            });
        });
        panel(ui, "Open app and web page slices", |ui| {
            demo::open_choice_demo(ui, accent, cfg.open_app_choice);
            toggle_row(
                ui,
                &mut cfg.open_app_choice,
                "Choose between a new window and the open app or page",
                "When the app already runs, hovering its slice shows two round badges beside it: a green + (new window) and a yellow one with a square (the app that is open). A web link gets the same badges only with the browser extension below.",
                accent,
            );
            note(
                ui,
                if cfg.open_app_choice {
                    "Release on the half of the slice nearer a badge to pick it: the green + is on the side where the slice starts, the yellow one on the side where it ends. An app that is not running, or a page that is not open, is simply opened."
                } else {
                    "Off: an Open app slice always opens the app, or goes to it when it runs, as before."
                },
            );
            if cfg.open_app_choice {
                note(ui, new_window_hint());
            }
        });
        panel(ui, "Browser extension (optional)", |ui| {
            toggle_row(
                ui,
                &mut cfg.browser_bridge,
                "Know which web pages are open, and control the browser",
                "CursorPie carries a small extension for Firefox and Chromium browsers (Chrome, Edge, Brave, Vivaldi). With it, an Open URL slice with an http or https link shows the two badges when that page is open, goes to that very tab, and opens a real new window. It also lets a Browser slice play, pause and skip songs, switch and close tabs, and move around WhatsApp Web and Telegram Web. Off, or without the extension, an Open URL slice just opens its link and a Browser slice tells you it cannot reach a browser.",
                accent,
            );
            if cfg.browser_bridge {
                note(ui, &bridge_status_text(&crate::bridge::status()));
                actions(ui, |ui| {
                    if button(
                        ui,
                        Look::Soft,
                        Some(symbols::FOLDER_OPEN),
                        "Show the extension files",
                    )
                    .clicked()
                        && let Err(e) = crate::bridge::extension::export_and_show()
                    {
                        eprintln!("CursorPie: could not write the browser extension: {e}");
                    }
                });
                note(ui, BRIDGE_INSTALL_HELP);
                toggle_row(
                    ui,
                    &mut cfg.extension_panels,
                    "Let the browser extension show panels",
                    "A panel is a small window of CursorPie with a list, a box or buttons. With this on, the extension's own commands (Panels: pick a tab from a list, ask for a word then go to that tab, a media menu...) and other code of the extension can ask you something on the desktop and get your answer. The panel shows what the extension wrote in it, such as the titles of your open tabs, only on screen and only for you; nothing is saved or sent anywhere. Off, those commands say that panels are switched off.",
                    accent,
                );
            } else {
                note(
                    ui,
                    "Nothing is installed or listened to while this is off. The extension only talks to CursorPie on this computer (the address of the pages that are open, how many tabs make sound, how many chats wait, the answer to a command); it sends nothing over the network.",
                );
            }
        });
        panel(ui, "Panels of flows", |ui| {
            toggle_row(
                ui,
                &mut cfg.panels,
                "Let flows show panels",
                "An Ask block in Panel mode shows a small window of CursorPie with the fields, lists and buttons the flow describes, and the flow gets what you entered. An Update panel block opens a progress panel of its own. Off, those blocks fail with a note that panels are switched off and the flow goes on.",
                accent,
            );
            if cfg.panels {
                toggle_row(
                    ui,
                    &mut cfg.progress_panel,
                    "Show a progress panel for long runs",
                    "When a flow, a pause or a sequence takes more than a couple of seconds, a small panel shows a bar, the step that is running (Waiting, Running command, Round 2/10) and a Stop button that ends the run. Quick runs never show it. A run that presses keys keeps the small notice instead on macOS and Linux, because a window may take the keyboard from the app those keys are for. Off, long runs only have the small notice.",
                    accent,
                );
            }
        });
        panel(ui, desktops_title(), |ui| {
            let mode = cfg.desktop_mode.effective(crate::platform::OS);
            demo::desktop_demo(ui, accent, mode, desktop_noun());
            note(
                ui,
                &format!(
                    "An 'Open app' slice may open an app whose window is on another {}.",
                    desktop_noun()
                ),
            );
            for &mode in crate::platform::OS.desktop_modes() {
                choice_row(
                    ui,
                    &mut cfg.desktop_mode,
                    mode,
                    &desktop_mode_label(mode),
                    "",
                    accent,
                );
            }
        });
    }

    /// The "System" page of the Behavior tab: startup and memory.
    fn behavior_system(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let cfg = &mut self.edit;
        panel(ui, "Startup", |ui| {
            toggle_row(
                ui,
                &mut cfg.launch_at_login,
                "Launch at login",
                crate::platform::OS.startup_hint(),
                accent,
            );
        });
        panel(ui, "Memory", |ui| {
            demo::memory_demo(ui, accent, cfg.sleep_after_s != 0, cfg.sleep_after_s);
            let mut on = cfg.sleep_after_s != 0;
            if toggle_row(
                ui,
                &mut on,
                "Free memory while idle",
                "After this long without a pie or Settings, the editor's working copy and the cached icons of your other pies are dropped. They are loaded again when you press the trigger; the icons of your general pie stay, so it opens just as fast.",
                accent,
            )
            .changed()
            {
                cfg.sleep_after_s = if on { 60 } else { 0 };
            }
            if cfg.sleep_after_s != 0 {
                slider_row(
                    ui,
                    "Idle time before freeing",
                    &mut cfg.sleep_after_s,
                    10..=600,
                    " s",
                    accent,
                );
            }
        });
    }
}

/// What the last scan found, for the line under the Scan button; `None` before the first scan.
/// `on` is the state of the switch now.
fn scan_text(os: Os, scanned: Option<usize>, on: bool) -> Option<String> {
    let name = if os == Os::Windows {
        "precision touchpad"
    } else if os == Os::Linux {
        "touchpad"
    } else {
        "trackpad"
    };
    scanned.map(|n| match (n, on) {
        (0, _) if os == Os::Windows => {
            "Scan: no precision touchpad found. A touchpad that Windows treats as a plain mouse cannot be read."
                .to_string()
        }
        (0, _) => format!("Scan: no {name} found."),
        (n, true) => format!(
            "Scan: {n} {name}{} found. The hold is on; press Save to keep it.",
            if n == 1 { "" } else { "s" }
        ),
        (n, false) => format!(
            "Scan: {n} {name}{} found. Turn the switch on to use it.",
            if n == 1 { "" } else { "s" }
        ),
    })
}

/// The line under the Trackpad switch: what the touch source found.
fn trackpad_status_text(on: bool, status: crate::tap::TouchStatus) -> String {
    trackpad_status_text_for(crate::platform::OS, on, status)
}

/// [`trackpad_status_text`] for an OS (so the tests can say what each one tells).
fn trackpad_status_text_for(os: Os, on: bool, status: crate::tap::TouchStatus) -> String {
    use crate::tap::TouchStatus::*;
    let windows = os == Os::Windows;
    let linux = os == Os::Linux;
    match (on, status) {
        (false, _) if linux => "Off. The touchpad is not read.".into(),
        (false, _) if windows => "Off. Nothing is registered with Windows.".into(),
        (false, _) => "Off. The system's touch interface is not loaded.".into(),
        (true, Off) => "Starts when you press Save.".into(),
        (true, Unavailable) if linux => {
            "Touchpads cannot be read here (no /dev/input).".into()
        }
        (true, Unavailable) if windows => {
            "Windows refused to deliver touchpad input to CursorPie.".into()
        }
        (true, Unavailable) => "Trackpad support is not available on this macOS.".into(),
        (true, NoDevice) if windows => {
            "No precision touchpad found. A touchpad that Windows treats as a plain mouse cannot be read."
                .into()
        }
        (true, NoDevice) => "No trackpad found.".into(),
        (true, NoPermission { devices }) => format!(
            "{} found, but this user may not read it. Add yourself to the `input` group \
             (sudo usermod -aG input $USER) and log in again.",
            if devices == 1 {
                "A touchpad".to_string()
            } else {
                format!("{devices} touchpads")
            }
        ),
        (true, Ready { devices, built_in }) if linux => {
            let _ = built_in;
            if devices == 1 {
                "Touchpad found.".into()
            } else {
                format!("{devices} touchpads found.")
            }
        }
        (true, Ready { devices, built_in }) => {
            let what = if windows && devices == 1 {
                "Precision touchpad".to_string()
            } else if windows {
                format!("{devices} precision touchpads")
            } else if devices == 1 && built_in {
                "Built-in trackpad".to_string()
            } else if devices == 1 {
                "Trackpad".to_string()
            } else {
                format!("{devices} trackpads")
            };
            if windows {
                format!("{what} found.")
            } else {
                format!(
                    "{what} found. Haptics follow the system's \"Force Click and haptic feedback\" setting."
                )
            }
        }
    }
}

fn touch_click_label(a: TouchClickAction) -> &'static str {
    match a {
        TouchClickAction::SwitchPie => "Switch the pie (like right-click)",
        TouchClickAction::PickHovered => "Pick the hovered mode",
        TouchClickAction::Nothing => "Do nothing",
    }
}

/// The Trackpad panel (macOS, Windows and Linux X11): hold with fingers to open the pie, one more finger clicks
/// to switch the mode, haptic ticks (macOS only). Everything is off until the switch is on.
fn trackpad_panel(
    ui: &mut egui::Ui,
    cfg: &mut Config,
    accent: Color32,
    status: crate::tap::TouchStatus,
) {
    let os = crate::platform::OS;
    panel(ui, "Trackpad", |ui| {
        note(
            ui,
            if os == Os::Linux {
                "Touch and hold on the touchpad to open the pie, move the finger to a slice, lift to run it. \
                 The touchpad is read from /dev/input, which needs the `input` group (X11 only)."
            } else if os == Os::Windows {
                "Touch and hold on a precision touchpad to open the pie, move the finger to a slice, lift to run it. \
                 This reads the touchpad's raw input (registered only while this is on)."
            } else {
                "Touch and hold on the trackpad to open the pie, move the finger to a slice, lift to run it. \
                 This uses an undocumented system interface (loaded only while this is on)."
            },
        );
        toggle_row(
            ui,
            &mut cfg.touch_trigger,
            "Open the pie with a trackpad hold",
            "Uses the hold time above. Switched on at the first start when a trackpad is found; a finger resting on a trackpad is common, so turn it off here if it gets in the way.",
            accent,
        );
        note(ui, &trackpad_status_text(cfg.touch_trigger, status));
        // A scan looks at the system without starting the touch source, so it works while the
        // switch is off. A trackpad that was not there at the first start (a Bluetooth one, a
        // laptop that was docked) is switched on by it: that is the point of pressing it.
        actions(ui, |ui| {
            if button(ui, Look::Soft, Some(symbols::SCAN), "Scan for trackpad").clicked()
                && crate::tap::touch_scan() > 0
            {
                cfg.touch_trigger = true;
            }
        });
        if let Some(text) = scan_text(os, crate::tap::touch_last_scan(), cfg.touch_trigger) {
            note(ui, &text);
        }
        ui.add_enabled_ui(cfg.touch_trigger, |ui| {
            note(ui, extra_finger_hint(os));
            slider_row(
                ui,
                "How still the finger must stay",
                &mut cfg.touch_still_mm,
                0.5..=6.0,
                " mm",
                accent,
            );
            row(
                ui,
                "Extra click",
                "What one more finger does while the pie is open.",
                |ui| {
                    let all = [
                        TouchClickAction::SwitchPie,
                        TouchClickAction::PickHovered,
                        TouchClickAction::Nothing,
                    ];
                    let labels: Vec<&str> = all.iter().map(|&a| touch_click_label(a)).collect();
                    let at = all
                        .iter()
                        .position(|&a| a == cfg.touch_click_action)
                        .unwrap_or(0);
                    if let Some(i) = select(ui, 230.0, touch_click_label(cfg.touch_click_action), &labels, at) {
                        cfg.touch_click_action = all[i];
                    }
                },
            );
            slider_row(
                ui,
                "Ignore holds after typing",
                &mut cfg.touch_typing_guard_ms,
                0..=2000,
                " ms",
                accent,
            );
            if os.has_haptics() {
                toggle_row(
                    ui,
                    &mut cfg.haptics,
                    "Haptic feedback",
                    "A tick when the pie opens and when the mode switches. Trackpad only; a mouse is silent.",
                    accent,
                );
                ui.add_enabled_ui(cfg.haptics, |ui| {
                    toggle_row(
                        ui,
                        &mut cfg.haptic_hover,
                        "Tick when the slice changes",
                        "",
                        accent,
                    );
                });
            }
        });
    });
}

/// The note under the trackpad switch: one finger holds, a second one clicks, and which system
/// gesture may take that second finger instead.
fn extra_finger_hint(os: Os) -> &'static str {
    match os {
        Os::Linux => {
            "One finger holds, a second clicks. The system may call that a right-click; CursorPie \
             swallows it while the pie is open."
        }
        Os::Windows => {
            "One finger holds, a second clicks. Windows may call that a right-click; CursorPie \
             swallows it while the pie is open. If a tap and hold clicks the app under the pie, turn \
             off \"Tap twice and drag to multi-select\" in Settings > Bluetooth & devices > Touchpad."
        }
        Os::Mac => {
            "One finger holds, a second clicks. macOS may call that a secondary click; CursorPie \
             swallows it while the pie is open."
        }
    }
}

/// The notice as it will look, in the accent and the material that are set, on a strip of the
/// page color: what "Show what CursorPie does" switches on.
fn hud_preview(ui: &mut egui::Ui, cfg: &Config) {
    use crate::ui::hud::{self, HudKind, HudNote};
    ui.add_space(GAP);
    // the strip grows with the size, so the pill and its shadow always fit
    let height = (hud::scale_of(cfg) * 56.0 + 40.0).max(96.0);
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, CONTROL_R, pal().bg);
    let note = HudNote::new(HudKind::App, "Open app", "Safari");
    hud::paint_pill(&painter, rect.center(), &note, &hud::PillLook::at_rest(cfg));
}

/// A strip of colors with four slices of the configured glass over it, so a change of the
/// sliders shows at once: how much of the colors comes through and how thick the frost is.
fn glass_preview(ui: &mut egui::Ui, cfg: &Config) {
    use crate::ui::draw::{glass_colors, rgb};
    use crate::ui::pie::glass;
    let Some(g) = glass::Glass::of(cfg) else {
        return;
    };
    ui.add_space(GAP);
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 120.0), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, CONTROL_R, rgb([64, 120, 214]));
    // what the glass sits on: a few blobs of color and a sharp diagonal, so blur and clarity show
    for (dx, dy, r, c) in [
        (0.12, 0.2, 52.0, [255, 149, 0]),
        (0.35, 0.9, 44.0, [255, 45, 85]),
        (0.62, 0.15, 56.0, [52, 199, 89]),
        (0.88, 0.8, 50.0, [175, 82, 222]),
    ] {
        painter.circle_filled(
            egui::pos2(
                rect.min.x + rect.width() * dx,
                rect.min.y + rect.height() * dy,
            ),
            r,
            rgb(c),
        );
    }
    painter.line_segment(
        [rect.left_bottom(), rect.right_top()],
        egui::Stroke::new(3.0, Color32::WHITE),
    );
    let center = rect.center();
    let (inner, outer) = (22.0, 54.0);
    let step = std::f32::consts::TAU / 4.0;
    for i in 0..4 {
        let a0 = i as f32 * step + 0.04 - std::f32::consts::FRAC_PI_4 * 2.0;
        glass::slice(
            &painter,
            center,
            (inner, outer),
            (a0, a0 + step - 0.08),
            glass_colors().slice,
            if i == 1 { 1.0 } else { 0.0 },
            1.0,
            g,
            false,
        );
    }
    glass::core(&painter, center, inner * 0.86, 1.0, None, g);
    ui.add_space(GAP);
}

#[cfg(test)]
mod bridge_text_tests {
    use super::*;
    use crate::bridge::Status;

    #[test]
    fn the_status_line_says_what_is_connected_or_missing() {
        let mut st = Status {
            running: true,
            ..Status::default()
        };
        assert!(bridge_status_text(&st).contains("no supported browser"));
        st.registered = vec!["Chrome", "Firefox"];
        assert!(bridge_status_text(&st).contains("Ready for: Chrome, Firefox"));
        st.browsers = vec![("Firefox".into(), 1), ("Chrome".into(), 12)];
        assert_eq!(
            bridge_status_text(&st),
            "Connected: Firefox (1 tab), Chrome (12 tabs)."
        );
        st.playing = 2;
        st.chats = vec![("whatsapp".into(), Some(3)), ("telegram".into(), None)];
        assert_eq!(
            bridge_status_text(&st),
            "Connected: Firefox (1 tab), Chrome (12 tabs). Sound in 2 tabs. WhatsApp: 3 unread. Telegram is open."
        );
        st.outdated = vec!["Chrome".into()];
        assert!(bridge_status_text(&st).contains("extension in Chrome is from another version"));
        st.playing = 0;
        st.chats.clear();
        st.outdated.clear();
        st.browsers.clear();
        st.errors = vec!["Brave: permission denied".into()];
        assert!(bridge_status_text(&st).contains("permission denied"));
        assert_eq!(
            bridge_status_text(&Status::default()),
            "Not started yet: save the settings to start the link."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scan_says_what_it_found_in_the_words_of_the_os() {
        assert_eq!(
            scan_text(Os::Mac, None, false),
            None,
            "nothing before the first scan"
        );
        assert_eq!(
            scan_text(Os::Mac, Some(0), false).as_deref(),
            Some("Scan: no trackpad found.")
        );
        assert!(
            scan_text(Os::Windows, Some(0), true)
                .unwrap()
                .contains("precision touchpad")
        );
        assert!(
            scan_text(Os::Linux, Some(1), true)
                .unwrap()
                .contains("1 touchpad found")
        );
        assert!(
            scan_text(Os::Mac, Some(2), true)
                .unwrap()
                .contains("2 trackpads found")
        );
        assert!(
            scan_text(Os::Mac, Some(1), false)
                .unwrap()
                .contains("Turn the switch on")
        );
    }

    #[test]
    fn the_trackpad_panel_draws_in_every_state_and_says_what_is_going_on() {
        use crate::tap::TouchStatus::*;
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        for on in [false, true] {
            for st in [
                Off,
                Unavailable,
                NoDevice,
                NoPermission { devices: 1 },
                Ready {
                    devices: 1,
                    built_in: true,
                },
                Ready {
                    devices: 2,
                    built_in: false,
                },
            ] {
                let mut cfg = Config {
                    touch_trigger: on,
                    ..Config::default()
                };
                ctx.run_ui(Default::default(), |ui| {
                    trackpad_panel(ui, &mut cfg, Color32::GRAY, st);
                })
                .drop_without_applying_deltas();
            }
        }
        assert!(trackpad_status_text_for(Os::Mac, true, Unavailable).contains("not available"));
        assert!(
            trackpad_status_text_for(
                Os::Mac,
                true,
                Ready {
                    devices: 1,
                    built_in: true
                }
            )
            .contains("Built-in")
        );
        assert!(trackpad_status_text_for(Os::Mac, true, NoDevice).contains("No trackpad"));
        assert!(
            trackpad_status_text_for(
                Os::Mac,
                false,
                Ready {
                    devices: 1,
                    built_in: true
                }
            )
            .starts_with("Off")
        );
        // Windows says touchpad things, and has no haptics to talk about
        let win = |on, st| trackpad_status_text_for(Os::Windows, on, st);
        assert!(win(true, Unavailable).contains("Windows"));
        assert!(win(true, NoDevice).contains("precision touchpad"));
        let ready = win(
            true,
            Ready {
                devices: 1,
                built_in: false,
            },
        );
        assert!(ready.starts_with("Precision touchpad") && !ready.contains("Haptics"));
        assert!(win(false, Off).starts_with("Off"));
        // Linux: no haptics sentence, and a missing permission names the fix
        let lin = |on, st| trackpad_status_text_for(Os::Linux, on, st);
        let found = lin(
            true,
            Ready {
                devices: 1,
                built_in: false,
            },
        );
        assert!(found.contains("Touchpad found") && !found.contains("Haptics"));
        let denied = lin(true, NoPermission { devices: 1 });
        assert!(denied.contains("input") && denied.contains("usermod"));
        assert!(lin(false, Off).starts_with("Off"));
        assert!(!extra_finger_hint(Os::Linux).contains("macOS"));
        assert!(extra_finger_hint(Os::Windows).contains("Windows"));
        assert!(extra_finger_hint(Os::Mac).contains("macOS"));
        let labels: Vec<_> = [
            TouchClickAction::SwitchPie,
            TouchClickAction::PickHovered,
            TouchClickAction::Nothing,
        ]
        .map(touch_click_label)
        .to_vec();
        assert!(labels[0] != labels[1] && labels[1] != labels[2]);
    }

    #[test]
    fn every_desktop_choice_has_its_own_label() {
        let labels: Vec<String> = crate::platform::OS
            .desktop_modes()
            .iter()
            .map(|&m| desktop_mode_label(m))
            .collect();
        for (i, a) in labels.iter().enumerate() {
            assert!(!a.is_empty());
            assert!(!labels[..i].contains(a), "{a} is listed twice");
        }
        assert!(desktop_mode_label(DesktopMode::BringToCurrent).contains("experimental"));
    }

    #[test]
    fn the_new_window_hint_names_what_this_os_does() {
        let hint = new_window_hint();
        assert!(!hint.is_empty());
        assert_eq!(
            hint.contains("Cmd+N"),
            crate::platform::OS == crate::platform::Os::Mac
        );
    }
}
