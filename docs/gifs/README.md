# Demo GIFs

The animations used by the [README](../../README.md), the [user guide](../user/README.md) and the landing page of the
website. All are 720 x 405 pixels. They live here, next to the documentation, because the website publishes `docs/` as it is
and the Markdown files link to them with relative paths (`docs/gifs/<name>.gif` from the README, `../gifs/<name>.gif` from
`docs/user/`).

| File | Shows | Used in |
|---|---|---|
| `14-problem-hook.gif` | "Tired of hunting for that one shortcut?" | README, website |
| `01-hero.gif` | CursorPie intro: your actions, one gesture away | README, website |
| `02-hold-flick-release.gif` | Hold the middle button, flick to a slice, release | README, Getting started, Using the pie, website |
| `03-cancel-in-the-middle.gif` | Release in the center to cancel | README, Getting started, Using the pie, website |
| `04-any-action.gif` | Shortcuts, apps, shell, mouse modes, submenus, flows | README, Using the pie, website |
| `05-nested-submenus.gif` | Submenus up to 5 levels deep | README, Using the pie, website |
| `06-per-app-profiles.gif` | A pie per app | README, Profiles and mouse buttons, website |
| `07-mouse-side-buttons.gif` | Back and forward as undo and redo | README, Profiles and mouse buttons, website |
| `08-trackpad.gif` | Touch and hold on a trackpad | README, Trackpad hold, website |
| `09-flows.gif` | Chain actions into a flow | README, Flows, website |
| `10-settings-editor.gif` | The Settings editor | README, Settings and backups, website |
| `11-themes-and-share.gif` | Themes and profile export | README, Settings and backups, Themes, website |
| `12-private-and-open-source.gif` | No account, no telemetry, MIT | README, website |
| `13-platforms.gif` | macOS, Windows, Linux | README, Install, website |

## Changing one

Replace the file and keep its name, so no link has to change. Keep it at 720 x 405: the pages set `width="720"` and the
landing page reserves the space with the same size, so a different shape makes the page jump while it loads. The files
are large (3 to 7 MB each), so prefer fewer colors and frames over a bigger canvas.

The folder is listed under `exclude` in `Cargo.toml`: the GIFs are never part of the crate that `cargo publish` uploads
(crates.io allows 10 MB), and `.gitattributes` marks `*.gif` as binary.
