//! The whole path with the log switched on: marker file -> `Builder::init` -> macros -> a file of
//! JSON lines. The log is global to the process, so this is the only test of this file.

// A release build has no log (see the crate documentation), so there is nothing to read back.
#![cfg(any(debug_assertions, feature = "force"))]

use cursorpie_diag::{Builder, Value, Watch};
use std::fs;

static WINDOW: Watch = Watch::new("win", "win_obs", 2000, 12);

fn count(lines: &[&str], needle: &str) -> usize {
    lines.iter().filter(|l| l.contains(needle)).count()
}

#[test]
fn a_run_writes_one_json_line_per_event() {
    let root = std::env::temp_dir().join(format!("cursorpie-diag-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    // the marker file switches the log on without touching the environment
    fs::write(root.join("debug"), "trace").unwrap();

    let on = Builder::new("diagtest")
        .support_dir(root.clone())
        .version("1.2.3")
        .report_env(&["DIAGTEST_SURELY_NOT_SET"])
        .header("backend", "x11")
        .init();
    assert!(on);
    assert!(cursorpie_diag::is_on());
    assert!(cursorpie_diag::enabled("ui") && cursorpie_diag::trace_enabled("ui"));

    let x = 812.5_f64;
    let name = "a\"b";
    let shape = vec![1, 2];
    cursorpie_diag::log!("ui", "show_pie", x, y = 2.0_f64, name, shape = ?shape);
    cursorpie_diag::trace!("tap", "drag", x = 1.0_f64);
    for _ in 0..3 {
        cursorpie_diag::once!("ui", "only_once", n = 1);
    }
    cursorpie_diag::event("sys", "startup")
        .field("monitors", 2_usize)
        .field(format!("mon{}", 0), "0,0 10x10")
        .write();
    {
        let mut timer = cursorpie_diag::timed!("ui", "work", step = 1);
        timer.field("done", true);
    }
    WINDOW.expect(&[("x", 10.0), ("y", 20.0)]);
    assert!(WINDOW.active());
    WINDOW.observe(
        Some(&[("x", 15.0), ("y", 20.0)]),
        &[("ppp", Value::from(1.0_f64))],
    );
    WINDOW.observe(Some(&[("x", 15.0), ("y", 20.0)]), &[]); // same answer: not written again

    let path = cursorpie_diag::active_path().expect("the log file is open");
    let body = fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = body.lines().collect();

    assert!(
        lines.iter().all(|l| l.starts_with('{') && l.ends_with('}')),
        "{body}"
    );
    assert!(
        lines[0].contains(r#""cat":"diag","ev":"start""#),
        "{}",
        lines[0]
    );
    assert!(lines[0].contains(r#""app":"diagtest""#) && lines[0].contains(r#""version":"1.2.3""#));
    assert!(lines[0].contains(r#""backend":"x11""#));
    assert!(lines[0].contains(r#""env_diagtest_surely_not_set":"""#));

    let pie = lines
        .iter()
        .find(|l| l.contains(r#""ev":"show_pie""#))
        .unwrap();
    for part in [
        r#""x":812.5"#,
        r#""y":2.0"#,
        r#""name":"a\"b""#,
        r#""shape":"[1, 2]""#,
    ] {
        assert!(pie.contains(part), "{part} missing in {pie}");
    }
    assert_eq!(count(&lines, r#""ev":"drag""#), 1);
    assert_eq!(count(&lines, r#""ev":"only_once""#), 1);
    assert_eq!(count(&lines, r#""monitors":2,"mon0":"0,0 10x10""#), 1);
    let work = lines.iter().find(|l| l.contains(r#""ev":"work""#)).unwrap();
    assert!(work.contains(r#""step":1,"done":true,"ms":"#), "{work}");
    assert_eq!(count(&lines, r#""ev":"win_obs""#), 1);
    let obs = lines
        .iter()
        .find(|l| l.contains(r#""ev":"win_obs""#))
        .unwrap();
    for part in [
        r#""ex":10.0"#,
        r#""ox":15.0"#,
        r#""dx":5.0"#,
        r#""dy":0.0"#,
        r#""ppp":1.0"#,
    ] {
        assert!(obs.contains(part), "{part} missing in {obs}");
    }

    let _ = fs::remove_dir_all(&root);
}
