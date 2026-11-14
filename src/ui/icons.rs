//! The icon texture cache shared by the pie and the Settings window.
//!
//! An icon string is either text / emoji (nothing to load), the path of an image file, or
//! `app:<bundle id>` (or `appcolor:<bundle id>`) for the real icon of an app (see [`appicon`]). Loading an image means reading
//! and scaling a file; loading an app icon means asking macOS for a multi-size TIFF and decoding
//! it. Both are far too slow to do on the frame that opens the pie, so decoded textures are kept
//! here:
//!
//! * The cache is **bounded** ([`CAPACITY`]) and evicts the least recently used entries, see
//!   [`Lru`].
//! * It **survives** opening and closing Settings. Only image files that changed on disk are
//!   reloaded ([`IconCache::revalidate`]), instead of throwing everything away every time.
//! * It is **warmed up** while the trigger is held, before the pie opens, see
//!   [`icons_to_warm`] and `Msg::Warm`. Only the first frame ([`icons_on_first_frame`]) is read on
//!   the UI thread; the app icons of submenus go to the worker thread.
//! * It is **trimmed** when the UI goes to sleep ([`IconCache::keep_only`]): what the next pie
//!   needs first ([`icons_to_keep_asleep`]) stays, so the first press after a long idle time finds
//!   its icons decoded instead of decoding them while the hold timer runs.
//! * Lists that show many app icons at once (the app picker) ask with
//!   [`IconCache::get_app_async`]: the system call and the decoding then run on a worker thread,
//!   so scrolling never waits for them (on macOS one icon is a multi-size TIFF of several MiB).

use crate::cache::Lru;
use crate::config::{Config, Item, ItemKind};
use crate::ui::appicon;
use crate::ui::draw::{icon_path, load_icon};
use eframe::egui;
use std::collections::HashSet;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// Most textures kept between frames. An icon is at most
/// [`ICON_MAX_PX`](crate::ui::draw::ICON_MAX_PX) pixels on its longest side (128 x 128 = 64 KiB),
/// so this is a few MiB at the very most.
pub const CAPACITY: usize = 96;

/// Most icon strings [`icons_to_keep_asleep`] names: the general pie, then the others. Each is a
/// texture of at most 64 KiB, so this is two MiB at the very most, and a few dozen KiB for the
/// icons of an ordinary pie.
pub const SLEEP_KEEP: usize = 32;

/// How long to wait before trying again to load an app icon that could not be loaded.
const APP_ICON_RETRY: Duration = Duration::from_secs(3);

/// Most decoded app icons turned into textures in one frame (an upload is cheap, but a long list
/// scrolled fast should not put them all into one frame).
const UPLOADS_PER_FRAME: usize = 8;

/// How many threads read app icons. On macOS one icon is a few MiB of TIFF and decoding it is the
/// cost, so one thread is enough (more would only pile up memory). On Windows and Linux the wait
/// is for the system instead: a scan of the running processes and the executable's file, or the
/// icon theme folders and, for an SVG, a `rsvg-convert` process. Two threads overlap those waits.
const WORKERS: usize = if matches!(crate::platform::OS, crate::platform::Os::Mac) {
    1
} else {
    2
};

/// What an [`IconCache`] holds, see [`IconCache::stats`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// Entries, including the remembered failures.
    pub entries: usize,
    /// Entries whose icon could not be loaded.
    pub failed: usize,
    /// Bytes of the textures (4 per pixel).
    pub texture_bytes: usize,
    /// App icons asked of the worker and not answered yet.
    pub pending: usize,
}

/// Where an app icon asked for with [`IconCache::get_app_async`] stands.
pub enum IconState {
    /// Ready to paint.
    Ready(egui::TextureHandle),
    /// Being read on the worker thread; the window is repainted when it arrives.
    Pending,
    /// The icon cannot be loaded (not an app icon, or the system has none).
    Missing,
}

/// The requests of the worker thread. The newest request is served first: a list that is scrolled
/// fast asks for rows that are gone before the worker gets to them, and the rows on screen now
/// must not wait behind those.
#[derive(Default)]
struct QueueState {
    stack: Vec<String>,
    closed: bool,
}

struct Queue {
    state: Mutex<QueueState>,
    wake: Condvar,
}

impl Queue {
    fn lock(&self) -> std::sync::MutexGuard<'_, QueueState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The loop of one worker thread: takes the newest request, reads and decodes the icon, hands it
/// back and asks for a repaint. Ends when the cache is dropped.
fn serve(q: &Queue, tx: &mpsc::Sender<(String, Option<egui::ColorImage>)>, ctx: &egui::Context) {
    loop {
        let key = {
            let mut s = q.lock();
            loop {
                if s.closed {
                    return;
                }
                if let Some(k) = s.stack.pop() {
                    break k;
                }
                s = q.wake.wait(s).unwrap_or_else(|e| e.into_inner());
            }
        };
        // a panic while decoding one odd icon must not end the thread
        let img = std::panic::catch_unwind(AssertUnwindSafe(|| appicon::load_string(&key)))
            .unwrap_or(None);
        if tx.send((key, img)).is_err() {
            return; // the cache is gone
        }
        ctx.request_repaint();
    }
}

/// The threads that read and decode app icons, see [`IconCache::get_app_async`].
struct Worker {
    queue: Arc<Queue>,
    done: Receiver<(String, Option<egui::ColorImage>)>,
}

impl Worker {
    /// Starts the threads ([`WORKERS`]). None if the system refuses to start even one.
    fn start(ctx: &egui::Context) -> Option<Self> {
        let queue = Arc::new(Queue {
            state: Mutex::new(QueueState::default()),
            wake: Condvar::new(),
        });
        let (tx, done) = mpsc::channel();
        let mut started = 0;
        for n in 0..WORKERS {
            let (q, tx, ctx) = (Arc::clone(&queue), tx.clone(), ctx.clone());
            let spawned = std::thread::Builder::new()
                .name(format!("cursorpie-icons-{n}"))
                .spawn(move || serve(&q, &tx, &ctx));
            if spawned.is_ok() {
                started += 1;
            }
        }
        (started > 0).then_some(Self { queue, done })
    }

    fn request(&self, key: &str) {
        self.queue.lock().stack.push(key.to_string());
        self.queue.wake.notify_one();
    }

    /// Forgets the requests that were not started yet, and the answers not taken yet.
    fn cancel_all(&self) {
        self.queue.lock().stack.clear();
        while self.done.try_recv().is_ok() {}
    }

    fn close(&self) {
        self.queue.lock().closed = true;
        self.queue.wake.notify_all();
    }
}

/// What identifies a version of an image file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStamp {
    modified: Option<SystemTime>,
    len: u64,
}

impl FileStamp {
    /// The stamp of the file at `path`, or None if it cannot be read.
    pub fn of(path: &std::path::Path) -> Option<Self> {
        let m = std::fs::metadata(path).ok()?;
        Some(Self {
            modified: m.modified().ok(),
            len: m.len(),
        })
    }
}

/// One cached icon, or the memory of a failed attempt.
struct Entry {
    /// None = the icon could not be loaded.
    tex: Option<egui::TextureHandle>,
    /// For image files: where it came from and what it looked like, to notice changes.
    file: Option<(PathBuf, Option<FileStamp>)>,
    /// When loading failed (an app icon is tried again after [`APP_ICON_RETRY`]).
    failed_at: Option<Instant>,
}

/// Bounded texture cache for icon strings. See the module docs.
#[derive(Default)]
pub struct IconCache {
    lru: Lru<String, Entry>,
    /// Reads app icons off the UI thread (started by the first [`IconCache::get_app_async`]).
    worker: Option<Worker>,
    /// App icons asked of the worker whose answer has not been taken yet.
    pending: HashSet<String>,
}

impl Drop for IconCache {
    fn drop(&mut self) {
        if let Some(w) = &self.worker {
            w.close();
        }
    }
}

impl IconCache {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// What the cache holds, for the diagnostic log. Walks the entries, so it is only called while
    /// the log is on (see [`IconCache::log_stats`]).
    pub fn stats(&self) -> CacheStats {
        let mut s = CacheStats {
            pending: self.pending.len(),
            ..CacheStats::default()
        };
        for e in self.lru.values() {
            s.entries += 1;
            match &e.tex {
                Some(t) => {
                    let [w, h] = t.size();
                    s.texture_bytes += w * h * 4;
                }
                None => s.failed += 1,
            }
        }
        s
    }

    /// Writes a `mem.icon_cache` line (how many icons, how many failed, the bytes of the textures,
    /// how many are still being read) when the memory log is on. `stage` says when.
    pub fn log_stats(&self, stage: &'static str) {
        if !crate::ui::memlog::enabled() {
            return;
        }
        let s = self.stats();
        diag::log!(
            "mem",
            "icon_cache",
            stage,
            entries = s.entries,
            failed = s.failed,
            texture_kib = s.texture_bytes / 1024,
            pending = s.pending
        );
    }

    /// Number of cached icons (including remembered failures).
    pub fn len(&self) -> usize {
        self.lru.len()
    }

    /// Whether nothing is cached.
    pub fn is_empty(&self) -> bool {
        self.lru.is_empty()
    }

    /// Call once per frame: trims the cache to [`CAPACITY`] (never touching icons used by the last frame).
    pub fn new_frame(&mut self) {
        self.lru.new_frame(CAPACITY);
    }

    /// Drops every texture and gives the table's memory back.
    pub fn clear(&mut self) {
        self.lru.clear();
        if let Some(w) = &self.worker {
            w.cancel_all();
        }
        // an answer that is still on its way is ignored: it is not in `pending` any more
        self.pending.clear();
    }

    /// Drops every texture except the ones named in `keep` (icon strings, see
    /// [`icons_to_keep_asleep`]), which stay decoded. Used when the UI sleeps: freeing the whole
    /// cache gains a few KiB but makes the first pie after a long idle time decode its icons
    /// while the hold timer runs, and on macOS one app icon is a multi-size TIFF.
    pub fn keep_only(&mut self, keep: &[String]) {
        self.lru.retain(|k, _| keep.iter().any(|s| s == k));
        if let Some(w) = &self.worker {
            w.cancel_all();
        }
        // an answer that is still on its way is ignored: it is not in `pending` any more
        self.pending.clear();
    }

    /// Forgets what is stale: image files that changed, vanished or failed to load. Cheap (one
    /// `stat` per cached file), so it runs whenever Settings opens or closes.
    pub fn revalidate(&mut self) {
        self.lru.retain(|_, e| match &e.file {
            Some((path, stamp)) => e.tex.is_some() && FileStamp::of(path) == *stamp,
            None => true,
        });
    }

    /// Whether `icon` is already in the cache (a remembered failure counts). Costs a lookup and
    /// never decodes anything, so a screen can ask before it spends time on a load.
    pub fn contains(&self, icon: &str) -> bool {
        if appicon::is_app_icon(icon) {
            return self.lru.contains(icon);
        }
        icon_path(icon).is_some_and(|p| self.lru.contains(&*p.to_string_lossy()))
    }

    /// Like [`IconCache::get`], but an app icon that is not cached yet is read and decoded on a
    /// worker thread instead of on this one: this returns [`IconState::Pending`] at once and the
    /// window is repainted when the icon is there. For lists with many icons that are scrolled.
    /// Other icon strings are answered as `get` does.
    pub fn get_app_async(&mut self, ctx: &egui::Context, icon: &str) -> IconState {
        if !appicon::is_app_icon(icon) {
            return self
                .get(ctx, icon)
                .map_or(IconState::Missing, IconState::Ready);
        }
        self.receive(ctx);
        if let Some(e) = self.lru.get(icon) {
            if let Some(tex) = &e.tex {
                return IconState::Ready(tex.clone());
            }
            // a failure is tried again after a pause, not on every frame
            if e.failed_at.is_some_and(|at| at.elapsed() < APP_ICON_RETRY) {
                return IconState::Missing;
            }
        }
        if self.pending.contains(icon) {
            return IconState::Pending;
        }
        if self.worker.is_none() {
            self.worker = Worker::start(ctx);
        }
        match &self.worker {
            Some(w) => {
                w.request(icon);
                self.pending.insert(icon.to_string());
                IconState::Pending
            }
            // no thread to be had: load it here, as `get` does
            None => self
                .get(ctx, icon)
                .map_or(IconState::Missing, IconState::Ready),
        }
    }

    /// Turns the icons the worker has finished into textures (a few per frame).
    fn receive(&mut self, ctx: &egui::Context) {
        let Some(w) = &self.worker else {
            return;
        };
        let mut ready = Vec::new();
        while ready.len() < UPLOADS_PER_FRAME {
            match w.done.try_recv() {
                Ok(r) => ready.push(r),
                Err(_) => break,
            }
        }
        if ready.len() == UPLOADS_PER_FRAME {
            ctx.request_repaint(); // there may be more waiting
        }
        for (key, img) in ready {
            if !self.pending.remove(&key) {
                continue; // the cache was cleared since: the answer is stale
            }
            let tex = img.map(|img| ctx.load_texture(&key, img, egui::TextureOptions::LINEAR));
            let failed_at = tex.is_none().then(Instant::now);
            self.lru.insert(
                key,
                Entry {
                    tex,
                    file: None,
                    failed_at,
                },
            );
        }
    }

    /// The texture for an icon string. None for text icons and for images that cannot be loaded.
    pub fn get(&mut self, ctx: &egui::Context, icon: &str) -> Option<egui::TextureHandle> {
        // "app:<bundle id>" / "appcolor:<bundle id>" / "appblack:<bundle id>" = the real icon of
        // an app, asked from the system (see `appicon`)
        if appicon::is_app_icon(icon) {
            if let Some(e) = self.lru.get(icon) {
                // a failure is not final: macOS may simply not have had the icon ready. Try again
                // after a pause, but not on every frame (decoding is not free).
                let retry =
                    e.tex.is_none() && e.failed_at.is_none_or(|at| at.elapsed() >= APP_ICON_RETRY);
                if !retry {
                    return e.tex.clone();
                }
            }
            let tex = appicon::load_string(icon)
                .map(|img| ctx.load_texture(icon, img, egui::TextureOptions::LINEAR));
            // No time is noted for a failure here. This is the warm-up, which runs while the
            // system is still cold from a long idle time, so a miss is often only that; noting
            // it would make `get_app_async` (the pie's own lookup) refuse for `APP_ICON_RETRY`
            // and the slice would show its text for seconds. Without it the pie hands the icon
            // to the worker thread at once, and the worker notes a failure that is real.
            self.lru.insert(
                icon.to_string(),
                Entry {
                    tex: tex.clone(),
                    file: None,
                    failed_at: None,
                },
            );
            return tex;
        }

        let path = icon_path(icon)?;
        let key = path.to_string_lossy().to_string();
        if let Some(e) = self.lru.get(&key) {
            return e.tex.clone();
        }
        let stamp = FileStamp::of(&path);
        let tex = load_icon(ctx, &path, &key);
        self.lru.insert(
            key,
            Entry {
                tex: tex.clone(),
                file: Some((path, stamp)),
                failed_at: None,
            },
        );
        tex
    }
}

/// The icon strings to keep decoded while the UI sleeps: the pie of the general profile first (it
/// is the one a press opens almost always), then those of the other profiles, up to
/// [`SLEEP_KEEP`] strings. See [`IconCache::keep_only`].
pub fn icons_to_keep_asleep(c: &Config) -> Vec<String> {
    let general = c.global_profile();
    let others = (0..c.profiles.len()).filter(|&i| i != general);
    let mut out: Vec<String> = Vec::new();
    for profile in std::iter::once(general).chain(others) {
        for icon in icons_to_warm(c, profile, None) {
            if out.len() >= SLEEP_KEEP {
                return out;
            }
            if !out.contains(&icon) {
                out.push(icon);
            }
        }
    }
    out
}

/// The icon strings the very first frame of the pie of `profile` paints: the icon of the active
/// mode, the app icon of the center and the top ring. Submenus are not in it. This is the part of
/// [`icons_to_warm`] that is read on the UI thread while the hold timer runs; the rest (the icons
/// of submenus, which are on screen only after the pointer stays on a slice) is read on the worker
/// thread, so a press with many app icons no longer reads them all one after another in the frame
/// that has to open the pie. At most [`CAPACITY`] strings.
pub fn icons_on_first_frame(c: &Config, profile: usize, target: Option<&str>) -> Vec<String> {
    let Some(p) = c.profiles.get(profile) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    let mut add = |s: &str| {
        if !s.is_empty() && out.len() < CAPACITY && !out.iter().any(|o| o == s) {
            out.push(s.to_string());
        }
    };
    if let Some(i) = p.active_item() {
        add(&i.icon);
    }
    if c.center_app_icon
        && let Some(b) = appicon::bundle_for(&p.apps, target)
    {
        add(&appicon::icon_string(c.app_icon_style, b));
    }
    for it in &p.items {
        add(&it.icon);
    }
    out
}

/// The icon strings a pie of `profile` needs on its first frames: its top ring, the submenus at
/// every depth (shallowest first), the center icon and the app icon of the targeted app. Text icons are included on
/// purpose: [`IconCache::get`] returns at once for them. At most [`CAPACITY`] strings.
pub fn icons_to_warm(c: &Config, profile: usize, target: Option<&str>) -> Vec<String> {
    let Some(p) = c.profiles.get(profile) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    let mut add = |s: &str| {
        if !s.is_empty() && out.len() < CAPACITY && !out.iter().any(|o| o == s) {
            out.push(s.to_string());
        }
    };
    if let Some(i) = p.active_item() {
        add(&i.icon);
    }
    if c.center_app_icon
        && let Some(b) = appicon::bundle_for(&p.apps, target)
    {
        add(&appicon::icon_string(c.app_icon_style, b));
    }
    // level by level: the top ring first, then the submenus of that ring, then the submenus of
    // those, and so on. Shallow icons come first because they are on screen first, and if the
    // bound is reached it is the deepest ones that wait for the worker thread.
    let mut level: Vec<&Item> = p.items.iter().collect();
    while !level.is_empty() {
        let mut next: Vec<&Item> = Vec::new();
        for it in level {
            add(&it.icon);
            if let ItemKind::Submenu(children) = &it.kind {
                next.extend(children.iter());
            }
        }
        level = next;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A unique scratch directory under the system temp dir.
    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cursorpie-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_png(path: &std::path::Path, rgba: [u8; 4], size: u32) {
        let mut img = image::RgbaImage::new(size, size);
        for p in img.pixels_mut() {
            *p = image::Rgba(rgba);
        }
        img.save(path).unwrap();
    }

    #[test]
    fn an_app_icon_is_loaded_off_the_ui_thread() {
        let ctx = egui::Context::default();
        let mut c = IconCache::new();
        let key = "appcolor:com.cursorpie.no-such-app";
        assert!(
            matches!(c.get_app_async(&ctx, key), IconState::Pending),
            "the first ask must not wait for the system"
        );
        let started = Instant::now();
        // Only a bound against a hang. For an app that does not exist, Windows scans the Start
        // menu shortcuts and the program folders (once, then the miss is remembered), which on a
        // busy machine or a CI runner takes well over ten seconds.
        let patience = Duration::from_secs(120);
        loop {
            match c.get_app_async(&ctx, key) {
                IconState::Pending if started.elapsed() < patience => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                IconState::Pending => panic!("the worker never answered"),
                IconState::Missing => break,
                IconState::Ready(_) => panic!("there is no such app"),
            }
        }
        assert_eq!(
            c.len(),
            1,
            "the failure is remembered, not asked again per frame"
        );
        assert!(matches!(c.get_app_async(&ctx, "<>"), IconState::Missing));
    }

    #[test]
    fn text_icons_need_no_texture() {
        let ctx = egui::Context::default();
        let mut c = IconCache::new();
        assert!(c.get(&ctx, "<>").is_none());
        assert!(c.is_empty(), "text icons are not cached");
    }

    #[test]
    fn a_file_icon_is_loaded_once_and_kept() {
        let dir = scratch("once");
        let file = dir.join("a.png");
        write_png(&file, [255, 0, 0, 255], 16);
        let ctx = egui::Context::default();
        let mut c = IconCache::new();
        let s = file.to_string_lossy().to_string();
        let a = c.get(&ctx, &s).expect("loads");
        let b = c.get(&ctx, &s).expect("cached");
        assert_eq!(a.id(), b.id(), "same texture, not reloaded");
        assert_eq!(c.len(), 1);
        c.revalidate();
        assert_eq!(
            c.len(),
            1,
            "an unchanged file stays cached across revalidate"
        );
    }

    #[test]
    fn revalidate_drops_changed_and_missing_files() {
        let dir = scratch("changed");
        let (a, b) = (dir.join("a.png"), dir.join("b.png"));
        write_png(&a, [255, 0, 0, 255], 16);
        write_png(&b, [0, 255, 0, 255], 16);
        let ctx = egui::Context::default();
        let mut c = IconCache::new();
        let (sa, sb) = (
            a.to_string_lossy().to_string(),
            b.to_string_lossy().to_string(),
        );
        let old = c.get(&ctx, &sa).unwrap().id();
        let _ = c.get(&ctx, &sb).unwrap();
        // a changes size (so the stamp differs even on a coarse clock), b disappears
        write_png(&a, [0, 0, 255, 255], 32);
        std::fs::remove_file(&b).unwrap();
        c.revalidate();
        assert_eq!(c.len(), 0);
        assert_ne!(c.get(&ctx, &sa).unwrap().id(), old, "reloaded from disk");
        assert!(c.get(&ctx, &sb).is_none());
        c.revalidate();
        assert_eq!(
            c.len(),
            1,
            "a failed load is forgotten so it can be retried"
        );
    }

    #[test]
    fn contains_tells_loaded_from_not_loaded_without_loading() {
        let dir = scratch("contains");
        let file = dir.join("a.png");
        write_png(&file, [0, 255, 0, 255], 8);
        let ctx = egui::Context::default();
        let mut c = IconCache::new();
        let s = file.to_string_lossy().to_string();
        assert!(!c.contains(&s));
        assert!(c.is_empty(), "asking does not load");
        let _ = c.get(&ctx, &s);
        assert!(c.contains(&s));
        assert!(!c.contains("<>"), "text icons are never cached");
    }

    #[test]
    fn a_broken_file_is_remembered_as_failed() {
        let dir = scratch("broken");
        let file = dir.join("x.png");
        std::fs::File::create(&file)
            .unwrap()
            .write_all(b"not a png")
            .unwrap();
        let ctx = egui::Context::default();
        let mut c = IconCache::new();
        let s = file.to_string_lossy().to_string();
        assert!(c.get(&ctx, &s).is_none());
        assert_eq!(
            c.len(),
            1,
            "the failure is cached: no decode attempt per frame"
        );
    }

    #[test]
    fn the_cache_is_bounded_but_never_thrashes_within_a_frame() {
        let dir = scratch("bound");
        let ctx = egui::Context::default();
        let mut c = IconCache::new();
        let files: Vec<String> = (0..CAPACITY + 10)
            .map(|i| {
                let f = dir.join(format!("{i}.png"));
                write_png(&f, [i as u8, 0, 0, 255], 4);
                f.to_string_lossy().to_string()
            })
            .collect();
        c.new_frame();
        for f in &files {
            let _ = c.get(&ctx, f);
        }
        c.new_frame(); // everything was used in the last frame: nothing is evicted
        assert_eq!(c.len(), CAPACITY + 10);
        c.new_frame(); // nothing used in between: trimmed to the capacity
        assert_eq!(c.len(), CAPACITY);
        c.clear();
        assert!(c.is_empty());
    }

    #[test]
    fn keep_only_drops_everything_but_the_named_icons() {
        let dir = scratch("keep");
        let ctx = egui::Context::default();
        let mut c = IconCache::new();
        let files: Vec<String> = (0..3)
            .map(|i| {
                let f = dir.join(format!("{i}.png"));
                write_png(&f, [i as u8, 0, 0, 255], 4);
                f.to_string_lossy().to_string()
            })
            .collect();
        for f in &files {
            let _ = c.get(&ctx, f);
        }
        assert_eq!(c.len(), 3);
        c.keep_only(&files[..1]);
        assert_eq!(c.len(), 1);
        assert!(c.contains(&files[0]), "the kept icon is still decoded");
        assert!(!c.contains(&files[2]));
        c.keep_only(&[]);
        assert!(c.is_empty());
    }

    #[test]
    fn the_icons_kept_asleep_start_with_the_general_pie() {
        use crate::config::{Action, Config, Item, Profile};
        let profile = |name: &str, apps: Vec<String>, icon: &str| Profile {
            name: name.into(),
            apps,
            items: vec![Item::run("X", icon, Action::Nothing)],
            active: vec![],
        };
        let c = Config {
            profiles: vec![
                profile("App", vec!["com.example.App".into()], "~/app.png"),
                profile("General", vec![], "~/general.png"),
            ],
            ..Default::default()
        };
        let keep = icons_to_keep_asleep(&c);
        let at = |icon: &str| keep.iter().position(|s| s == icon);
        assert!(at("~/general.png").is_some() && at("~/app.png").is_some());
        assert!(
            at("~/general.png") < at("~/app.png"),
            "the general pie comes first"
        );
    }

    #[test]
    fn the_icons_kept_asleep_are_bounded_and_without_duplicates() {
        use crate::config::{Action, Config, Item, Profile};
        let many: Vec<Item> = (0..SLEEP_KEEP + 8)
            .map(|i| Item::run("X", &format!("~/{i}.png"), Action::Nothing))
            .collect();
        let c = Config {
            profiles: vec![Profile {
                name: "General".into(),
                apps: vec![],
                items: many,
                active: vec![],
            }],
            ..Default::default()
        };
        let keep = icons_to_keep_asleep(&c);
        assert_eq!(keep.len(), SLEEP_KEEP);
        let mut unique = keep.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), keep.len());
        let none = Config {
            profiles: vec![],
            ..Default::default()
        };
        assert!(icons_to_keep_asleep(&none).is_empty());
    }

    #[test]
    fn warm_list_covers_ring_submenu_center_and_app_icon() {
        use crate::config::{Action, Config, Item, Profile};
        let c = Config {
            center_app_icon: true,
            profiles: vec![Profile {
                name: "App".into(),
                apps: vec!["com.example.App".into()],
                items: vec![
                    Item::run("A", "~/a.png", Action::Nothing),
                    Item::submenu("S", "S", vec![Item::run("B", "~/b.png", Action::Nothing)]),
                    Item::run("Dup", "~/a.png", Action::Nothing),
                ],
                active: vec![],
            }],
            ..Default::default()
        };
        let w = icons_to_warm(&c, 0, Some("com.example.App"));
        assert!(w.contains(&"app:com.example.App".to_string()));
        let colored = Config {
            app_icon_style: crate::config::AppIconStyle::Color,
            ..c.clone()
        };
        assert!(
            icons_to_warm(&colored, 0, Some("com.example.App"))
                .contains(&"appcolor:com.example.App".to_string()),
            "the warm-up asks for the style that will be drawn"
        );
        assert!(w.contains(&"~/a.png".to_string()) && w.contains(&"~/b.png".to_string()));
        assert_eq!(
            w.iter().filter(|s| *s == "~/a.png").count(),
            1,
            "no duplicates"
        );
        assert!(icons_to_warm(&c, 7, None).is_empty(), "unknown profile");
    }

    #[test]
    fn the_first_frame_list_is_the_ring_the_center_and_no_submenu() {
        use crate::config::{Action, Config, Item, Profile};
        let c = Config {
            center_app_icon: true,
            profiles: vec![Profile {
                name: "App".into(),
                apps: vec!["com.example.App".into()],
                items: vec![
                    Item::run("A", "~/a.png", Action::Nothing),
                    Item::submenu("S", "S", vec![Item::run("B", "~/b.png", Action::Nothing)]),
                    Item::run("Dup", "~/a.png", Action::Nothing),
                ],
                active: vec![],
            }],
            ..Default::default()
        };
        let first = icons_on_first_frame(&c, 0, Some("com.example.App"));
        let has = |icon: &str| first.iter().any(|s| s == icon);
        assert!(has("app:com.example.App"), "the center");
        assert!(has("~/a.png") && has("S"), "the ring");
        assert!(!has("~/b.png"), "a submenu waits for the worker");
        assert_eq!(first.iter().filter(|s| *s == "~/a.png").count(), 1);
        let all = icons_to_warm(&c, 0, Some("com.example.App"));
        assert!(first.iter().all(|i| all.contains(i)), "in the warm list");
        assert!(icons_on_first_frame(&c, 7, None).is_empty(), "no profile");
    }

    #[test]
    fn warm_list_reaches_submenus_of_submenus_shallowest_first() {
        use crate::config::{Action, Config, Item, Profile};
        let leaf = |icon: &str| Item::run("L", icon, Action::Nothing);
        let deep = Item::submenu("Deep", "~/deep.png", vec![leaf("~/c.png")]);
        let mid = Item::submenu("Mid", "~/mid.png", vec![leaf("~/b.png"), deep]);
        let c = Config {
            profiles: vec![Profile {
                name: "General".into(),
                apps: vec![],
                items: vec![leaf("~/a.png"), mid],
                active: vec![],
            }],
            ..Default::default()
        };
        let w = icons_to_warm(&c, 0, None);
        let at = |icon: &str| w.iter().position(|s| s == icon);
        for icon in ["~/a.png", "~/mid.png", "~/b.png", "~/deep.png", "~/c.png"] {
            assert!(at(icon).is_some(), "{icon} is warmed");
        }
        assert!(
            at("~/mid.png") < at("~/b.png"),
            "the ring before its submenu"
        );
        assert!(at("~/b.png") < at("~/c.png"), "a level before the next one");
    }
}
