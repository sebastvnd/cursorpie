//! Who is using the pie: the answer to "What do you do most?" on the welcome guide.
//!
//! A person picks one or more [`Persona`]s and the guide shapes the first pie around them:
//!
//! * [`pie_for`] builds the slices of the general pie. Each persona has a short list of slices, in
//!   order of importance; several personas are merged round by round (every persona gets its best
//!   slice in before anyone gets a second one), equal names are kept once, and the last slot of the
//!   ring is always the "Launch" submenu of the default pie.
//! * [`app_profiles`] lists the pies of single apps that belong to a persona (a design pie for
//!   Figma and Photoshop, a writing pie for Word and Obsidian...). A persona without such apps
//!   adds none.
//! * [`apply`] writes both into a [`Config`]. It only ever *adds* profiles and replaces the slices
//!   of the general one, so nothing the person already made is lost.
//!
//! Everything is built from the same presets as the default config (`presets_for`,
//! `quick_actions_for`), so the shortcuts are right for the OS, and everything is a pure function
//! of the OS, so the tests cover all three systems on any host.

use super::defaults::{presets_for, quick_actions_for};
use super::{Action, Config, Item, MAX_ITEMS, MAX_PROFILES, Profile};
use crate::keys::{Shortcut, key};
use crate::platform::Os;
use crate::platform::linux::roles::Role;

/// What a person does most with their computer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Persona {
    Everyday,
    Student,
    Developer,
    Designer,
    Writer,
    Creator,
}

impl Persona {
    /// Every persona, in the order the welcome guide shows them (and the order they are merged in).
    pub const ALL: [Persona; 6] = [
        Persona::Everyday,
        Persona::Student,
        Persona::Developer,
        Persona::Designer,
        Persona::Writer,
        Persona::Creator,
    ];

    /// Short name, one word.
    pub fn title(self) -> &'static str {
        match self {
            Persona::Everyday => "Everyday",
            Persona::Student => "Student",
            Persona::Developer => "Developer",
            Persona::Designer => "Designer",
            Persona::Writer => "Writer",
            Persona::Creator => "Creator",
        }
    }

    /// One short line under the name.
    pub fn blurb(self) -> &'static str {
        match self {
            Persona::Everyday => "Browsing, chatting and getting things done.",
            Persona::Student => "Notes, research and long reading.",
            Persona::Developer => "Code, terminals and docs.",
            Persona::Designer => "Design tools, zoom and precision.",
            Persona::Writer => "Docs, notes and long-form text.",
            Persona::Creator => "Video, music and editing.",
        }
    }

    /// The slices of this persona, best first. The first one is always "History", so the side
    /// buttons of the pie start on something every app understands.
    fn slices(self, os: Os) -> Vec<Item> {
        let p = presets_for(os); // 0 History, 1 Brackets, 2 Clipboard, 3 Browser, 4 Tabs, 5 Zoom, 6 Scroll, 7 Edit
        let q = quick_actions_for(os); // 0 Screenshot, 3 Emoji (the same place on every OS)
        let at = |v: &[Item], i: usize| v[i].clone();
        match self {
            Persona::Everyday => vec![
                at(&p, 0),
                at(&p, 2),
                at(&p, 3),
                at(&q, 0),
                at(&p, 6),
                at(&q, 3),
                at(&p, 5),
            ],
            Persona::Student => vec![
                at(&p, 0),
                at(&p, 2),
                at(&p, 3),
                at(&p, 5),
                at(&q, 0),
                at(&p, 4),
                at(&p, 7),
            ],
            Persona::Developer => vec![
                at(&p, 0),
                at(&p, 1),
                at(&p, 2),
                terminal(os),
                at(&p, 3),
                at(&p, 7),
                at(&p, 4),
            ],
            Persona::Designer => vec![
                at(&p, 0),
                at(&p, 5),
                at(&p, 2),
                at(&q, 0),
                at(&p, 6),
                at(&p, 3),
            ],
            Persona::Writer => vec![
                at(&p, 0),
                at(&p, 2),
                at(&p, 7),
                at(&p, 5),
                at(&p, 3),
                at(&q, 3),
                at(&q, 0),
            ],
            Persona::Creator => vec![
                at(&p, 0),
                frames(),
                at(&p, 5),
                at(&p, 6),
                at(&q, 0),
                at(&p, 2),
            ],
        }
    }

    /// The pie of the apps this persona lives in, if there are any on `os`.
    fn app_profile(self, os: Os) -> Option<Profile> {
        let p = presets_for(os);
        let (name, apps, items): (&str, &[&str], Vec<Item>) = match self {
            Persona::Designer => (
                "Design",
                match os {
                    Os::Mac => &[
                        "com.figma.Desktop",
                        "com.adobe.Photoshop",
                        "com.adobe.illustrator",
                        "com.bohemiancoding.sketch3",
                    ],
                    Os::Windows => &["figma.exe", "photoshop.exe", "illustrator.exe"],
                    Os::Linux => &[
                        "gimp",
                        "org.gimp.gimp",
                        "inkscape",
                        "org.inkscape.inkscape",
                        "krita",
                        "org.kde.krita",
                    ],
                },
                vec![p[0].clone(), p[5].clone(), p[6].clone(), p[2].clone()],
            ),
            Persona::Writer => (
                "Writing",
                match os {
                    Os::Mac => &[
                        "com.microsoft.Word",
                        "com.apple.iWork.Pages",
                        "notion.id",
                        "md.obsidian",
                    ],
                    Os::Windows => &["winword.exe", "notion.exe", "obsidian.exe"],
                    Os::Linux => &[
                        "obsidian",
                        "libreoffice-writer",
                        "org.libreoffice.libreoffice",
                    ],
                },
                vec![p[0].clone(), p[2].clone(), p[7].clone(), p[5].clone()],
            ),
            Persona::Creator => (
                "Editing",
                match os {
                    Os::Mac => &[
                        "com.apple.FinalCut",
                        "com.blackmagic-design.DaVinciResolve",
                        "com.apple.logic10",
                    ],
                    Os::Windows => &["resolve.exe", "adobe premiere pro.exe"],
                    Os::Linux => &["kdenlive", "org.kde.kdenlive", "davinci-resolve"],
                },
                vec![
                    p[0].clone(),
                    frames(),
                    p[5].clone(),
                    p[6].clone(),
                    p[2].clone(),
                ],
            ),
            // Developers already have the "Code" pie of the default config; the rest work in the
            // general pie and the browser pie.
            Persona::Everyday | Persona::Student | Persona::Developer => return None,
        };
        Some(Profile {
            name: name.into(),
            apps: apps.iter().map(|a| (*a).to_owned()).collect(),
            items,
            active: vec![0],
        })
    }
}

/// "Terminal" as a slice: the app of the OS, as in the default Launch submenu.
fn terminal(os: Os) -> Item {
    let app: String = match os {
        Os::Mac => "Terminal".into(),
        Os::Windows => "wt".into(),
        Os::Linux => Role::Terminal.text().into(),
    };
    Item::run("Terminal", "ico:terminal-window", Action::OpenApp(app))
}

/// Frame by frame in a timeline: left and right arrow.
fn frames() -> Item {
    Item::mode(
        "Frames",
        "ico:skip-forward",
        Shortcut::new(key::LEFT, 0),
        Shortcut::new(key::RIGHT, 0),
    )
}

/// The slices of the general pie for `picks`: the default pie when nothing is picked, otherwise
/// the personas merged (see the module doc), at most [`MAX_ITEMS`] slices.
pub fn pie_for(os: Os, picks: &[Persona]) -> Vec<Item> {
    let default = Config::default_for(os)
        .profiles
        .into_iter()
        .next()
        .map(|p| p.items)
        .unwrap_or_default();
    if picks.is_empty() {
        return default;
    }
    let lists: Vec<Vec<Item>> = Persona::ALL
        .iter()
        .copied()
        .filter(|p| picks.contains(p))
        .map(|p| p.slices(os))
        .collect();
    // the last slot of the ring is the Launch submenu of the default pie
    let launch = default.into_iter().find(|i| i.name == "Launch");
    let room = if launch.is_some() {
        MAX_ITEMS - 1
    } else {
        MAX_ITEMS
    };
    let mut out: Vec<Item> = Vec::new();
    'fill: for round in 0..MAX_ITEMS {
        for list in &lists {
            let Some(item) = list.get(round) else {
                continue;
            };
            if out.iter().any(|o| o.name == item.name) {
                continue;
            }
            out.push(item.clone());
            if out.len() >= room {
                break 'fill;
            }
        }
    }
    out.extend(launch);
    out
}

/// The pies of single apps that go with `picks`, one per persona that has any.
pub fn app_profiles(os: Os, picks: &[Persona]) -> Vec<Profile> {
    Persona::ALL
        .iter()
        .copied()
        .filter(|p| picks.contains(p))
        .filter_map(|p| p.app_profile(os))
        .collect()
}

/// Shapes `config` around `picks`: the general pie gets the merged slices and the app pies are
/// added (never twice, never above [`MAX_PROFILES`]). Does nothing for an empty pick.
pub fn apply(config: &mut Config, os: Os, picks: &[Persona]) {
    if picks.is_empty() {
        return;
    }
    if let Some(general) = config.profiles.iter_mut().find(|p| p.is_general()) {
        general.items = pie_for(os, picks);
        general.active = vec![0];
    }
    for profile in app_profiles(os, picks) {
        if config.profiles.len() >= MAX_PROFILES {
            break;
        }
        if config.profiles.iter().any(|p| p.name == profile.name) {
            continue;
        }
        config.profiles.push(profile);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYSTEMS: [Os; 3] = [Os::Mac, Os::Windows, Os::Linux];

    #[test]
    fn nothing_picked_leaves_the_default_pie() {
        for os in SYSTEMS {
            let default = Config::default_for(os).profiles.remove(0).items;
            assert_eq!(pie_for(os, &[]), default);
            let mut c = Config::default_for(os);
            let before = c.clone();
            apply(&mut c, os, &[]);
            assert_eq!(c.profiles, before.profiles);
        }
    }

    #[test]
    fn every_persona_makes_a_full_pie_that_starts_with_history() {
        for os in SYSTEMS {
            for persona in Persona::ALL {
                let pie = pie_for(os, &[persona]);
                assert!(!pie.is_empty() && pie.len() <= MAX_ITEMS, "{persona:?}");
                assert_eq!(pie[0].name, "History", "{persona:?} on {os:?}");
                assert_eq!(pie.last().map(|i| i.name.as_str()), Some("Launch"));
                let mut names: Vec<&str> = pie.iter().map(|i| i.name.as_str()).collect();
                names.sort_unstable();
                names.dedup();
                assert_eq!(names.len(), pie.len(), "{persona:?} has a slice twice");
            }
        }
    }

    #[test]
    fn all_personas_together_still_fit_the_ring() {
        for os in SYSTEMS {
            let pie = pie_for(os, &Persona::ALL);
            assert_eq!(pie.len(), MAX_ITEMS);
            // everyone's best slice (the one after History, which they all start with) is in
            // before anyone's second
            for persona in Persona::ALL {
                let best = &persona.slices(os)[1].name;
                assert!(
                    pie.iter().any(|i| &i.name == best),
                    "{persona:?} lost its best slice '{best}' on {os:?}"
                );
            }
        }
    }

    #[test]
    fn a_developer_gets_a_terminal_a_student_does_not() {
        for os in SYSTEMS {
            assert!(
                pie_for(os, &[Persona::Developer])
                    .iter()
                    .any(|i| i.name == "Terminal")
            );
            assert!(
                !pie_for(os, &[Persona::Student])
                    .iter()
                    .any(|i| i.name == "Terminal")
            );
        }
    }

    #[test]
    fn app_pies_belong_to_apps_and_are_added_once() {
        for os in SYSTEMS {
            for profile in app_profiles(os, &Persona::ALL) {
                assert!(!profile.apps.is_empty(), "{} lists no app", profile.name);
                assert!(!profile.items.is_empty() && profile.items.len() <= MAX_ITEMS);
                assert!(!profile.is_general());
            }
            let mut c = Config::default_for(os);
            apply(&mut c, os, &Persona::ALL);
            let count = c.profiles.len();
            apply(&mut c, os, &Persona::ALL);
            assert_eq!(c.profiles.len(), count, "a second apply adds nothing");
            assert_eq!(c.profiles.iter().filter(|p| p.is_general()).count(), 1);
            assert!(c.profiles.len() <= MAX_PROFILES);
        }
    }

    #[test]
    fn apply_keeps_the_profiles_the_person_already_has() {
        for os in SYSTEMS {
            let mut c = Config::default_for(os);
            let names: Vec<String> = c.profiles.iter().map(|p| p.name.clone()).collect();
            apply(&mut c, os, &[Persona::Designer]);
            for name in names {
                assert!(c.profiles.iter().any(|p| p.name == name), "{name} is gone");
            }
            assert!(c.profiles.iter().any(|p| p.name == "Design"));
        }
    }
}
