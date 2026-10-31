//! The switch: which events are written. Pure text in, a decision out, so it is easy to test.

/// Which events are written: everything or some categories, minus some, with or without `trace`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Spec {
    all: bool,
    trace: bool,
    stderr: bool,
    cats: Vec<String>,
    skip: Vec<String>,
}

impl Spec {
    /// Reads a list such as `1`, `tap,ui`, `all,trace` or `all,-tap,stderr`.
    ///
    /// | Word | Meaning |
    /// |---|---|
    /// | `1`, `on`, `true`, `yes`, `all`, `*` | every category |
    /// | `tap` (any other word) | that category; several words add up |
    /// | `-tap` or `!tap` | not that category, even when `all` is on |
    /// | `trace` | also the high-volume events written with [`trace!`](crate::trace) |
    /// | `stderr` | also print every line on stderr |
    /// | `0`, `off`, `false`, `no` | off, wherever it is in the list |
    ///
    /// Words are not case sensitive and are separated by commas or spaces. `None` means off:
    /// nothing at all, an empty text, or an off word. A list without a category (`trace`,
    /// `stderr`, `-tap`) means every category.
    ///
    /// ```
    /// use cursorpie_diag::Spec;
    ///
    /// let spec = Spec::parse("all,-tap").unwrap();
    /// assert!(spec.allows("ui", false));
    /// assert!(!spec.allows("tap", false));
    /// assert!(Spec::parse("tap,off").is_none());
    /// ```
    pub fn parse(text: &str) -> Option<Spec> {
        let tokens: Vec<String> = text
            .split(|c: char| c == ',' || c.is_whitespace())
            .map(|t| t.trim().to_ascii_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        if tokens.is_empty()
            || tokens
                .iter()
                .any(|t| matches!(t.as_str(), "0" | "off" | "false" | "no"))
        {
            return None;
        }
        let mut spec = Spec::default();
        for t in tokens {
            match t.as_str() {
                "1" | "on" | "true" | "yes" | "all" | "*" => spec.all = true,
                "trace" => spec.trace = true,
                "stderr" => spec.stderr = true,
                _ => {
                    if let Some(name) = t.strip_prefix('-').or_else(|| t.strip_prefix('!')) {
                        if !name.is_empty() {
                            spec.skip.push(name.to_string());
                        }
                    } else {
                        spec.cats.push(t);
                    }
                }
            }
        }
        if spec.cats.is_empty() {
            spec.all = true; // `trace`, `stderr` or `-tap` alone mean every category
        }
        Some(spec)
    }

    /// Whether an event of category `cat` is written. `trace` marks a high-volume event.
    pub fn allows(&self, cat: &str, trace: bool) -> bool {
        let named = |list: &[String]| list.iter().any(|c| c.eq_ignore_ascii_case(cat));
        (!trace || self.trace) && !named(&self.skip) && (self.all || named(&self.cats))
    }

    /// Whether every line is also printed on stderr.
    pub fn mirrors_to_stderr(&self) -> bool {
        self.stderr
    }
}

/// The switch from its two sources. A set environment variable wins (so `APP_DEBUG=0` turns a
/// marker file off for one run); a marker file that is empty counts as `1`.
pub fn resolve_spec(env: Option<&str>, marker_file: Option<&str>) -> Option<Spec> {
    match env {
        Some(v) => Spec::parse(v),
        None => marker_file.and_then(|f| Spec::parse(if f.trim().is_empty() { "1" } else { f })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_reads_lists_and_off_words() {
        assert_eq!(Spec::parse(""), None);
        assert_eq!(Spec::parse("0"), None);
        assert_eq!(Spec::parse("tap,off"), None, "off anywhere means off");
        let all = Spec::parse("1").unwrap();
        assert!(all.allows("tap", false) && all.allows("anything", false));
        assert!(!all.allows("tap", true), "trace events need `trace`");
        let some = Spec::parse(" TAP, ui ").unwrap();
        assert!(some.allows("tap", false) && some.allows("ui", false));
        assert!(
            some.allows("TAP", false),
            "categories are not case sensitive"
        );
        assert!(!some.allows("win", false));
    }

    #[test]
    fn trace_alone_means_everything_with_trace() {
        let t = Spec::parse("trace").unwrap();
        assert!(t.allows("tap", true) && t.allows("ui", false));
        let only_tap = Spec::parse("tap,trace").unwrap();
        assert!(only_tap.allows("tap", true));
        assert!(!only_tap.allows("ui", true) && !only_tap.allows("ui", false));
    }

    #[test]
    fn a_category_can_be_taken_out() {
        let spec = Spec::parse("all,-tap").unwrap();
        assert!(spec.allows("ui", false) && !spec.allows("tap", false));
        let alone = Spec::parse("!tap").unwrap();
        assert!(alone.allows("ui", false) && !alone.allows("tap", false));
        let both = Spec::parse("tap,ui,-ui").unwrap();
        assert!(both.allows("tap", false) && !both.allows("ui", false));
        assert!(
            Spec::parse("-").unwrap().allows("tap", false),
            "a lone dash names nothing"
        );
    }

    #[test]
    fn stderr_is_a_flag_and_not_a_category() {
        let spec = Spec::parse("stderr").unwrap();
        assert!(spec.mirrors_to_stderr() && spec.allows("anything", false));
        let tap = Spec::parse("tap,stderr").unwrap();
        assert!(tap.mirrors_to_stderr() && !tap.allows("stderr", false));
        assert!(!Spec::parse("tap").unwrap().mirrors_to_stderr());
    }

    #[test]
    fn the_variable_wins_over_the_marker_file() {
        assert_eq!(resolve_spec(Some("0"), Some("1")), None);
        assert_eq!(resolve_spec(Some("tap"), Some("1")), Spec::parse("tap"));
        assert_eq!(
            resolve_spec(None, Some("")),
            Spec::parse("1"),
            "empty file = on"
        );
        assert_eq!(resolve_spec(None, Some("ui")), Spec::parse("ui"));
        assert_eq!(resolve_spec(None, None), None);
    }
}
