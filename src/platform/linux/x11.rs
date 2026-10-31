//! The X11 session: one process-wide connection to the X server, shared by every thread.
//!
//! Xlib is thread-safe once `XInitThreads` ran before the connection was opened, so the tap thread
//! (events), the UI thread (cursor, displays) and the action threads (key and mouse injection) use
//! the same `Display`. Requests that change grabs are serialized by [`GRABS`]; X errors in them are
//! caught instead of letting Xlib's default handler end the process.

use super::desktop_entry::{self, Index};
use super::xlib::*;
use super::{scale, toplevels, xdg};
use crate::geometry::Point;
use crate::keys::Shortcut;
use crate::platform::{ScreenRect, keymap};
use std::ffi::CString;
use std::os::raw::{c_int, c_long, c_uchar, c_uint, c_ulong, c_void};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// The app id of CursorPie's own windows (never reported as "the app under the pointer").
pub const OWN_APP_ID: &str = "cursorpie";

/// Atoms looked up once.
struct Atoms {
    active_window: Atom,
    client_list_stacking: Atom,
    wm_state: Atom,
    wm_state_hidden: Atom,
    wm_desktop: Atom,
    current_desktop: Atom,
    wm_window_type: Atom,
    wm_pid: Atom,
    resource_manager: Atom,
}

/// The connection. Never freed: it lives as long as the process.
pub struct Conn {
    pub dpy: *mut Display,
    pub root: Window,
    pub xi_opcode: c_int,
    atoms: Atoms,
    /// Global display scale (X11 has no per-monitor scale).
    pub scale: f64,
}

// SAFETY: the Display was opened after `XInitThreads`, which makes Xlib lock internally; the other
// fields are plain values.
unsafe impl Send for Conn {}
unsafe impl Sync for Conn {}

static CONN: AtomicPtr<Conn> = AtomicPtr::new(ptr::null_mut());
static OPEN_LOCK: Mutex<()> = Mutex::new(());

/// The shared connection, opened on first use. `None` if there is no X server to talk to (it is
/// tried again on the next call).
pub fn conn() -> Option<&'static Conn> {
    let p = CONN.load(Ordering::Acquire);
    if !p.is_null() {
        // SAFETY: set once from a leaked Box, never freed.
        return Some(unsafe { &*p });
    }
    let _guard = crate::config::lock(&OPEN_LOCK);
    let p = CONN.load(Ordering::Acquire);
    if !p.is_null() {
        // SAFETY: as above.
        return Some(unsafe { &*p });
    }
    let raw = Box::into_raw(Box::new(Conn::open()?));
    CONN.store(raw, Ordering::Release);
    // SAFETY: `raw` comes from a Box that is never freed.
    Some(unsafe { &*raw })
}

// ----------------------------------------------------------------------------- error trapping

static X_ERROR: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn on_x_error(_dpy: *mut Display, _event: *mut XErrorEvent) -> c_int {
    X_ERROR.store(true, Ordering::SeqCst);
    0
}

// ------------------------------------------------------------------------------- grab state

/// The pointer buttons that are currently grabbed, and the master pointer's device id (learned from
/// events; 2 is the usual "Virtual core pointer").
struct GrabState {
    buttons: Vec<c_int>,
    master: c_int,
}

static GRABS: Mutex<GrabState> = Mutex::new(GrabState {
    buttons: Vec::new(),
    master: 2,
});

fn set_bit(bits: &mut [u8], event: c_int) {
    bits[(event >> 3) as usize] |= 1 << (event & 7);
}

/// An app index shared by every thread, rebuilt now and then in the background.
static INDEX: RwLock<Option<Arc<Index>>> = RwLock::new(None);

/// The installed applications, once the first scan has finished.
pub fn index() -> Option<Arc<Index>> {
    INDEX.read().ok().and_then(|g| g.clone())
}

/// Starts the thread that scans the desktop entries (at once, then every five minutes). Window
/// queries never wait for it: until it finishes they report the raw window class.
pub fn warm_index() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("cursorpie-apps".into())
        .spawn(|| {
            loop {
                let home = crate::config::home();
                let dirs = xdg::application_dirs(
                    &home,
                    std::env::var("XDG_DATA_HOME").ok().as_deref(),
                    std::env::var("XDG_DATA_DIRS").ok().as_deref(),
                );
                let fresh = Arc::new(desktop_entry::scan(&dirs));
                if let Ok(mut w) = INDEX.write() {
                    *w = Some(fresh);
                }
                std::thread::sleep(Duration::from_secs(300));
            }
        });
    if spawned.is_err() {
        STARTED.store(false, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------------- the Conn

impl Conn {
    fn open() -> Option<Conn> {
        // SAFETY: plain Xlib calls on a display this function owns until it returns.
        unsafe {
            // before the first display is opened, so that this one may be shared between threads
            XInitThreads();
            let dpy = XOpenDisplay(ptr::null());
            if dpy.is_null() {
                return None;
            }
            let root = XDefaultRootWindow(dpy);
            let (mut opcode, mut first_event, mut first_error) = (0, 0, 0);
            let name = CString::new("XInputExtension").ok()?;
            if XQueryExtension(
                dpy,
                name.as_ptr(),
                &mut opcode,
                &mut first_event,
                &mut first_error,
            ) == FALSE
            {
                XCloseDisplay(dpy);
                return None;
            }
            // XInput 2.0 is enough for everything used here; ask for 2.2 and accept what is given
            let (mut major, mut minor) = (2, 2);
            if XIQueryVersion(dpy, &mut major, &mut minor) != 0 {
                XCloseDisplay(dpy);
                return None;
            }
            let atom = |n: &str| -> Atom {
                match CString::new(n) {
                    Ok(c) => XInternAtom(dpy, c.as_ptr(), FALSE),
                    Err(_) => 0,
                }
            };
            let atoms = Atoms {
                active_window: atom("_NET_ACTIVE_WINDOW"),
                client_list_stacking: atom("_NET_CLIENT_LIST_STACKING"),
                wm_state: atom("_NET_WM_STATE"),
                wm_state_hidden: atom("_NET_WM_STATE_HIDDEN"),
                wm_desktop: atom("_NET_WM_DESKTOP"),
                current_desktop: atom("_NET_CURRENT_DESKTOP"),
                wm_window_type: atom("_NET_WM_WINDOW_TYPE"),
                wm_pid: atom("_NET_WM_PID"),
                resource_manager: atom("RESOURCE_MANAGER"),
            };
            let mut conn = Conn {
                dpy,
                root,
                xi_opcode: opcode,
                atoms,
                scale: 1.0,
            };
            let xft = conn
                .prop8(root, conn.atoms.resource_manager, XA_STRING, 16384)
                .and_then(|b| String::from_utf8(b).ok())
                .and_then(|t| scale::parse_xft_dpi(&t));
            conn.scale =
                scale::scale_from(std::env::var("WINIT_X11_SCALE_FACTOR").ok().as_deref(), xft);
            diag::log!(
                "x11",
                "scale",
                scale = conn.scale,
                xft_scale_input = xft.unwrap_or(0.0),
                env_override = std::env::var("WINIT_X11_SCALE_FACTOR").unwrap_or_default()
            );
            Some(conn)
        }
    }

    // ------------------------------------------------------------------ properties

    /// A format-8 property of `w` as bytes.
    fn prop8(&self, w: Window, prop: Atom, ty: Atom, max_longs: c_long) -> Option<Vec<u8>> {
        // SAFETY: the returned buffer is copied and freed before returning.
        unsafe {
            let (mut actual_type, mut format) = (0, 0);
            let (mut n, mut after) = (0, 0);
            let mut data: *mut c_uchar = ptr::null_mut();
            let status = XGetWindowProperty(
                self.dpy,
                w,
                prop,
                0,
                max_longs,
                FALSE,
                ty,
                &mut actual_type,
                &mut format,
                &mut n,
                &mut after,
                &mut data,
            );
            if status != 0 || data.is_null() {
                return None;
            }
            let out = if format == 8 && n > 0 {
                std::slice::from_raw_parts(data, n as usize).to_vec()
            } else {
                Vec::new()
            };
            XFree(data as *mut c_void);
            if out.is_empty() { None } else { Some(out) }
        }
    }

    /// A format-32 property of `w` as unsigned values (Xlib hands them out as C longs).
    fn prop32(&self, w: Window, prop: Atom, ty: Atom) -> Vec<c_ulong> {
        // SAFETY: the returned buffer is copied and freed before returning.
        unsafe {
            let (mut actual_type, mut format) = (0, 0);
            let (mut n, mut after) = (0, 0);
            let mut data: *mut c_uchar = ptr::null_mut();
            let status = XGetWindowProperty(
                self.dpy,
                w,
                prop,
                0,
                4096,
                FALSE,
                ty,
                &mut actual_type,
                &mut format,
                &mut n,
                &mut after,
                &mut data,
            );
            if status != 0 || data.is_null() {
                return Vec::new();
            }
            let out = if format == 32 && n > 0 {
                std::slice::from_raw_parts(data as *const c_ulong, n as usize).to_vec()
            } else {
                Vec::new()
            };
            XFree(data as *mut c_void);
            out
        }
    }

    fn atom_name(&self, atom: Atom) -> String {
        // SAFETY: the name is copied and freed.
        unsafe {
            let p = XGetAtomName(self.dpy, atom);
            if p.is_null() {
                return String::new();
            }
            let s = std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned();
            XFree(p as *mut c_void);
            s
        }
    }

    // ------------------------------------------------------------------ pointer and screens

    /// The pointer position in X pixels (physical).
    pub fn pointer_px(&self) -> Option<(i32, i32)> {
        // SAFETY: out-parameters are valid locals.
        unsafe {
            let (mut root_ret, mut child) = (0, 0);
            let (mut rx, mut ry, mut wx, mut wy) = (0, 0, 0, 0);
            let mut mask: c_uint = 0;
            let ok = XQueryPointer(
                self.dpy,
                self.root,
                &mut root_ret,
                &mut child,
                &mut rx,
                &mut ry,
                &mut wx,
                &mut wy,
                &mut mask,
            );
            if ok == FALSE { None } else { Some((rx, ry)) }
        }
    }

    /// The pointer in logical points (X pixels divided by the display scale).
    pub fn pointer(&self) -> Option<(f64, f64)> {
        let (x, y) = self.pointer_px()?;
        Some((x as f64 / self.scale, y as f64 / self.scale))
    }

    /// The monitors in logical points.
    pub fn monitors(&self) -> Vec<ScreenRect> {
        // SAFETY: the monitor list is copied and freed.
        let mut out = unsafe {
            let mut n: c_int = 0;
            let p = XRRGetMonitors(self.dpy, self.root, TRUE, &mut n);
            let mut v = Vec::new();
            if !p.is_null() && n > 0 {
                for m in std::slice::from_raw_parts(p, n as usize) {
                    v.push(ScreenRect {
                        x: m.x as f64 / self.scale,
                        y: m.y as f64 / self.scale,
                        w: m.width as f64 / self.scale,
                        h: m.height as f64 / self.scale,
                    });
                }
            }
            if !p.is_null() {
                XRRFreeMonitors(p);
            }
            v
        };
        if out.is_empty() {
            // no RandR monitors: the whole screen
            if let Some((_, _, w, h)) = self.geometry_of(self.root) {
                out.push(ScreenRect {
                    x: 0.0,
                    y: 0.0,
                    w: w / self.scale,
                    h: h / self.scale,
                });
            }
        }
        out
    }

    /// `(x, y, width, height)` of a window in root coordinates.
    fn geometry_of(&self, w: Window) -> Option<(f64, f64, f64, f64)> {
        // SAFETY: out-parameters are valid locals.
        unsafe {
            let mut root_ret = 0;
            let (mut x, mut y) = (0, 0);
            let (mut width, mut height, mut border, mut depth): (c_uint, c_uint, c_uint, c_uint) =
                (0, 0, 0, 0);
            if XGetGeometry(
                self.dpy,
                w,
                &mut root_ret,
                &mut x,
                &mut y,
                &mut width,
                &mut height,
                &mut border,
                &mut depth,
            ) == 0
            {
                return None;
            }
            let (mut rx, mut ry) = (0, 0);
            let mut child = 0;
            if XTranslateCoordinates(self.dpy, w, self.root, 0, 0, &mut rx, &mut ry, &mut child)
                == FALSE
            {
                return None;
            }
            Some((rx as f64, ry as f64, width as f64, height as f64))
        }
    }

    // ------------------------------------------------------------------ windows and apps

    fn active_window(&self) -> Option<Window> {
        self.prop32(self.root, self.atoms.active_window, XA_WINDOW)
            .first()
            .copied()
            .filter(|w| *w != 0)
    }

    /// Client windows, bottom-most first (as `_NET_CLIENT_LIST_STACKING` gives them).
    fn stacking(&self) -> Vec<Window> {
        self.prop32(self.root, self.atoms.client_list_stacking, XA_WINDOW)
    }

    fn current_desktop(&self) -> Option<u32> {
        self.prop32(self.root, self.atoms.current_desktop, XA_CARDINAL)
            .first()
            .map(|d| *d as u32)
    }

    /// The workspace of a window; `None` if it is on all of them.
    fn window_desktop(&self, w: Window) -> Option<u32> {
        let d = *self.prop32(w, self.atoms.wm_desktop, XA_CARDINAL).first()?;
        let d = d as u32;
        if d == 0xFFFF_FFFF { None } else { Some(d) }
    }

    fn is_hidden(&self, w: Window) -> bool {
        self.prop32(w, self.atoms.wm_state, XA_ATOM)
            .contains(&self.atoms.wm_state_hidden)
    }

    fn is_ours(&self, w: Window) -> bool {
        self.prop32(w, self.atoms.wm_pid, XA_CARDINAL)
            .first()
            .is_some_and(|p| *p as u32 == std::process::id())
    }

    fn type_names(&self, w: Window) -> Vec<String> {
        self.prop32(w, self.atoms.wm_window_type, XA_ATOM)
            .into_iter()
            .map(|a| self.atom_name(a))
            .collect()
    }

    /// The canonical app id of a window: its class mapped through the desktop entries, else the
    /// lower-case class itself.
    fn app_id(&self, w: Window) -> Option<String> {
        let bytes = self.prop8(w, XA_WM_CLASS, XA_STRING, 256)?;
        let (instance, class) = toplevels::parse_wm_class(&bytes)?;
        if let Some(idx) = index()
            && let Some(id) = idx.canonical(&[class.as_str(), instance.as_str()])
        {
            return Some(id);
        }
        let id = desktop_entry::id_key(&class);
        if id.is_empty() { None } else { Some(id) }
    }

    /// The app in front.
    pub fn frontmost(&self) -> Option<String> {
        self.app_id(self.active_window()?)
    }

    /// Every real window, top-most first.
    fn toplevels(&self) -> Vec<toplevels::Toplevel> {
        let mut stack = self.stacking();
        stack.reverse();
        let mut out = Vec::new();
        for w in stack {
            let Some(app) = self.app_id(w) else { continue };
            let Some((x, y, width, height)) = self.geometry_of(w) else {
                continue;
            };
            let names = self.type_names(w);
            let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
            let skip =
                toplevels::is_skipped_type(&name_refs) || app == OWN_APP_ID || self.is_ours(w);
            out.push(toplevels::Toplevel {
                app,
                x,
                y,
                w: width,
                h: height,
                hidden: self.is_hidden(w),
                skip,
                desktop: self.window_desktop(w),
            });
        }
        out
    }

    /// The app of the window under the logical point `p`.
    pub fn app_at(&self, p: Point) -> Option<String> {
        let list = self.toplevels();
        let i = toplevels::hit_test(
            &list,
            p.x * self.scale,
            p.y * self.scale,
            self.current_desktop(),
        )?;
        Some(list[i].app.clone())
    }

    /// Asks the window manager to raise and focus the top-most window of `app`.
    pub fn activate(&self, app: &str) {
        self.raise(app);
    }

    /// Like [`Conn::activate`], and says whether `app` has a window at all. The window may be on
    /// another workspace: `_NET_ACTIVE_WINDOW` makes the usual window managers (Mutter, KWin, Xfwm,
    /// Openbox, i3...) go to that workspace, which is how "Open app" follows an app to its desktop.
    pub fn raise(&self, app: &str) -> bool {
        let key = desktop_entry::id_key(app);
        if self.frontmost().as_deref() == Some(key.as_str()) {
            return true;
        }
        let mut stack = self.stacking();
        stack.reverse();
        let Some(target) = stack
            .into_iter()
            .find(|w| self.app_id(*w).as_deref() == Some(key.as_str()))
        else {
            return false;
        };
        let mut ev = XEvent::zeroed();
        // SAFETY: every member of the union starts at offset 0 and the event is zeroed; the cast
        // views it as a client message, which `XSendEvent` copies.
        unsafe {
            let m = &mut *(&mut ev as *mut XEvent as *mut XClientMessageEvent);
            m.type_ = CLIENT_MESSAGE;
            m.window = target;
            m.message_type = self.atoms.active_window;
            m.format = 32;
            m.data = [2, CURRENT_TIME as c_long, 0, 0, 0]; // 2 = the request comes from a pager
            XSendEvent(
                self.dpy,
                self.root,
                FALSE,
                SUBSTRUCTURE_REDIRECT_MASK | SUBSTRUCTURE_NOTIFY_MASK,
                &mut ev,
            );
            XFlush(self.dpy);
        }
        true
    }

    // ------------------------------------------------------------------ injection

    fn fake_key(&self, keycode: u8, press: bool) {
        // SAFETY: plain request.
        unsafe {
            XTestFakeKeyEvent(self.dpy, keycode as c_uint, press as Bool, 0);
        }
    }

    /// Presses the modifiers, the key, then releases everything in reverse order.
    pub fn press_shortcut(&self, sc: Shortcut) {
        let Some(key) = keymap::x_keycode_for(sc.key) else {
            return;
        };
        let mods: Vec<u8> = keymap::LINUX_MODIFIER_KEYS
            .iter()
            .filter(|(bit, _)| sc.mods & *bit != 0)
            .filter_map(|(_, code)| u8::try_from(*code + 8).ok())
            .collect();
        for m in &mods {
            self.fake_key(*m, true);
        }
        self.fake_key(key, true);
        self.fake_key(key, false);
        for m in mods.iter().rev() {
            self.fake_key(*m, false);
        }
        // SAFETY: plain request.
        unsafe {
            XFlush(self.dpy);
        }
    }

    /// Presses or releases the X button `xb` as if the user did.
    ///
    /// A press would trigger CursorPie's own passive grab again, so the grabs (and an active grab
    /// that a physical press started) are lifted around it and put back after the server has
    /// handled the event. A release cannot start a grab and is sent as it is.
    pub fn inject_button(&self, xb: c_int, press: bool) {
        let g = crate::config::lock(&GRABS);
        if press {
            self.trapped(|| {
                for b in &g.buttons {
                    self.ungrab_button(*b);
                }
                // SAFETY: plain request; a stale id is an X error, which `trapped` absorbs.
                unsafe {
                    XIUngrabDevice(self.dpy, g.master, CURRENT_TIME);
                }
            });
        }
        // SAFETY: plain requests.
        unsafe {
            XTestFakeButtonEvent(self.dpy, xb as c_uint, press as Bool, 0);
            XSync(self.dpy, FALSE);
        }
        if press {
            self.trapped(|| {
                for b in &g.buttons {
                    self.grab_button(*b);
                }
            });
        }
    }

    // ------------------------------------------------------------------ grabs and events

    /// Runs `f`, absorbs the X errors it causes, and says whether there were any.
    fn trapped<R>(&self, f: impl FnOnce() -> R) -> (R, bool) {
        X_ERROR.store(false, Ordering::SeqCst);
        // SAFETY: the handler only sets a flag. The previous handler is put back.
        let previous = unsafe { XSetErrorHandler(Some(on_x_error)) };
        let r = f();
        // SAFETY: as above.
        unsafe {
            XSync(self.dpy, FALSE);
            XSetErrorHandler(previous);
        }
        (r, X_ERROR.load(Ordering::SeqCst))
    }

    fn grab_button(&self, button: c_int) -> bool {
        let mut bits = [0u8; XI_MASK_LEN];
        set_bit(&mut bits, XI_BUTTON_PRESS);
        set_bit(&mut bits, XI_BUTTON_RELEASE);
        set_bit(&mut bits, XI_MOTION);
        let mut mask = XIEventMask {
            deviceid: XI_ALL_MASTER_DEVICES,
            mask_len: XI_MASK_LEN as c_int,
            mask: bits.as_mut_ptr(),
        };
        let mut mods = [XIGrabModifiers {
            modifiers: XI_ANY_MODIFIER,
            status: 0,
        }];
        // SAFETY: the mask and modifier arrays outlive the call.
        let failed = unsafe {
            XIGrabButton(
                self.dpy,
                XI_ALL_MASTER_DEVICES,
                button,
                self.root,
                NONE,
                GRAB_MODE_ASYNC,
                GRAB_MODE_ASYNC,
                FALSE,
                &mut mask,
                1,
                mods.as_mut_ptr(),
            )
        };
        failed == 0 && mods[0].status == 0
    }

    fn ungrab_button(&self, button: c_int) {
        let mut mods = [XIGrabModifiers {
            modifiers: XI_ANY_MODIFIER,
            status: 0,
        }];
        // SAFETY: the modifier array outlives the call.
        unsafe {
            XIUngrabButton(
                self.dpy,
                XI_ALL_MASTER_DEVICES,
                button,
                self.root,
                1,
                mods.as_mut_ptr(),
            );
        }
    }

    /// Makes `wanted` the set of grabbed buttons. `true` if every one of them could be grabbed (a
    /// window manager or another tool may hold a grab on the same button).
    pub fn set_grabs(&self, wanted: &[c_int]) -> bool {
        let mut g = crate::config::lock(&GRABS);
        let (installed, _) = self.trapped(|| {
            for b in g.buttons.clone() {
                if !wanted.contains(&b) {
                    self.ungrab_button(b);
                }
            }
            let mut installed = Vec::new();
            for b in wanted {
                if g.buttons.contains(b) || self.grab_button(*b) {
                    installed.push(*b);
                }
            }
            installed
        });
        let all = installed.len() == wanted.len();
        g.buttons = installed;
        all
    }

    /// Lifts every button grab (used while CursorPie is paused: events then reach the apps untouched).
    pub fn release_grabs(&self) {
        let g = crate::config::lock(&GRABS);
        self.trapped(|| {
            for b in &g.buttons {
                self.ungrab_button(*b);
            }
            // SAFETY: plain request; a stale id is absorbed by `trapped`.
            unsafe {
                XIUngrabDevice(self.dpy, g.master, CURRENT_TIME);
            }
        });
    }

    /// Puts the grabs of [`Conn::set_grabs`] back after [`Conn::release_grabs`].
    pub fn restore_grabs(&self) -> bool {
        let g = crate::config::lock(&GRABS);
        let (ok, _) = self.trapped(|| {
            let mut all = true;
            for b in &g.buttons {
                all &= self.grab_button(*b);
            }
            all
        });
        ok
    }

    /// The buttons that are grabbed right now.
    pub fn grabbed(&self) -> Vec<c_int> {
        crate::config::lock(&GRABS).buttons.clone()
    }

    /// Remembers the master pointer's device id (needed to end an active grab).
    pub fn set_master(&self, id: c_int) {
        crate::config::lock(&GRABS).master = id;
    }

    /// Asks for the release of any button as a raw event: it is also reported while a grab is
    /// lifted, which is how the end of a handed-over drag is noticed.
    pub fn select_raw_release(&self) {
        let mut bits = [0u8; XI_MASK_LEN];
        set_bit(&mut bits, XI_RAW_BUTTON_RELEASE);
        let mut mask = XIEventMask {
            deviceid: XI_ALL_MASTER_DEVICES,
            mask_len: XI_MASK_LEN as c_int,
            mask: bits.as_mut_ptr(),
        };
        // SAFETY: the mask outlives the call.
        unsafe {
            XISelectEvents(self.dpy, self.root, &mut mask, 1);
            XFlush(self.dpy);
        }
    }

    /// Takes the keyboard so the next key press can be recorded as a shortcut.
    pub fn grab_keyboard(&self) -> bool {
        // SAFETY: plain request.
        unsafe {
            XGrabKeyboard(
                self.dpy,
                self.root,
                FALSE,
                GRAB_MODE_ASYNC,
                GRAB_MODE_ASYNC,
                CURRENT_TIME,
            ) == GRAB_SUCCESS
        }
    }

    pub fn ungrab_keyboard(&self) {
        // SAFETY: plain requests.
        unsafe {
            XUngrabKeyboard(self.dpy, CURRENT_TIME);
            XFlush(self.dpy);
        }
    }
}
