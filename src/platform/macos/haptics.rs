//! One haptic tick on a Force Touch trackpad, through the public `NSHapticFeedbackManager`.
//!
//! The call needs no private API. The system decides whether anything is felt: nothing happens
//! when no finger touches the trackpad (a mouse is silent), when the trackpad has no actuator or
//! when "Force Click and haptic feedback" is off in System Settings. CursorPie has no override.
//!
//! **Thread:** [`haptic`] is called by the UI thread (the message `Msg::Haptic` is handled in
//! `App::logic`), never by the event tap or the touch thread. Whether AppKit allows it from a
//! background thread is spike S4 of `docs/TRACKPAD_HAPTICS_PLAN.md`; until then the main thread is used.

use crate::platform::HapticKind;
use objc2_app_kit::{
    NSHapticFeedbackManager, NSHapticFeedbackPattern, NSHapticFeedbackPerformanceTime,
    NSHapticFeedbackPerformer,
};

/// The AppKit pattern for a [`HapticKind`].
fn pattern(kind: HapticKind) -> NSHapticFeedbackPattern {
    match kind {
        HapticKind::LevelChange => NSHapticFeedbackPattern::LevelChange,
        HapticKind::Alignment => NSHapticFeedbackPattern::Alignment,
        HapticKind::Generic => NSHapticFeedbackPattern::Generic,
    }
}

/// Plays one tick, now.
pub fn haptic(kind: HapticKind) {
    NSHapticFeedbackManager::defaultPerformer().performFeedbackPattern_performanceTime(
        pattern(kind),
        NSHapticFeedbackPerformanceTime::Now,
    );
}
