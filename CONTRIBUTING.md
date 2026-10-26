# Contributing to CursorPie

Thank you for helping! This page covers how to set up, build, test and submit a change. It is also rendered in
the Rust docs as `guides::contributing`.

## Quick start

```sh
git clone https://github.com/sebastvnd/cursorpie.git
cd cursorpie
cargo tools hooks install   # once per clone: turns on the Git hooks
cargo run --release         # try the app
cargo tools ci              # every check CI runs
```

Then: create a branch, commit in small steps, open a pull request, and squash-merge when CI is green. The details
are under [Development workflow](#development-workflow).

| I want to... | Go to |
|---|---|
| Set up my machine | [Prerequisites](#prerequisites) |
| Build or package the app | [Build, run and package](#build-run-and-package) |
| Know what to run before a pull request | [Development workflow](#development-workflow) |
| Know what the project expects | [Project rules](#project-rules) and [Coding guidelines](#coding-guidelines) |
| Write a commit message or pull request | [Commits and pull requests](#commits-and-pull-requests) |
| Find the right document | [Documentation](#documentation) |
| Publish a release (maintainers) | [Releasing](#releasing) |

## Prerequisites

- **macOS 11 or later, Windows 10 (1809) / 11, or Linux with an X11 session.** Other Unixes build a do-nothing
  stub, enough to run `cargo test`.
- **Rust 1.88 or later** (edition 2024), installed with [rustup](https://rustup.rs).
- **macOS:** Xcode Command Line Tools (`xcode-select --install`) for the linker and `codesign`.
- **Windows:** the MSVC build tools (Visual Studio Build Tools, "Desktop development with C++").
- **Linux:** a C linker and the development files of libx11, libxi, libxtst and libxrandr.
  Debian and Ubuntu: `sudo apt install build-essential libx11-dev libxi-dev libxtst-dev libxrandr-dev`.
  Fedora: `sudo dnf install gcc libX11-devel libXi-devel libXtst-devel libXrandr-devel`. A minimal system may
  also lack libraries the window toolkit loads at run time (OpenGL, libxkbcommon-x11, libxcursor).

## Build, run and package

`cargo run --release` builds and starts the app on every system. To produce something you can hand out:

| You are on | You want | Run | Result |
|---|---|---|---|
| macOS | the app | `scripts/bundle.sh` | `target/bundle/CursorPie.app` |
| macOS | a disk image | `scripts/make-dmg.sh` | `target/bundle/CursorPie-<version>-<arch>.dmg` |
| Windows | the app and the Setup wizard | `scripts\package-windows.ps1` | `target/bundle/CursorPie-windows.zip`, plus `CursorPie-<version>-setup.exe` if Inno Setup 6 is installed |
| Linux | the app | `scripts/package-linux.sh` | `target/bundle/cursorpie-<version>-linux-<arch>.tar.gz` |
| other Unix | to run the tests | `cargo test` | no app: it builds the do-nothing stub |
| any OS | to type-check the Windows backend | `cargo check --features check-windows` | no binary |

Only a native build on macOS, Windows or Linux gives you a working app. The other builds exist to check that your
change still compiles elsewhere. Signing, notarizing and installing are explained for users in
`docs/INSTALL.md`; the maintainer-facing notes follow.

**macOS.** CursorPie needs the **Accessibility** permission (and sometimes *Input Monitoring*). With `cargo run`,
macOS attributes it to your terminal app, and a rebuilt ad-hoc-signed binary may need it granted again. Set
`SIGN_ID` to sign with a stable identity. `UNIVERSAL=1` makes `scripts/bundle.sh` and `scripts/make-dmg.sh` build
one app for Apple silicon and Intel.

**Windows.** The zip holds `cursorpie.exe`, `LICENSE` and `README.md`. The executable is a tray app without a
console window and is not code signed unless `SIGN_CERT_THUMBPRINT` is set (and `signtool` is on `PATH`). The first
start of a packaged `cursorpie.exe` installs it for the current user (`platform/windows/install.rs`). Builds in a
`target\debug` or `target\release` folder are never installed, so `cargo run --release` cannot replace the copy
that starts at login; set `CURSORPIE_NO_INSTALL=1` for the same on any other copy. To test the install, run the
packaged exe from the unzipped folder and follow checks 7a to 7c of `docs/WINDOWS_MIGRATION.md`; test the Setup
wizard with check 7d.

**Linux.** The app works in an **X11 session** (supported, but experimental: its checklist is under internal
observation); in Wayland it only sees X11 and XWayland windows (see
`docs/LINUX_MIGRATION.md`). `scripts/package-linux.sh` builds a tarball with an `install.sh`. Build on an older
distribution if the binary must start on older ones. `CURSORPIE_BACKEND=x11` forces the X11 backend, for bug
reports.

### What each OS can check

Rust only compiles the code of the OS you build on, so a lint can only see that code:

| Code | Checked by |
|---|---|
| `platform/macos/`, `tap/macos.rs` | `cargo clippy` and `cargo test` on macOS only |
| `platform/windows/`, `tap/windows.rs` | Windows, or any OS with `--features check-windows` |
| `platform/linux/`, `tap/linux.rs` | Linux only. The pure helpers beside them (session, desktop entries, window hit test, XDG paths) are tested on every OS |
| everything else | any OS |

## Development workflow

Everything you run for the project is one command: `cargo tools <command>`. The tools live in the `tools/`
package (standard library only, never part of the app) and behave the same on macOS, Windows and Linux.
`cargo tools help` lists them.

| Command | What it does |
|---|---|
| `cargo tools hooks install` | once per clone: turns on the Git hooks of `.githooks/` |
| `cargo tools ci` | every check CI runs, in order, stopping at the first failure |
| `cargo tools ci --quick` | only the checks that take seconds (formatting, version files) |
| `cargo tools check` | `Cargo.toml`, `Cargo.lock` and `CHANGELOG.md` agree on the version |
| `cargo tools bump LEVEL` | raises the version and prepares the changelog (see [Releasing](#releasing)) |
| `cargo tools commit-msg FILE` | checks a commit message (the `commit-msg` hook runs it) |
| `cargo tools diag [FILE]` | reads a debug log of the app (`CURSORPIE_DEBUG=1`) and says where positions stop agreeing, see [docs/DEBUGGING.md](docs/DEBUGGING.md) |

### The loop

```text
1. cargo tools hooks install          once, right after cloning
2. git switch -c fix/short-name       start from an up-to-date main; one branch per change
3. edit, test, commit in small steps  the commit-msg hook checks every subject
4. cargo tools ci                     the pre-push hook runs it before every push
5. open a pull request                CI must be green on macOS, Windows and Linux
6. squash-merge into main             the pull request title becomes the commit subject
```

The hooks run `cargo tools ci --quick` before a commit, check the subject of the message, and run `cargo tools ci`
before a push. They are a convenience: `--no-verify` skips them once, and CI runs the same command, so CI has the
last word. A branch without a pull request does not start CI: open a draft pull request when you want it.

### What `cargo tools ci` runs

It must finish without warnings. The list lives in `tools/src/ci.rs`, and CI runs the same command, so there is
one definition of "the checks pass".

| Step | Command | Where |
|---|---|---|
| format | `cargo fmt --all -- --check` | everywhere |
| version files agree | `cargo tools check` | everywhere |
| clippy | `cargo clippy --workspace --all-targets -- -D warnings` | everywhere |
| tests | `cargo test --workspace --all-targets` | everywhere |
| doc tests | `cargo test --doc` | everywhere |
| docs | `cargo doc --workspace --no-deps --document-private-items` with `RUSTDOCFLAGS="-D warnings"` | macOS only: docs.rs builds for macOS, and only there the links to the macOS backend resolve |
| Windows backend | `cargo clippy --all-targets --features check-windows -- -D warnings` and `cargo test --features check-windows` | macOS and Linux (on Windows the backend is the build itself) |

Plain `cargo test` runs only the app: use `cargo test --workspace` to include the tools. `cargo docs` (an alias
from `.cargo/config.toml`) builds the docs and opens them. Because this page is part of the Rust docs, keep
URLs in `<...>` or link syntax, wrap anything with angle brackets in backticks, and point at repository files with
backticks instead of Markdown links.

## Project rules

These rules keep the project maintainable by more than one person over a long time. Each one exists because the
alternative costs more later.

- **`main` is always releasable.** Nothing is pushed to it directly: every change arrives in a pull request with
  green CI. Maintainers: switch on branch protection in the GitHub settings (require a pull request and the three
  `test` jobs).
- **One concern per pull request.** Formatting-only changes and drive-by cleanups go in their own pull request,
  so a review shows what changed and `git blame` stays useful.
- **Zero warnings.** rustfmt, clippy and rustdoc run with warnings as errors. Fix the cause; add an
  `#[allow(...)]` only with a comment that says why.
- **Tests come with logic.** A bug fix starts with a test that fails before the fix, whenever the bug can be
  reproduced without a screen. What cannot be unit tested (gestures, rendering, caches after sleep) is listed under
  "Tested by hand" in the pull request.
- **Notes and docs for users.** A user-visible change adds a line under `## Unreleased` in `CHANGELOG.md`,
  written for users of the app, and updates `docs/USER_GUIDE.md` or `docs/CONFIGURATION.md` when behavior or
  settings change. Refactors, tests, docs and tooling need no changelog line. A breaking change starts its line
  with `Breaking:` and says how to migrate.
- **The version and the tags belong to the release tool.** Only maintainers release, from `main`, with
  `cargo tools bump`. A pushed tag is never moved, deleted or reused: a bad release is fixed by the next patch
  release.
- **Dependencies stay few and reviewed.** `Cargo.lock` is committed, so every build uses the versions that were
  tested. A new dependency needs a reason in the pull request (what it does, why the standard library is not
  enough, its license, whether it is maintained). Dependency updates are their own pull request.
- **All platforms stay in step.** A change in `platform/`, `tap/` or behind a `cfg` is checked for the other
  systems too (`cargo tools ci` type-checks the Windows backend on macOS and Linux), and the manual checklists in
  `docs/WINDOWS_MIGRATION.md` and `docs/LINUX_MIGRATION.md` are run for what the change touches.
- **Old config files keep loading.** See *Config compatibility* under [Coding guidelines](#coding-guidelines).

## Commits and pull requests

**Commit messages.** The release tool turns commit subjects into changelog lines when nobody wrote notes, so a
subject is written for a reader of the changelog, in the imperative: `Fix double-click replay on slow clicks`.
The `commit-msg` hook rejects a message when:

- the subject is shorter than 10 or longer than 72 characters;
- it does not start with a capital letter, or ends with a full stop;
- it starts with a log word instead of an order (`Fixed`, `Adding`, `WIP`): write `Fix`, `Add`, `Update`;
- there is no blank line between the subject and the body.

Merges, reverts, `Release ...` commits and `fixup!` or `squash!` commits are not checked. The body is for the
*why*: what you tried, what you ruled out, what the reader must know.

**Pull requests.**

- Keep a pull request focused on one change. Describe *what* and *why*, and link the issue.
- Pull requests are squash-merged, so the title is the subject that stays in the history: write it by the same
  rules as a commit subject.
- Include tests for logic changes, and update the user docs when behavior or settings change.
- Say what you tested by hand for anything that tests cannot cover: gestures and UI (system version, trigger
  button, which apps) and anything that touches caching, sleep or undo (pie opening after a long idle time,
  Settings after sleep, Cmd+Z after deleting a slice, restoring a backup). The pull request template has a
  section for it.

## Coding guidelines

- **Format** with `rustfmt` defaults. Do not mix formatting-only changes into functional ones.
- **Comments explain *why*, not *what*.** Add a short comment where the reason is not obvious (a platform quirk,
  an ordering constraint, a lock rule). Do not narrate the code.
- **Document public items** with `///`, and every module with `//!`. Start with a one-line summary. Use
  intra-doc links (``[`Config`](crate::config::Config)``) rather than plain code names where an item exists. Add
  `# Panics`, `# Errors` or `# Safety` sections when they apply.
- **Write comments, docs, commit messages and identifiers in English.**
- **Keep the event tap safe** (details in `guides::architecture`): never block in the callback, never panic out of
  it, pass events through when unsure, release the gesture lock before sending messages, and keep the lock order
  *gesture, then config, then side state*.
- **Keep the Settings look consistent.** Colors, corner radii and spacing come from `src/ui/settings/theme.rs`
  (controls are 32 px high, the gap is 8 px), controls from `src/ui/settings/widgets.rs` (see `guides::extending`
  and `docs/UI_DESIGN.md`). No emoji on buttons.
- **Keep drawing sharp.** Never push a hand-made `egui::Mesh` for something with an edge: use
  `ui::draw::ring_slice` (anti-aliased) or an egui shape. Drive animations with `ui::draw::step_linear` plus one
  easing curve, not by feeding an exponential through an easing function. Test the geometry and math you add (see
  the tests in `src/ui/draw.rs`).
- **Bound every cache and give it a way out.** Anything the UI keeps between frames (textures, decoded images,
  lists read from disk) needs a size limit, and must be dropped in `App::enter_sleep` or when Settings closes (see
  `guides::architecture`, *Idle sleep and caches*). Never key a cache by unbounded input without eviction. Do not
  decode or read files on the frame that opens the pie: warm them in `App::warm`.
- **Draw only what is visible.** A list or grid that can pass a screenful uses `ScrollArea::show_rows`, never one widget per entry. Build the tiles from indexes, not clones of the strings, and load textures only for the visible rows, a few per frame (see `src/ui/settings/icon_picker.rs`).
- **Limit input where it enters, and tell the user.** Read an image's size from its header before decoding it and refuse what is too big (`ui::draw::MAX_SOURCE_SIDE`); read a text file with `Read::take`, not `read_to_string` without a limit (`config::read_text_capped`). Prefer shrinking or refusing with a message in the status bar (`Msg::Notice`) to failing silently, and do slow work on a worker thread (see `ui::iconfit`). Keep a decoded source image alive only as long as it takes to make the small one.
- **Build once, drop when hidden.** Data that does not change between frames is built when its panel opens, kept in `Ed`, and set back to `None` when the panel is not showing (see `Ed::add_catalog`). Do not allocate it again on every frame.
- **Keep Settings undoable.** Settings edits `App::edit`, and `History` watches it as a whole, so state that
  belongs in the config needs no extra code. State kept elsewhere (`Ed`) is not part of the history: make it
  tolerate a config that changed under it (see `Ed::fit_to`).
- **One code base, three systems.** Put OS calls in `src/platform/<os>/` (or the native hook glue
  `src/tap/macos.rs`, `src/tap/windows.rs`, `src/tap/linux.rs`) and give every backend the same function names.
  Everything above them takes an `Os` or portable types (`geometry::Point`, `tap::Ev`). Do not add `cfg`
  attributes to call sites. Keep config keys macOS virtual key codes and translate at the edge. Do not break one
  system while working on another: run the tests on each.
- **Keep logic testable.** Put decisions that need no OS API in plain functions and unit test them (see
  `side::resolve`, `config` tests, `appicon` tests). `cache`, `history`, `ui::sleep` and `config::backup` are
  written with no OS or egui types on purpose, and take the clock or the directory as a parameter instead of
  reading them. The tools in `tools/` follow the same rules as the app: standard library only, decisions in pure
  functions, every function documented and tested.
- **No new dependencies** without a good reason. The app is deliberately small and makes no network connections.
- **Config compatibility.** New settings need a `Default`; older `config.json` files must keep loading (see
  `guides::config_format`).
- **Security.** `Command` actions run shell commands with the user's permissions. Never run or import one
  silently.
- **Never edit the version by hand.** It lives in `Cargo.toml` and is raised only by the release tool, which also
  keeps `Cargo.lock` and `CHANGELOG.md` in step.

For step-by-step instructions on common changes (a new action kind, a setting, a Settings tab, icons...) see
`guides::extending`.

## Documentation

| Where | For whom | What |
|---|---|---|
| `README.md` | everyone | What CursorPie is, who it is for, quick start. Kept short on purpose |
| `docs/INSTALL.md`, `USER_GUIDE.md`, `CONFIGURATION.md`, `KNOWN_ISSUES.md` | users | Installing, using, configuring, limits |
| Rust docs (`cargo docs`, `src/guides/*.rs`) | developers | Architecture, config format, recipes for common changes. The source of truth for how the code works |
| `docs/WINDOWS_MIGRATION.md`, `LINUX_MIGRATION.md`, `OTHER_DESKTOPS.md`, `UI_DESIGN.md` | developers | Design records and manual test checklists |
| `docs/README.md` | everyone | An index of everything in `docs/` |
| `CHANGELOG.md` | users | What changed in each release |

The guides in `src/guides/` are ordinary `//!` comments. Start with `guides::architecture`: it explains the
threads, the data flow and the rules that keep the event tap safe. For where each module lives, read the module
map at the top of the crate docs (`src/lib.rs`) instead of a copy here.

Outside `src/`, the repository is laid out like this:

```text
tools/         the repository tools, run as `cargo tools ...`: ci, check, bump, hooks, commit-msg
.githooks/     Git hooks that call the tools (turned on with `cargo tools hooks install`)
.github/       the CI workflow and the pull request template
scripts/       packaging: bundle.sh and make-dmg.sh (macOS), package-windows.ps1, package-linux.sh
installer/     Inno Setup script of the Windows Setup wizard
packaging/     Linux desktop entry
icon/          the logo and sample icons (the in-app logo is drawn in code, see src/ui/settings/widgets.rs)
docs/          user documentation and design records
```

## Releasing

The version is raised by a tool, not by hand, so `Cargo.toml`, `Cargo.lock` and `CHANGELOG.md` can never
disagree. It uses only the standard library, so it runs on every OS:

```sh
cargo tools bump minor --dry-run   # 1. look at the plan: nothing is written
cargo tools bump minor             # 2. do it
cargo tools bump minor --commit    #    or do it, commit "Release X.Y.Z" and tag vX.Y.Z
cargo tools check                  # verify the files agree (also part of cargo tools ci)
```

### Which level

CursorPie uses `MAJOR.MINOR.PATCH`. Pick the level by what the *user* sees, not by how much code changed:

| Level | Use it for | Example |
|---|---|---|
| `patch` | fixes only: nothing new, nothing changed for the user | `0.3.0` -> `0.3.1` |
| `minor` | a new feature or setting, or changed behavior. **Below 1.0.0 a breaking change is also a minor release** | `0.3.1` -> `0.4.0` |
| `major` | from 1.0.0 on, a breaking change (a config that no longer loads, a removed feature); and the step to 1.0.0 itself | `0.4.0` -> `1.0.0` |

The tool only moves one step forward. It never goes back, never skips levels and never makes a pre-release
(`-beta`); a version that was released is never reused. The lower numbers reset to zero. The version of the config
file (`CONFIG_VERSION`) is a separate number with its own rule, see `guides::config_format`.

### What the tool does

- Raises `version` in `Cargo.toml`, and the `cursorpie` entry of `Cargo.lock` when the file exists. Everything
  else that shows the version reads `Cargo.toml`: `about::VERSION` (the About tab and the Windows "Apps &
  features" entry) and `scripts/bundle.sh` (the `.app` bundle).
- Writes the changelog entry as a `## X.Y.Z` section on top of `CHANGELOG.md`: first the notes you wrote under
  `## Unreleased` (the best source, written for readers); without them, the subjects of the commits since the last
  `v*` tag, minus merges and `Release ...` commits (use `--since REV` when there is no tag), flagged for review;
  with neither, it stops and changes nothing.
- Leaves a fresh, empty `## Unreleased` section on top of the changelog.
- With `--commit` it commits those files as `Release X.Y.Z` and tags `vX.Y.Z`. That needs a clean working tree,
  the `main` branch (`--any-branch` allows another one, for a rehearsal) and a tag that does not exist yet. It
  never pushes.
- Refuses to start when `Cargo.toml`, `Cargo.lock` and `CHANGELOG.md` already disagree, and writes nothing unless
  every file could be prepared. If a write fails halfway, the files already written are put back.

### What you still do by hand

The tool prints these at the end of every run, with a `WARNING:` where something is probably wrong right now:

1. **Read the changelog entry.** Always, and especially when it was generated.
2. **Remove sentences that name a version** in `README.md`. The tool finds a line containing the old version
   number after the word "Version" and tells you the line. The README should not have such sentences at all. Do
   not touch mentions of older versions in the history of `CHANGELOG.md` or `docs/`: they are facts.
3. **Decide on `CONFIG_VERSION`.** If this release changes `config.json` in a way an old reader would misread,
   raise it in `src/config/mod.rs` and write a migration. A new setting with a `Default` needs nothing.
4. **Check the user docs** for a `minor` or `major` release. The tool says "README.md" here; read it as the
   README and the pages in `docs/` that describe the changed behavior or settings.
5. **Build, test and publish.** Build on each OS, run the manual checklists, and push with
   `git push --follow-tags` (CI then checks that the tag matches the files). Attach to a GitHub release:
   `CursorPie-<version>-<arch>.dmg` (from `scripts/make-dmg.sh`, with its `.sha256`),
   `CursorPie-<version>-setup.exe`, `CursorPie-windows.zip` and `cursorpie-<version>-linux-<arch>.tar.gz`. Keep
   `BUNDLE_ID` equal in `src/about.rs` and `scripts/bundle.sh`.

If `check` reports a mismatch, someone changed the version by hand: undo that change and run the tool instead.

## Reporting bugs

Open an issue with: your system (macOS or Windows version; on Linux the distribution and whether it is X11 or
Wayland), how CursorPie was started (installed app or `cargo run`), the trigger button, what you did, what you
expected, and the output of `cargo run --release` from a terminal (CursorPie prints diagnostics to stderr).
Attach your `config.json` if it is relevant, after removing any commands you do not want to share.

## License

By contributing you agree that your contributions are licensed under the project's MIT License (see the `LICENSE`
file).
