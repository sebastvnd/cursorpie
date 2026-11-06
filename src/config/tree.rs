//! Helpers for walking the menu tree: `items[]` of a profile, with `Submenu` nesting.
//!
//! A *path* is a list of indices: `[3, 1]` is slice 1 inside the submenu at slice 3 of the top ring.

use super::{Action, Config, Item, ItemKind, Profile};
use std::collections::BTreeSet;

/// The ring at `path`: an empty path is the top ring, `[3]` is the submenu at slice 3, and so on.
/// `None` if the path leaves the tree.
pub fn items_at<'a>(items: &'a [Item], path: &[usize]) -> Option<&'a [Item]> {
    let mut cur = items;
    for &i in path {
        match &cur.get(i)?.kind {
            ItemKind::Submenu(children) => cur = children.as_slice(),
            _ => return None,
        }
    }
    Some(cur)
}

/// The slice at `path` (the last index selects the slice inside the ring the rest leads to).
pub fn item_at<'a>(items: &'a [Item], path: &[usize]) -> Option<&'a Item> {
    let (last, parents) = path.split_last()?;
    items_at(items, parents)?.get(*last)
}

/// Mutable version of [`item_at`].
pub fn item_at_mut<'a>(items: &'a mut [Item], path: &[usize]) -> Option<&'a mut Item> {
    let (first, rest) = path.split_first()?;
    let item = items.get_mut(*first)?;
    if rest.is_empty() {
        return Some(item);
    }
    match &mut item.kind {
        ItemKind::Submenu(children) => item_at_mut(children, rest),
        _ => None,
    }
}

/// Whether one action runs a shell command.
fn action_runs_command(a: &Action) -> bool {
    match a {
        Action::Command(_) => true,
        Action::Sequence(v) => v.iter().any(action_runs_command),
        _ => false,
    }
}

/// Whether any slice in `v` (submenus included) runs a shell command.
fn items_run_commands(v: &[Item]) -> bool {
    v.iter().any(|i| match &i.kind {
        ItemKind::Mode {
            back,
            forward,
            extra,
        } => {
            action_runs_command(back)
                || action_runs_command(forward)
                || extra.values().any(action_runs_command)
        }
        ItemKind::Run(a) => action_runs_command(a),
        ItemKind::Submenu(c) => items_run_commands(c),
    })
}

/// Adds to `out` the extra buttons that a mode in `items` (submenus included) sends an action for.
fn collect_extra_buttons(items: &[Item], out: &mut BTreeSet<i64>) {
    for it in items {
        match &it.kind {
            ItemKind::Mode { extra, .. } => out.extend(
                extra
                    .iter()
                    .filter(|(_, a)| !matches!(a, Action::Nothing))
                    .map(|(b, _)| *b),
            ),
            ItemKind::Submenu(children) => collect_extra_buttons(children, out),
            ItemKind::Run(_) => {}
        }
    }
}

/// Adds to `out` the app of every "Open app" slice in `items` (submenus included), once each, in
/// the order they are met, as typed in the slice (trimmed).
fn collect_open_apps(items: &[Item], out: &mut Vec<String>) {
    for it in items {
        match &it.kind {
            ItemKind::Run(a) => {
                if let Some(app) = a.open_target()
                    && !out.iter().any(|o| o == app)
                {
                    out.push(app.to_string());
                }
            }
            ItemKind::Submenu(children) => collect_open_apps(children, out),
            ItemKind::Mode { .. } => {}
        }
    }
}

/// The apps that the "Open app" slices of the pies `profiles` open, submenus included, each once.
/// An index that is not a profile is skipped. Empty when no such slice exists, which is what lets a
/// press skip looking at the running apps altogether.
pub fn open_apps_of(c: &Config, profiles: &[usize]) -> Vec<String> {
    let mut out = Vec::new();
    for &p in profiles {
        if let Some(profile) = c.profiles.get(p) {
            collect_open_apps(&profile.items, &mut out);
        }
    }
    out
}

/// The extra buttons (button 5 and up; back and forward are always handled) that some mode in any
/// profile sends an action for. Only these are taken from the apps: the others keep working as before.
pub fn extra_buttons_in_use(c: &Config) -> BTreeSet<i64> {
    let mut out = BTreeSet::new();
    for p in &c.profiles {
        collect_extra_buttons(&p.items, &mut out);
    }
    out
}

/// true if any action in the tree runs a shell command (used to warn after importing a config).
pub fn has_commands(cfg: &Config) -> bool {
    cfg.profiles.iter().any(profile_has_commands) || cfg.flows.iter().any(|f| f.has_commands())
}

/// true if any action of the profile runs a shell command (used to warn after importing a profile).
pub fn profile_has_commands(p: &Profile) -> bool {
    items_run_commands(&p.items)
}

/// Mutable version of [`items_at`]: the ring at `path`, where an empty path is the top ring.
/// `None` if the path leaves the tree.
pub fn ring_mut<'a>(items: &'a mut Vec<Item>, path: &[usize]) -> Option<&'a mut Vec<Item>> {
    if path.is_empty() {
        return Some(items);
    }
    match &mut item_at_mut(items, path)?.kind {
        ItemKind::Submenu(c) => Some(c),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_app(name: &str, app: &str) -> Item {
        Item::run(name, "ico:app-window", Action::OpenApp(app.into()))
    }

    fn profile(items: Vec<Item>) -> Profile {
        Profile {
            name: "p".into(),
            apps: Vec::new(),
            items,
            active: vec![0],
        }
    }

    fn config_with(profiles: Vec<Profile>) -> Config {
        Config {
            profiles,
            ..Config::default()
        }
    }

    #[test]
    fn the_apps_of_a_profile_are_listed_once_submenus_included() {
        let c = config_with(vec![profile(vec![
            run_app("a", "Safari"),
            Item::run("key", "k", Action::Nothing),
            Item::submenu(
                "more",
                "m",
                vec![run_app("b", " Notes "), run_app("again", "Safari")],
            ),
            Item::run(
                "seq",
                "s",
                Action::Sequence(vec![Action::Nothing, Action::OpenApp("Mail".into())]),
            ),
        ])]);
        assert_eq!(open_apps_of(&c, &[0]), ["Safari", "Notes", "Mail"]);
    }

    #[test]
    fn a_pie_without_an_open_app_slice_has_no_apps() {
        let c = config_with(vec![profile(vec![
            Item::run("key", "k", Action::Nothing),
            Item::run("blank", "b", Action::OpenApp("  ".into())),
            Item::run("url", "u", Action::OpenUrl("https://example.com".into())),
        ])]);
        assert!(open_apps_of(&c, &[0]).is_empty());
        assert!(open_apps_of(&c, &[]).is_empty());
        assert!(open_apps_of(&c, &[7]).is_empty(), "not a profile");
    }

    #[test]
    fn the_apps_of_several_pies_are_joined_without_repeats() {
        let c = config_with(vec![
            profile(vec![run_app("a", "Safari")]),
            profile(vec![run_app("b", "Safari"), run_app("c", "Notes")]),
        ]);
        assert_eq!(open_apps_of(&c, &[0, 1]), ["Safari", "Notes"]);
    }
}
