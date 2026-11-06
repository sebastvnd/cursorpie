//! `cursorpie --touchpad-debug [seconds]`: why does the touchpad do nothing?
//!
//! Walks the chain the trackpad hold depends on and says which link is broken, then shows the
//! fingers live and what the recognizer makes of them. It does not need the app to run (stop a
//! running CursorPie first: two copies would both react), starts no window and opens nothing but
//! the touchpad nodes for reading.
//!
//! 1. the session (X11 or Wayland) and the setting `touch_trigger` (saved?);
//! 2. the user's groups (`input`), the usual reason for "no permission";
//! 3. every device of the kernel that has anything to do with touch, and why it was taken or not;
//! 4. for the taken ones: can the node be opened, what size is the surface;
//! 5. live: every frame change, and the events of the recognizer with the settings of the config.
//!
//! The finger positions are printed to this terminal only, on request; the app never logs them.

use super::{
    ABS_MT_POSITION_X, ABS_MT_POSITION_Y, EVENT_SIZE, Geometry, MAX_SLOTS, Parser, REPEAT_MS,
    decode, device_id, read_axis,
};
use crate::config::cfg;
use crate::platform::linux::evdev_io::{POLLIN, PollFd, open_node, poll_fds, raw_fd};
use crate::platform::linux::inputdev;
use crate::tap::touch::params_from_config;
use crate::tap::touch::recognizer::{Finger, Frame, Recognizer, TouchEvent};
use std::time::{Duration, Instant};

/// How long the live part runs when no number is given.
const DEFAULT_SECS: u64 = 20;
/// The longest live part.
const MAX_SECS: u64 = 600;

/// What the report found, for [`hints`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// The session is Wayland and no X display is set.
    pub wayland_only: bool,
    /// `touch_trigger` in the saved config.
    pub switch_on: bool,
    /// Touchpads the kernel lists and CursorPie takes.
    pub pads: usize,
    /// Of those, the nodes that could be opened.
    pub open: usize,
    /// Of those, the nodes refused for lack of permission.
    pub denied: usize,
    /// The user is named in the `input` group of `/etc/group` but this process is not in it yet.
    pub group_not_active: bool,
    /// Any frame arrived during the live part.
    pub frames: bool,
    /// The most fingers seen at once.
    pub max_fingers: usize,
    /// The recognizer reported a hold that started.
    pub armed: bool,
    /// The recognizer reported the pie opening.
    pub opened: bool,
    /// The live part ran at all (there was something to read).
    pub live: bool,
}

/// What to do about it, most important first. Empty when everything worked.
pub fn hints(f: &Facts) -> Vec<&'static str> {
    let mut v = Vec::new();
    if f.wayland_only {
        v.push(
            "This is a Wayland session without an X display: CursorPie's mouse layer needs X11 \
             (log in with the \"on Xorg\" session), so the app does nothing here even though the \
             touchpad can be read.",
        );
    }
    if !f.switch_on {
        v.push(
            "Settings > Behavior > Trackpad: turn on \"Open the pie with a trackpad hold\" and press \
             Save. While it is off nothing is read.",
        );
    }
    if f.pads == 0 {
        v.push(
            "No device was taken as a touchpad. Read the list above: each device says why it was \
             left out. A touchpad that the kernel exposes as a plain mouse cannot be used.",
        );
    } else if f.denied > 0 && f.open == 0 {
        if f.group_not_active {
            v.push(
                "You are in the `input` group but this session started before that: log out and log in again.",
            );
        } else {
            v.push(
                "No permission to read the touchpad: sudo usermod -aG input $USER, then log out and in.",
            );
        }
    }
    if f.live && !f.frames {
        v.push(
            "The touchpad was open but sent nothing while you touched it. Is it the right device \
             (see the list), and is it enabled in your desktop's settings?",
        );
    } else if f.frames && f.max_fingers == 0 {
        v.push("Events arrived but no finger was ever down: this node may be a different interface of the device.");
    } else if f.frames && !f.armed {
        v.push(
            "Fingers were seen but no hold started. Hold still: more than \"How still the finger \
             must stay\" (mm) of movement, a key pressed within the typing guard, a palm-sized \
             contact or the border of the pad all prevent it.",
        );
    } else if f.armed && !f.opened {
        v.push(
            "A hold started but was lifted or moved before the hold time (Behavior > hold time) ran out.",
        );
    } else if f.opened {
        v.push("The touchpad and the recognizer work. If the pie still does not open in the app, run it with CURSORPIE_DEBUG=1 and look at the `touch` and `tap` lines (docs/DEBUGGING.md).");
    }
    v
}

/// The names of the groups with these ids, from the text of `/etc/group`.
pub fn group_names(group_file: &str, gids: &[u32]) -> Vec<String> {
    group_file
        .lines()
        .filter_map(|l| {
            let mut p = l.split(':');
            let name = p.next()?;
            let gid: u32 = p.nth(1)?.parse().ok()?;
            gids.contains(&gid).then(|| name.to_string())
        })
        .collect()
}

/// Whether `/etc/group` lists `user` as a member of `group`.
pub fn listed_in_group(group_file: &str, group: &str, user: &str) -> bool {
    group_file.lines().any(|l| {
        let mut p = l.split(':');
        p.next() == Some(group) && p.nth(2).is_some_and(|m| m.split(',').any(|u| u == user))
    })
}

/// The group ids of this process (`Groups:` of `/proc/self/status`).
fn process_gids(status: &str) -> Vec<u32> {
    status
        .lines()
        .find_map(|l| l.strip_prefix("Groups:"))
        .map(|g| {
            g.split_whitespace()
                .filter_map(|n| n.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn say(mark: &str, text: impl AsRef<str>) {
    println!("[{mark}] {}", text.as_ref());
}

/// Runs the tool when the command line asks for it. `true` = it ran (the app must not start).
pub fn run_from_args() -> bool {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(at) = args.iter().position(|a| a == "--touchpad-debug") else {
        return false;
    };
    let secs = args
        .get(at + 1)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_SECS)
        .clamp(1, MAX_SECS);
    run(Duration::from_secs(secs));
    true
}

struct Pad {
    node: String,
    file: std::fs::File,
    geo: Geometry,
    id: u64,
    parser: Parser,
    last_print: Option<Duration>,
    last_count: usize,
}

fn run(live: Duration) {
    println!("CursorPie touchpad debug (stop a running CursorPie first)\n");
    let mut facts = Facts::default();

    // 1. session and setting
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let display = std::env::var("DISPLAY").unwrap_or_default();
    let wayland = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
    facts.wayland_only = display.is_empty() && !wayland.is_empty();
    say(
        if facts.wayland_only { "!!" } else { "ok" },
        format!("session: type={session:?} DISPLAY={display:?} WAYLAND_DISPLAY={wayland:?}"),
    );
    let c = cfg();
    facts.switch_on = c.touch_trigger;
    say(
        if facts.switch_on { "ok" } else { "!!" },
        format!(
            "config: touch_trigger={} still={} mm hold={} ms typing_guard={} ms",
            c.touch_trigger, c.touch_still_mm, c.pie_hold_ms, c.touch_typing_guard_ms
        ),
    );
    drop(c);

    // 2. groups
    let groups = std::fs::read_to_string("/etc/group").unwrap_or_default();
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let names = group_names(&groups, &process_gids(&status));
    let in_input = names.iter().any(|n| n == "input");
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_default();
    facts.group_not_active = !in_input && listed_in_group(&groups, "input", &user);
    say(
        if in_input { "ok" } else { ".." },
        format!("groups of this session: {}", names.join(" ")),
    );

    // 3. devices
    let Ok(text) = std::fs::read_to_string("/proc/bus/input/devices") else {
        say("!!", "cannot read /proc/bus/input/devices");
        finish(&facts);
        return;
    };
    let list = inputdev::explain_touchpads(&text);
    if list.is_empty() {
        say(
            "!!",
            "the kernel lists no device that has anything to do with touch",
        );
    }
    let mut pads = Vec::new();
    for cand in &list {
        match cand.verdict {
            Err(why) => say(
                "..",
                format!("{} ({}): left out, it {why}", cand.name, cand.node),
            ),
            Ok(()) => {
                facts.pads += 1;
                let path = format!("/dev/input/{}", cand.node);
                match open_node(&path) {
                    Ok(file) => {
                        let fd = raw_fd(&file);
                        match (
                            read_axis(fd, ABS_MT_POSITION_X),
                            read_axis(fd, ABS_MT_POSITION_Y),
                        ) {
                            (Some(x), Some(y)) => {
                                facts.open += 1;
                                let geo = Geometry { x, y };
                                let (w, h) = geo.size_mm();
                                say(
                                    "ok",
                                    format!(
                                        "{} ({}): touchpad, opened; x {}..{} y {}..{} resolution {}/{} units per mm, surface {w:.0} x {h:.0} mm",
                                        cand.name,
                                        cand.node,
                                        x.min,
                                        x.max,
                                        y.min,
                                        y.max,
                                        x.resolution,
                                        y.resolution
                                    ),
                                );
                                if w == 0.0 {
                                    say(
                                        "..",
                                        "the size is unknown: a 130 x 90 mm surface is assumed, millimeters are approximate",
                                    );
                                }
                                pads.push(Pad {
                                    node: cand.node.clone(),
                                    file,
                                    geo,
                                    id: device_id(&cand.node),
                                    parser: Parser::new(),
                                    last_print: None,
                                    last_count: 0,
                                });
                            }
                            _ => say(
                                "!!",
                                format!(
                                    "{} ({path}): opened, but the position axes cannot be read (EVIOCGABS failed or this CPU architecture is not supported)",
                                    cand.name
                                ),
                            ),
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                        facts.denied += 1;
                        say(
                            "!!",
                            format!("{} ({path}): touchpad, but permission denied", cand.name),
                        );
                    }
                    Err(e) => say(
                        "!!",
                        format!("{} ({path}): cannot be opened: {e}", cand.name),
                    ),
                }
            }
        }
    }

    // 5. live
    if !pads.is_empty() {
        facts.live = true;
        println!(
            "\nLive for {} s: touch the pad; hold one finger still for the hold time. Ctrl-C ends it.\n",
            live.as_secs()
        );
        watch(&mut pads, live, &mut facts);
    }
    finish(&facts);
}

fn finish(facts: &Facts) {
    println!();
    let h = hints(facts);
    if h.is_empty() {
        say("ok", "nothing to fix found");
    }
    for line in h {
        say("->", line);
    }
}

/// Reads the pads for `live` and prints what the fingers and the recognizer do.
fn watch(pads: &mut [Pad], live: Duration, facts: &mut Facts) {
    use std::io::Read;
    let start = Instant::now();
    let mut rec = Recognizer::new(params_from_config());
    let mut fingers: Vec<Finger> = Vec::with_capacity(MAX_SLOTS);
    let mut events = Vec::new();
    let mut buf = [0u8; EVENT_SIZE * 64];
    while start.elapsed() < live {
        let mut fds: Vec<PollFd> = pads
            .iter()
            .map(|p| PollFd {
                fd: raw_fd(&p.file),
                events: POLLIN,
                revents: 0,
            })
            .collect();
        let ready = poll_fds(&mut fds, REPEAT_MS);
        let now = start.elapsed();
        for (i, pad) in pads.iter_mut().enumerate() {
            let mut frame = false;
            if ready > 0 && fds[i].revents & POLLIN != 0 {
                loop {
                    match (&pad.file).read(&mut buf) {
                        Ok(n) if n > 0 => {
                            for (ty, code, value) in decode(&buf[..n]) {
                                frame |= pad.parser.feed(ty, code, value);
                            }
                        }
                        _ => break,
                    }
                }
            }
            if frame {
                facts.frames = true;
            }
            // a resting finger sends nothing: repeat the frame, as the app does
            if !(frame || pad.parser.any_down()) {
                continue;
            }
            pad.parser.fingers(&pad.geo, &mut fingers);
            facts.max_fingers = facts.max_fingers.max(fingers.len());
            events.clear();
            rec.frame(
                &Frame {
                    device: pad.id,
                    time: now,
                    size_mm: pad.geo.size_mm(),
                    fingers: &fingers,
                },
                &mut events,
            );
            print_fingers(pad, now, &fingers);
            report(&events, now, facts);
        }
        events.clear();
        rec.poll(now, &mut events);
        report(&events, now, facts);
    }
}

fn print_fingers(pad: &mut Pad, now: Duration, fingers: &[Finger]) {
    let changed = fingers.len() != pad.last_count;
    let due = pad
        .last_print
        .is_none_or(|t| now.saturating_sub(t) >= Duration::from_millis(250));
    if !(changed || due) {
        return;
    }
    pad.last_count = fingers.len();
    pad.last_print = Some(now);
    let (w, h) = pad.geo.size_mm();
    let at: Vec<String> = fingers
        .iter()
        .map(|f| {
            if w > 0.0 {
                format!("({:.1},{:.1})mm", f.x * w, f.y * h)
            } else {
                format!("({:.0}%,{:.0}%)", f.x * 100.0, f.y * 100.0)
            }
        })
        .collect();
    println!(
        "{:7.3}s {:8} fingers={} {}",
        now.as_secs_f32(),
        pad.node,
        fingers.len(),
        at.join(" ")
    );
}

fn report(events: &[TouchEvent], now: Duration, facts: &mut Facts) {
    for e in events {
        match e {
            TouchEvent::HoldArmed { .. } => facts.armed = true,
            TouchEvent::HoldOpen => facts.opened = true,
            _ => {}
        }
        if !matches!(e, TouchEvent::Move) {
            println!("{:7.3}s   -> recognizer: {e:?}", now.as_secs_f32());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROUPS: &str = "root:x:0:\ninput:x:104:alice,bob\nwheel:x:10:alice\nbob:x:1001:\n";

    #[test]
    fn groups_are_named_and_membership_is_read_from_the_file() {
        assert_eq!(group_names(GROUPS, &[0, 10]), ["root", "wheel"]);
        assert!(group_names(GROUPS, &[]).is_empty());
        assert!(listed_in_group(GROUPS, "input", "alice"));
        assert!(!listed_in_group(GROUPS, "input", "carol"));
        assert!(!listed_in_group(GROUPS, "wheel", "bob"));
        // "bob" the user group has no members line to match
        assert!(!listed_in_group(GROUPS, "bob", "bob"));
    }

    #[test]
    fn the_groups_of_the_process_come_from_its_status() {
        let status = "Name:\tcursorpie\nGroups:\t10 104 1000 \nUid:\t1000\n";
        assert_eq!(process_gids(status), [10, 104, 1000]);
        assert!(process_gids("Name:\tx\n").is_empty());
    }

    fn ok() -> Facts {
        Facts {
            switch_on: true,
            pads: 1,
            open: 1,
            live: true,
            frames: true,
            max_fingers: 1,
            armed: true,
            opened: true,
            ..Facts::default()
        }
    }

    #[test]
    fn the_hints_name_the_broken_link() {
        let all = |f: Facts| hints(&f).join(" ");
        assert!(
            all(Facts {
                switch_on: false,
                ..ok()
            })
            .contains("Save")
        );
        assert!(
            all(Facts {
                wayland_only: true,
                ..ok()
            })
            .contains("Wayland")
        );
        assert!(
            all(Facts {
                pads: 0,
                open: 0,
                live: false,
                ..ok()
            })
            .contains("No device")
        );
        let denied = Facts {
            open: 0,
            denied: 1,
            live: false,
            frames: false,
            ..ok()
        };
        assert!(all(denied).contains("usermod"));
        assert!(
            all(Facts {
                group_not_active: true,
                ..denied
            })
            .contains("log out")
        );
        assert!(
            all(Facts {
                frames: false,
                ..ok()
            })
            .contains("sent nothing")
        );
        assert!(
            all(Facts {
                armed: false,
                opened: false,
                ..ok()
            })
            .contains("no hold started")
        );
        assert!(
            all(Facts {
                opened: false,
                ..ok()
            })
            .contains("hold time")
        );
        assert!(all(ok()).contains("work"));
    }

    #[test]
    fn a_hold_on_a_pad_is_armed_and_opened_by_the_recognizer() {
        // the same path `watch` takes, without a device: one still finger for longer than the hold
        let mut rec = Recognizer::new(params_from_config());
        let hold = params_from_config().hold;
        let mut out = Vec::new();
        let finger = [Finger {
            id: 1,
            x: 0.5,
            y: 0.5,
            size: 1.0,
        }];
        let mut t = Duration::ZERO;
        while t < hold + Duration::from_millis(200) {
            rec.frame(
                &Frame {
                    device: 1,
                    time: t,
                    size_mm: (100.0, 60.0),
                    fingers: &finger,
                },
                &mut out,
            );
            t += Duration::from_millis(20);
        }
        let mut facts = Facts::default();
        report(&out, t, &mut facts);
        assert!(facts.armed && facts.opened);
    }
}
