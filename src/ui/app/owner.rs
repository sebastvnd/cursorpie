//! Who has the one window right now, and how it passes from one owner to the next.
//!
//! The pie, the notice pill, the lock animation, a panel and the Settings card all share one native
//! window (see [`guides::architecture`](crate::guides::architecture#one-window-for-everything)).
//! Which of them owns it used to be implied by a dozen flags (`pie_open`, `visible`, `hud`, `panel`,
//! `lock_anim`, `settings_open`, `close_flush`, `standby`...) that every transition had to keep in
//! step by hand, and the order in which `App::ui` tests them was the only definition of "who is
//! drawing". A transition that forgot one flag left a stale owner behind (a lock animation that
//! kept the stand-by from mapping the window, a pill of the last pie that came back with the next
//! one), and a transition that reconfigured a window that was still mapped for the old owner drew
//! the old owner's last frame stretched, or at the old place, for a few frames.
//!
//! This file is the one place that says:
//!
//! * [`App::owner`]: the owner, from the flags, in the same priority as `App::ui` draws them;
//! * [`handover`]: what a change of owner does to the window ([`Handover`]), as a pure function that
//!   is tested here, so `show_pie` and the others ask it instead of each deciding on their own;
//! * [`App::note_owner`]: writes every change of owner to the diagnostic log (`ui` / `owner`), so a
//!   glitch can be read back as "from X to Y".

use super::App;

/// Who the window belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Owner {
    /// Hidden, nobody wants it.
    Idle,
    /// Mapped, empty and parked off screen, waiting for a pie (Windows, see `standby`).
    Standby,
    /// The pie: open, blooming, fading, or a preview of it. The pill of the slice names is drawn
    /// *inside* this owner, it never owns the window.
    Pie,
    /// The padlock animation (the stand-in; a pie that collapses into the padlock is still `Pie`
    /// until `pie_open` goes false, then this).
    Lock,
    /// The notice pill of an action.
    Notice,
    /// A panel of a flow or of the browser extension.
    Panel,
    /// The Settings or welcome card, and the empty frames that follow it (`close_flush`).
    Card,
}

impl Owner {
    /// A short name for the diagnostic log.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Owner::Idle => "idle",
            Owner::Standby => "standby",
            Owner::Pie => "pie",
            Owner::Lock => "lock",
            Owner::Notice => "notice",
            Owner::Panel => "panel",
            Owner::Card => "card",
        }
    }
}

/// What a change of owner does to the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Handover {
    /// The window is hidden (or empty and parked): the new owner sizes, places and maps it the usual way.
    Fresh,
    /// The window is mapped and already has the size and the place the new owner needs: nothing is
    /// sent to it (a submenu of the pie, a panel that follows the previous one).
    Keep,
    /// The window is mapped but is not what the new owner needs. Moving or resizing a mapped window
    /// is a request that the window manager carries out a few frames later, so for those frames
    /// the new owner is drawn into the old geometry: stretched, cut off, or at the old place. Where
    /// that happens (Linux) the window is hidden first and comes back through the usual map.
    Remap,
}

/// What passing the window from `from` to `to` takes. `fits` says whether the window as it stands
/// already has the size and the place `to` needs.
pub(crate) fn handover(from: Owner, to: Owner, fits: bool) -> Handover {
    match from {
        Owner::Idle | Owner::Standby => Handover::Fresh,
        _ if from == to && fits => Handover::Keep,
        _ => Handover::Remap,
    }
}

impl App {
    /// Who owns the window now, in the order `App::ui` draws them.
    pub(crate) fn owner(&self) -> Owner {
        if self.settings_open || self.close_flush.is_some() {
            Owner::Card
        } else if self.panel.is_some() && !self.pie_open {
            Owner::Panel
        } else if self.lock_anim.is_some() && !self.pie_open {
            Owner::Lock
        } else if self.hud.is_some() && !self.pie_open {
            Owner::Notice
        } else if self.pie_open || self.visible {
            Owner::Pie
        } else if self.standby.is_some() {
            Owner::Standby
        } else {
            Owner::Idle
        }
    }

    /// Runs every pass of `App::logic`: writes a change of owner to the diagnostic log.
    pub(super) fn note_owner(&mut self) {
        let now = self.owner();
        if now != self.last_owner {
            diag::log!(
                "ui",
                "owner",
                from = self.last_owner.name(),
                to = now.name(),
                visible = self.visible
            );
            self.last_owner = now;
        }
    }

    /// Forgets what only a pie or the lock animation owns, for an owner that takes the window from
    /// them (Settings): the lock animation that was playing, the padlock that was opening in the
    /// middle of a pie, and the pill of the slice names of the last pie.
    ///
    /// Nothing else clears these when the window changes hands: a `lock_anim` that stayed `Some`
    /// kept `window_taken` true for ever (so the stand-by never mapped the window again and the
    /// next pie came late), and a stale `slice_pill` was drawn, or found "over" and skipped for a
    /// frame, by the next pie.
    pub(super) fn release_transients(&mut self) {
        self.lock_anim = None;
        self.lock_over_pie = false;
        self.lock_reveal = None;
        self.slice_pill = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hidden_or_parked_window_is_simply_mapped_by_whoever_wants_it() {
        for to in [
            Owner::Pie,
            Owner::Notice,
            Owner::Panel,
            Owner::Lock,
            Owner::Card,
        ] {
            assert_eq!(handover(Owner::Idle, to, false), Handover::Fresh);
            assert_eq!(handover(Owner::Standby, to, true), Handover::Fresh);
        }
    }

    #[test]
    fn a_submenu_keeps_the_window_it_has() {
        assert_eq!(handover(Owner::Pie, Owner::Pie, true), Handover::Keep);
        assert_eq!(handover(Owner::Panel, Owner::Panel, true), Handover::Keep);
    }

    #[test]
    fn a_mapped_window_that_does_not_fit_is_remapped_not_reconfigured() {
        // the same owner but the ring has to leave the window: remap
        assert_eq!(handover(Owner::Pie, Owner::Pie, false), Handover::Remap);
        // a new pie while the last one still fades, or after a notice that left it mapped
        for from in [Owner::Notice, Owner::Lock, Owner::Panel, Owner::Card] {
            assert_eq!(handover(from, Owner::Pie, true), Handover::Remap);
            assert_eq!(handover(from, Owner::Pie, false), Handover::Remap);
        }
    }

    #[test]
    fn every_owner_has_a_name_for_the_log() {
        let all = [
            Owner::Idle,
            Owner::Standby,
            Owner::Pie,
            Owner::Lock,
            Owner::Notice,
            Owner::Panel,
            Owner::Card,
        ];
        let mut names: Vec<&str> = all.iter().map(|o| o.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), all.len());
    }

    #[test]
    fn the_owner_follows_the_flags_in_the_order_the_window_is_drawn() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = App::new(rx, tx, 200.0, None);
        assert_eq!(app.owner(), Owner::Idle);
        app.visible = true;
        assert_eq!(
            app.owner(),
            Owner::Pie,
            "a mapped window with no other owner is the pie's"
        );
        app.pie_open = true;
        assert_eq!(app.owner(), Owner::Pie);
        app.pie_open = false;
        app.lock_anim = Some(crate::ui::pie::lock::LockAnim::new(true));
        assert_eq!(app.owner(), Owner::Lock);
        app.settings_open = true;
        assert_eq!(app.owner(), Owner::Card, "Settings wins over everything");
        app.release_transients();
        app.settings_open = false;
        assert_eq!(app.owner(), Owner::Pie);
    }
}
