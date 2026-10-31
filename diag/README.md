# diag

The diagnostic log of CursorPie, as a crate of its own: switch-on-demand JSON lines, no dependencies, nothing in it
that is specific to the app. The full guide is the crate documentation (`cargo docs`, crate `cursorpie_diag`) and
[`docs/DEBUGGING.md`](../docs/DEBUGGING.md).

```rust
diag::log!("ui", "show_pie", x, y = y, kind = ?kind);        // x and kind: the variable's own name / Debug text
diag::trace!("tap", "drag", x = pos.x, y = pos.y);           // only with `trace` in the switch
diag::once!("x11", "no_xinput2");                            // the first time this line is reached
let _t = diag::timed!("ui", "load_icons", count = n);        // writes `ms` when it goes out of scope
diag::event("sys", "startup").field(format!("mon{i}"), r).write(); // names built at run time
```

It is part of **debug builds only**: a release build has none of the code, unless it is built with the feature
`force` (in the app: `--features debug-log`).

The app sets it up once, in `src/debug.rs`; `CURSORPIE_DEBUG=1 cursorpie` switches it on and
`cargo tools diag` reads the result.
