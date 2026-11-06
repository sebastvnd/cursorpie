//! `cargo tools trackpad`: starts the app with the virtual trackpad open, to try the trackpad
//! gestures with a plain mouse.
//!
//! The trackpad is a window of the real app (it feeds the same recognizer a real trackpad does), so
//! this builds and runs the app with the `vtrackpad` feature (an ordinary build has none of the tool's
//! code) and turns the tool on through the environment. Nothing is
//! written to the app's settings: closing the trackpad (its red dot) leaves the app running as usual.
//!
//! | Option | Meaning |
//! |---|---|
//! | `--width POINTS` | the width of the trackpad, 240 to 640 (default 360); the slider on the trackpad changes it too |
//! | `--silent` | no click sound |
//! | `--release` | build the app with optimizations (the default is a debug build) |

use std::path::Path;
use std::process::{Command, ExitCode};

/// The smallest and the largest width the app accepts (`WIDTHS` of `ui::vtrackpad`).
const WIDTHS: (f32, f32) = (240.0, 640.0);

/// What to start.
#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    pub width: Option<u32>,
    pub silent: bool,
    pub release: bool,
}

/// Reads the arguments after `trackpad`.
///
/// # Errors
/// On an unknown option, or a width that is missing, not a number or outside 240..=640.
pub fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        width: None,
        silent: false,
        release: false,
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--width" => {
                let n = args.next().ok_or("--width needs a number of points")?;
                let w: f32 = n
                    .parse()
                    .ok()
                    .filter(|w: &f32| w.is_finite())
                    .ok_or_else(|| format!("--width: '{n}' is not a number"))?;
                if !(WIDTHS.0..=WIDTHS.1).contains(&w) {
                    return Err(format!(
                        "--width: {w} is outside {} to {}",
                        WIDTHS.0, WIDTHS.1
                    ));
                }
                options.width = Some(w.round() as u32);
            }
            "--silent" => options.silent = true,
            "--release" => options.release = true,
            other => return Err(format!("unknown option '{other}' for trackpad")),
        }
    }
    Ok(options)
}

/// The environment that turns the tool on in the app.
fn environment(o: &Options) -> Vec<(&'static str, String)> {
    let mut env = vec![("CURSORPIE_VTRACKPAD", "1".to_string())];
    if let Some(w) = o.width {
        env.push(("CURSORPIE_VTRACKPAD_WIDTH", w.to_string()));
    }
    if o.silent {
        env.push(("CURSORPIE_VTRACKPAD_SOUND", "0".to_string()));
    }
    env
}

/// The arguments of the `cargo` that runs the app.
fn cargo_args(o: &Options) -> Vec<&'static str> {
    let mut args = vec!["run", "--package", "cursorpie", "--features", "vtrackpad"];
    if o.release {
        args.push("--release");
    }
    args
}

/// Builds and runs the app with the trackpad open, until the app quits.
pub fn run(root: &Path, o: &Options) -> Result<ExitCode, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    println!(
        "Starting CursorPie with the virtual trackpad (quit the app from its menu bar icon; the red dot closes the trackpad only)."
    );
    let status = Command::new(cargo)
        .args(cargo_args(o))
        .envs(environment(o))
        .current_dir(root)
        .status()
        .map_err(|e| format!("could not run cargo: {e}"))?;
    Ok(if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn it_starts_with_the_defaults() {
        let o = parse_args(&[]).unwrap();
        assert_eq!(
            environment(&o),
            vec![("CURSORPIE_VTRACKPAD", "1".to_string())]
        );
        assert_eq!(
            cargo_args(&o),
            ["run", "--package", "cursorpie", "--features", "vtrackpad"]
        );
    }

    #[test]
    fn the_options_reach_the_app_through_the_environment() {
        let o = parse_args(&args(&["--width", "420", "--silent", "--release"])).unwrap();
        let env = environment(&o);
        assert!(env.contains(&("CURSORPIE_VTRACKPAD_WIDTH", "420".to_string())));
        assert!(env.contains(&("CURSORPIE_VTRACKPAD_SOUND", "0".to_string())));
        assert_eq!(
            cargo_args(&o),
            [
                "run",
                "--package",
                "cursorpie",
                "--features",
                "vtrackpad",
                "--release"
            ]
        );
    }

    #[test]
    fn bad_options_are_told_why() {
        assert!(parse_args(&args(&["--width"])).is_err());
        assert!(parse_args(&args(&["--width", "wide"])).is_err());
        assert!(parse_args(&args(&["--width", "NaN"])).is_err());
        assert!(
            parse_args(&args(&["--width", "100"]))
                .unwrap_err()
                .contains("outside")
        );
        assert!(parse_args(&args(&["--loud"])).is_err());
    }
}
