//! # Extending CursorPie: recipes
//!
//! Each recipe lists the files to touch, in the order that keeps the project compiling. File
//! paths are relative to `src/`. Read [`architecture`](super::architecture) first.
//!
//! * [Add an action kind](#add-an-action-kind)
//! * [Add a setting](#add-a-setting)
//! * [Add a ready-made slice](#add-a-ready-made-slice)
//! * [Add a slice kind](#add-a-slice-kind)
//! * [Add a Settings tab](#add-a-settings-tab)
//! * [Add icons to the picker](#add-icons-to-the-picker)
//! * [Keep the Settings look consistent](#keep-the-settings-look-consistent)
//! * [Add a message to the UI thread](#add-a-message-to-the-ui-thread)
//! * [Add a menu bar item](#add-a-menu-bar-item)
//! * [Cache something in the UI](#cache-something-in-the-ui)
//! * [Show a long list or read a user file](#show-a-long-list-or-read-a-user-file)
//! * [Keep the profile rules](#keep-the-profile-rules)
//! * [Make a change undoable](#make-a-change-undoable)
//! * [Change gesture behavior](#change-gesture-behavior)
//! * [Rebrand a fork](#rebrand-a-fork)
//!
//! ## Add an action kind
//!
//! Example: "Open a file in a given app".
//!
//! 1. `config/model.rs`: add a variant to [`Action`](crate::config::Action). Variants are
//!    serialized by name, so choose it carefully: it is part of the file format.
//! 2. `actions.rs`: handle it in `execute`. If it sends keystrokes, also return `true` for it in
//!    `sends_keys` so the target app is brought to the front first.
//! 3. `config/tree.rs`: if it can run arbitrary code, make
//!    [`has_commands`](crate::config::has_commands) return `true` for it (that drives the
//!    "this config runs shell commands" warning after an import).
//! 4. `ui/settings/action_editor.rs`: add it to `KINDS`, `action_index`, `make_action` and the
//!    input row. The `Sequence` kind shows how to add a kind the editor cannot edit yet.
//! 5. Add a unit test next to the code (see `actions::tests`).
//!
//! ## Add a setting
//!
//! 1. `config/model.rs`: add a field to [`Config`](crate::config::Config) with a doc comment.
//!    The struct is `#[serde(default)]`, so older files keep loading.
//! 2. `config/defaults.rs`: set its value in `impl Default for Config`.
//! 3. `config/sanitize.rs`: clamp it if it has a range, and add a case to `config/mod.rs` tests.
//! 4. `ui/settings/tabs/behavior.rs`: add the widget (it edits `self.edit`, the *copy* of the
//!    config, which *Save* applies).
//! 5. Use it where needed with `let v = cfg().field;`. **Copy the value out and drop the guard**
//!    instead of holding the lock across slow work.
//! 6. Document it in the field table of `docs/user/configuration.md`, and in `docs/user/README.md` if users see it.
//!
//! ## Add a ready-made slice
//!
//! `config/defaults.rs` has three lists shown in the "Add a slice" panel:
//! [`presets`](crate::config::presets) (side-button modes), [`quick_actions`](crate::config::quick_actions)
//! and [`blank_items`](crate::config::blank_items). Append an `Item::mode(..)` or `Item::run(..)`.
//! Use [`Shortcut::ch`](crate::keys::Shortcut::ch) for letters and punctuation so the shortcut
//! follows the user's keyboard layout, and the constants in [`keys::key`](crate::keys::key) for
//! layout-independent keys. The first eight presets are indexed by position in
//! `Config::default()`, so append new presets at the end.
//!
//! ## Add a slice kind
//!
//! Larger than it looks, because a slice kind touches every layer:
//!
//! 1. `config/model.rs`: new [`ItemKind`](crate::config::ItemKind) variant.
//! 2. `config/tree.rs` and `config/sanitize.rs`: make `has_commands`, `first_mode` and
//!    `clean_items` aware of it (the compiler points at the exhaustive matches).
//! 3. `tap/resolve.rs`: decide in `select` what releasing over it does.
//! 4. `ui/pie/view.rs` and `ui/pie/mod.rs`: how it is drawn; `ui/settings/pie_designer.rs`
//!    for the designer.
//! 5. `ui/settings/inspector.rs`: the kind control (segmented) and the editor body.
//!
//! ## Add a Settings tab
//!
//! 1. `ui/settings/state.rs`: add a variant to `Tab`.
//! 2. `ui/settings/tabs/<name>.rs`: `impl App { pub(in crate::ui::settings) fn <name>_tab(&mut self, ui: &mut egui::Ui) }`
//!    and register the file in `tabs/mod.rs`.
//! 3. `ui/settings/window.rs`: add it to the `sections` array of the sidebar and to the `match` that draws it.
//!
//! Layout rule used throughout the Settings UI: compute widths from the width the caller gives
//! you instead of `available_width()` inside nested layouts, otherwise content can spill out of
//! the card.
//!
//! ## Add a page to the welcome window
//!
//! 1. `ui/settings/welcome.rs`: raise `PAGES` and add a `match` arm in `App::welcome_ui` that calls a
//!    new `fn <name>_page(ui: &mut egui::Ui, ..)`. Pages draw inside the body rectangle; the
//!    footer (dots, Back, Next, Get started) and the top bar (Skip) come from `welcome_ui`.
//! 2. A text that differs between macOS and Windows is a pure function of
//!    [`Os`](crate::platform::Os) next to the others, with a test for both values.
//! 3. State that must survive between pages goes into `Welcome`; anything that must happen when the
//!    window closes belongs in `App::finish_welcome` (`ui/app/windows.rs`), which runs once the
//!    close animation is over for every way of leaving (Skip, Esc, the window's close button).
//! 4. The window shows once per user. To show it again while testing, delete `welcome_shown` from
//!    the app support directory (next to `config.json`), or use "Welcome guide..." in the tray menu.
//!
//! ## Change the installers
//!
//! * macOS: `scripts/bundle.sh` makes the `.app`, `scripts/make-dmg.sh` packs it. The bundle
//!   identifier must stay equal to `about::BUNDLE_ID`.
//! * Windows: `installer/cursorpie.iss` (Inno Setup 6.3 or later) is built by
//!   `scripts/package-windows.ps1`. It installs into the folder the program uses when it installs
//!   itself and writes the same `Run` value, so keep both in step with
//!   `platform/windows/install.rs` and `login_item.rs`. Never change its `AppId`: Windows uses it to
//!   update an existing install. The program recognizes a Setup install by `unins000.exe` in its
//!   folder (`platform::windows::util::installed_by_setup`) and then does not install itself.
//!
//! ## Add icons to the picker
//!
//! Append `ic("glyph", "Name", "search keywords")` to a category in
//! `ui/settings/icon_catalog.rs`. Glyphs are drawn with egui's bundled fonts; if one renders as
//! an empty box, remove it. Users can also drop images into
//! `~/Library/Application Support/CursorPie/icons/` (the folder next to `config.json` on every system).
//! The picker lists at most `MAX_USER_ICONS` files of at most 2 MB and says how many it left out
//! (`icon_catalog::scan_user_icons`); an image the user *chooses* with "Image..." goes through
//! [`iconfit::fit`](crate::ui::iconfit::fit), which shrinks it or refuses it with a message.
//!
//! ## Keep the Settings look consistent
//!
//! The Settings window is modern flat: light, roomy, with large rounded cards, pill chips and one
//! icon set (see `docs/developer/UI_DESIGN.md`). When you add or change a widget:
//!
//! 1. Take colors from `pal()` (`pal().bg`, `.card`, `.idle`, `.hot`, ...: Settings is light or
//!    dark, so never keep a color in a `const`), and radius, spacing and height from
//!    `ui/settings/theme.rs` (`CONTROL_R`, `PANEL_R`, `GAP`, `CONTROL_H`). Do not write `Color32::from_rgb(..)`, a
//!    corner radius or a pixel height in an editor. Text on the accent color uses
//!    `on_accent(accent)`, because the user picks the accent and it can be light.
//! 2. Every button, chip, input and list row is `CONTROL_H` (36) high, and spacing is `GAP` (12) or
//!    `SECTION_GAP` (24). Do not add your own spacing numbers.
//! 3. A tab is a stack of `panel(ui, "Title", ..)` cards. Use the widgets in
//!    `ui/settings/widgets.rs` (`button`, `segmented`, `icon_tabs`, `select`, `menu_popup`, `tile`, `switch`, `slider_row`, `toggle_row`,
//!    `choice_row`, `text_field`, `badge`, `info_row`) instead of egui's defaults (`radio_value`,
//!    `checkbox`, `Slider`, `TextEdit::singleline`): they share one shape and one height.
//! 4. Navigation lives in the sidebar and the Undo / Redo / Cancel / Save buttons in the top bar
//!    (`window.rs`), Quit at the bottom of the sidebar and messages in the footer. A new tab is one
//!    more entry in the `sections` array and `section_info`, and one more arm in the body `match`.
//!    Keep a page to one column of `panel`s: the body is capped at 720 pt wide.
//! 5. Text is `TEXT` (primary) or `MUTED` (secondary, for labels and hints). An option that is off
//!    keeps `TEXT`: dim text reads as disabled. Set colors explicitly rather than with `.strong()`.
//! 6. **Icons are the icon set, never letters or emoji.** Name an icon in `ui/symbols.rs` and draw it
//!    with `fonts::paint_icon` or `widgets::icon_button`; give an icon-only button a tooltip. A
//!    button with words may carry an icon beside them.
//! 7. **A popup is a sheet or a popover.** Use `widgets::sheet` for something that needs the whole
//!    window's attention (a picker, a long form, a confirmation such as the question
//!    before quitting, see `ui/settings/quit_dialog.rs`) and `widgets::popover` for a few choices under the
//!    button that opened it. A list of choices is `widgets::select`, never `egui::ComboBox` (its list ignores the look). Do not open a bare `egui::Area` or `Window` for
//!    anything else.
//! 8. Titles use the semibold family (`fonts::title`); everything else is the normal font in
//!    sentence case.
//! 9. Anything with an edge that you draw by hand goes through `ui::draw` (`ring_slice`,
//!    `arc_points`, `snap_rect`); a raw `egui::Mesh` is not anti-aliased.
//!
//! The pie itself stays radial: its slices are picked by angle, and that hit-testing is shared
//! with the event tap (see `geometry.rs`).
//!
//! ## Add a message to the UI thread
//!
//! 1. `msg.rs`: add a variant to [`Msg`](crate::msg::Msg).
//! 2. `ui/app/messages.rs`: handle it in `handle_msg`.
//! 3. Send it with [`Notifier::send`](crate::msg::Notifier::send) (from the tap) or
//!    `tx.send(..)` followed by `ctx.request_repaint()` (from a plain thread).
//!
//! For work that blocks (file choosers, `mdls`) spawn a thread; `ui/settings/picker.rs` has the
//! helper for file choosers.
//!
//! ## Add a menu bar item
//!
//! `ui/tray.rs` builds the menu (`MenuItem::with_id("my_id", ..)`); `ui/app/windows.rs`
//! `handle_tray` receives the id as `Msg::Tray(id)`. "Welcome guide..." is an example: its id
//! `welcome` calls `open_welcome`.
//!
//! ## Cache something in the UI
//!
//! Read [Idle sleep and caches](super::architecture#idle-sleep-and-caches) first.
//!
//! 1. Store it in a [`Lru`](crate::cache::Lru) (or add to [`IconCache`](crate::ui::icons::IconCache)
//!    if it is an icon). Call `new_frame(cap)` once per frame so it stays bounded.
//! 2. Clear it in `App::enter_sleep` (`ui/app/lifecycle.rs`), so it does not outlive an idle period.
//! 3. If the first use would be slow, fill it ahead of time in `App::warm`, which runs when the
//!    trigger is pressed. Never do slow work on the frame that opens the pie.
//! 4. If the source can change on disk, store a stamp ([`FileStamp`](crate::ui::icons::FileStamp))
//!    and drop stale entries in a `revalidate` step instead of clearing everything.
//! 5. Put the pure part (eviction, decisions) in a type with no egui or macOS types and unit test it.
//!
//! ## Show a long list or read a user file
//!
//! 1. A list or grid that can pass a screenful goes in `egui::ScrollArea::show_rows`, so only the visible
//!    rows run. Set the row gap on a `ui.scope` around the scroll area: `show_rows` takes it from there.
//!    Build the tiles from indexes into the data, and clone a string only for a tile that is drawn.
//! 2. Texture loads for those rows are limited per frame (`LOADS_PER_FRAME` in `icon_picker.rs`): ask
//!    [`IconCache::contains`](crate::ui::icons::IconCache::contains) first, draw a placeholder for the rest and
//!    call `request_repaint` so they load on the next frame.
//! 3. Limit a file where it enters: read an image's size with
//!    `open_reader` and refuse it before decoding, and read text with
//!    [`read_text_capped`](crate::config::read_text_capped). Do slow work on a worker thread
//!    (`spawn_file_picker` runs its closure on one) and report the result with `Msg::Notice`.
//! 4. Unit test the decision (the size limit, the file name, the message) with a scratch directory, like the
//!    tests in `ui/iconfit.rs`.
//!
//! ## Keep the profile rules
//!
//! Exactly one profile is general (no apps); every other profile has at least one app. Create
//! profiles with [`Profile::for_app`](crate::config::Profile::for_app) (or `share::add_profile` for
//! imports), never with an empty `apps` list, and offer no way to delete the general profile or the
//! last app of another one. [`Config::sanitize`](crate::config::Config::sanitize) enforces the rule
//! as a last line of defense.
//!
//! ## Make a change undoable
//!
//! Anything stored in the config copy `App::edit` is undoable with no extra code: the history
//! snapshots the whole value every frame. Just edit `self.edit` (Settings widgets already do).
//!
//! * State kept outside the config, such as the selection in `Ed`, is not in the history. After an
//!   undo it can point at something that no longer exists, so read it defensively or extend
//!   `Ed::fit_to`.
//! * If an action replaces `edit` wholesale (import, restore), do not reset the history: the
//!   replacement is a step the user can undo. Call `Ed::reset_nav` (or `fit_to`) afterwards.
//! * A new field of `Config` (or of the types inside it) must keep `PartialEq`: the history compares
//!   values to notice changes.
//!
//! ## Change gesture behavior
//!
//! Gestures live in `tap/`. Re-read the [state machine](super::architecture#the-gesture-state-machine)
//! and the [callback rules](super::architecture#rules-for-the-event-tap-callback) first.
//!
//! * A new state goes into `tap/gesture.rs`; each transition is one function in `tap/trigger.rs`,
//!   dispatched from `trigger::handle`.
//! * Anything that waits (a timer) is a thread that re-checks the gesture `id` after waking up.
//! * Return `CallbackResult::Drop` only for events the program owns; when in doubt use `Keep`.
//! * A new **gesture source** (the trackpad is the model, `tap/touch/`) does not get a state
//!   machine of its own: a pure recognizer turns native frames into events, and a small bridge calls
//!   the `touch_*` entry points of `tap/trigger.rs`, which act only on gestures that carry its
//!   `Source`. Give the native layer one file, load system libraries at run time and fail soft.
//! * Pure decisions (which profile, which pie follows the side buttons) belong in code that does
//!   not touch macOS APIs, so they can be unit tested; see [`side::resolve`](crate::side::resolve).
//!
//! ## Rebrand a fork
//!
//! Edit `about.rs` (name, author, links, bundle id), `Cargo.toml`, `LICENSE`, and keep
//! `BUNDLE_ID` identical in `about.rs` and `scripts/bundle.sh`.
//!
//! ### Replace the logo
//!
//! The logo is one function, `draw_logo` in `ui/settings/widgets.rs`. The top bar of the Settings
//! window (`ui/settings/window.rs`, a 24 pt box) and the About tab (`ui/settings/tabs/about.rs`, an
//! 88 pt box) both call it, so keep its signature and change only its body.
//!
//! 1. Keep the SVG as the source file (`assets/logo.svg`) and render a square PNG from it, because
//!    the app has no SVG renderer and adding one would break the "no new dependencies" rule:
//!
//!    ```sh
//!    rsvg-convert -w 256 -h 256 assets/logo.svg -o assets/logo.png
//!    ```
//!
//! 2. Embed the PNG with `include_bytes!`, decode it with the `image` crate (its `png` feature is
//!    already on) and upload it once as an egui texture. A texture is a cache: keep it in one place
//!    and drop it in `App::enter_sleep` (see [Cache something in the UI](#cache-something-in-the-ui)).
//! 3. Draw it with `Painter::image`. A white logo on a transparent background can be tinted with
//!    the accent color, which keeps the old behavior of following the accent.
//! 4. Add a unit test that decodes the embedded PNG, so a broken asset fails `cargo test`.
//!
//! The menu bar / tray icon (`ui/tray.rs`) and the icon of the `.app` bundle are separate and are
//! not changed by this.
