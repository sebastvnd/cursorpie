//! `cargo tools install`: builds CursorPie from this checkout and installs it on a Linux machine,
//! whichever distribution it is, including the ones that have no package or no instructions yet.
//!
//! What it does, in order (each step can be left out, and `--dry-run` only prints them):
//!
//! 1. finds out the distribution (`/etc/os-release`: `ID`, then every word of `ID_LIKE`) and with it
//!    the package manager, or takes `--pm` when the distribution is not known;
//! 2. installs the C toolchain and the development files the build needs (libx11, libxi, libxtst,
//!    libxrandr; see `CONTRIBUTING.md`) and the libraries the window toolkit loads at run time. It
//!    skips this when `pkg-config` already finds the libraries;
//! 3. checks that Rust is as new as `rust-version` of `Cargo.toml` asks, and runs
//!    `cargo build --release`;
//! 4. copies the program, the desktop entry and the icons under a prefix (default `~/.local`, or
//!    `/usr/local` with `--system`), the same files `scripts/package-linux.sh` puts in its
//!    `install.sh`;
//! 5. installs the udev rule a Wayland session needs (default on a Wayland session, `--udev` forces
//!    it, `--no-udev` leaves it out).
//!
//! A distribution this file does not know gets the list of what to install, by the names the
//! libraries have everywhere, and a hint to run again with `--pm <manager>` and `--no-deps`. The
//! package names are the ones the distributions use at the time of writing and have not been
//! checked on every release of every one; a name that does not exist is reported by the package
//! manager itself, and the fix is one line in [`Manager::build_packages`].
//!
//! The decisions (which manager, which packages, which command, where each file goes) are pure
//! functions with unit tests; only [`run`] touches the machine. The tool uses the standard library
//! only, and does nothing on other systems than Linux.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::path::{Path, PathBuf};
#[cfg(target_os = "linux")]
use std::process::Command;
use std::process::ExitCode;

/// The libraries `pkg-config` must find for the build to work (the names of their `.pc` files).
const PKG_CONFIG_LIBS: [&str; 4] = ["x11", "xi", "xtst", "xrandr"];

/// A package manager the tool knows how to drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Manager {
    Apt,
    Dnf,
    Pacman,
    Zypper,
    Apk,
    Xbps,
    Emerge,
}

impl Manager {
    /// Every manager, for the usage text and the tests.
    pub const ALL: [Manager; 7] = [
        Manager::Apt,
        Manager::Dnf,
        Manager::Pacman,
        Manager::Zypper,
        Manager::Apk,
        Manager::Xbps,
        Manager::Emerge,
    ];

    /// The name `--pm` takes.
    pub fn name(self) -> &'static str {
        match self {
            Manager::Apt => "apt",
            Manager::Dnf => "dnf",
            Manager::Pacman => "pacman",
            Manager::Zypper => "zypper",
            Manager::Apk => "apk",
            Manager::Xbps => "xbps",
            Manager::Emerge => "emerge",
        }
    }

    /// The manager called `name` (`yum` and `microdnf` count as `dnf`, `xbps-install` as `xbps`).
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "apt" | "apt-get" => Some(Manager::Apt),
            "dnf" | "yum" | "microdnf" => Some(Manager::Dnf),
            "pacman" => Some(Manager::Pacman),
            "zypper" => Some(Manager::Zypper),
            "apk" => Some(Manager::Apk),
            "xbps" | "xbps-install" => Some(Manager::Xbps),
            "emerge" | "portage" => Some(Manager::Emerge),
            _ => None,
        }
    }

    /// What the build needs: a C linker, `pkg-config` and the development files of the X libraries.
    pub fn build_packages(self) -> &'static [&'static str] {
        match self {
            Manager::Apt => &[
                "build-essential",
                "pkg-config",
                "libx11-dev",
                "libxi-dev",
                "libxtst-dev",
                "libxrandr-dev",
            ],
            Manager::Dnf => &[
                "gcc",
                "pkgconf-pkg-config",
                "libX11-devel",
                "libXi-devel",
                "libXtst-devel",
                "libXrandr-devel",
            ],
            Manager::Pacman => &[
                "base-devel",
                "pkgconf",
                "libx11",
                "libxi",
                "libxtst",
                "libxrandr",
            ],
            Manager::Zypper => &[
                "gcc",
                "pkg-config",
                "libX11-devel",
                "libXi-devel",
                "libXtst-devel",
                "libXrandr-devel",
            ],
            Manager::Apk => &[
                "build-base",
                "pkgconf",
                "libx11-dev",
                "libxi-dev",
                "libxtst-dev",
                "libxrandr-dev",
            ],
            Manager::Xbps => &[
                "base-devel",
                "pkg-config",
                "libX11-devel",
                "libXi-devel",
                "libXtst-devel",
                "libXrandr-devel",
            ],
            Manager::Emerge => &[
                "x11-libs/libX11",
                "x11-libs/libXi",
                "x11-libs/libXtst",
                "x11-libs/libXrandr",
                "dev-util/pkgconf",
            ],
        }
    }

    /// What the program loads when it runs: OpenGL, libxkbcommon-x11 and libxcursor.
    pub fn runtime_packages(self) -> &'static [&'static str] {
        match self {
            Manager::Apt => &["libgl1", "libxkbcommon-x11-0", "libxcursor1"],
            Manager::Dnf => &["mesa-libGL", "libxkbcommon-x11", "libXcursor"],
            Manager::Pacman => &["mesa", "libxkbcommon-x11", "libxcursor"],
            Manager::Zypper => &["Mesa-libGL1", "libxkbcommon-x11-0", "libXcursor1"],
            Manager::Apk => &["mesa-gl", "libxkbcommon-x11", "libxcursor"],
            Manager::Xbps => &["MesaLib", "libxkbcommon", "libXcursor"],
            Manager::Emerge => &[
                "media-libs/mesa",
                "x11-libs/libxkbcommon",
                "x11-libs/libXcursor",
            ],
        }
    }

    /// The command (without `sudo`) that installs `packages`, answering yes by itself when `yes`.
    pub fn install_command(self, packages: &[&str], yes: bool) -> Vec<String> {
        let mut cmd: Vec<&str> = match self {
            Manager::Apt => vec!["apt-get", "install"],
            Manager::Dnf => vec!["dnf", "install"],
            Manager::Pacman => vec!["pacman", "-S", "--needed"],
            Manager::Zypper => vec!["zypper", "install"],
            Manager::Apk => vec!["apk", "add"],
            Manager::Xbps => vec!["xbps-install", "-S"],
            Manager::Emerge => vec!["emerge", "--noreplace"],
        };
        if yes {
            match self {
                Manager::Apt | Manager::Dnf | Manager::Zypper => cmd.push("-y"),
                Manager::Pacman => cmd.push("--noconfirm"),
                Manager::Xbps => cmd.push("-y"),
                Manager::Apk | Manager::Emerge => {}
            }
        }
        cmd.extend(packages);
        cmd.into_iter().map(str::to_owned).collect()
    }

    /// The command that refreshes the package lists first, for the managers that need it.
    pub fn refresh_command(self) -> Option<Vec<String>> {
        match self {
            Manager::Apt => Some(vec!["apt-get".into(), "update".into()]),
            _ => None,
        }
    }
}

/// The lines of `/etc/os-release` this tool reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OsRelease {
    /// `ID`: `fedora`, `ubuntu`, `arch`, ...
    pub id: String,
    /// The words of `ID_LIKE`: the families the distribution comes from.
    pub like: Vec<String>,
    /// `PRETTY_NAME`, for the messages.
    pub pretty: String,
}

/// Reads the text of an `os-release` file (`KEY=value` or `KEY="value"` per line).
pub fn parse_os_release(text: &str) -> OsRelease {
    let mut out = OsRelease::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim_matches('\'');
        match key.trim() {
            "ID" => out.id = value.to_ascii_lowercase(),
            "ID_LIKE" => {
                out.like = value
                    .split_whitespace()
                    .map(str::to_ascii_lowercase)
                    .collect();
            }
            "PRETTY_NAME" => out.pretty = value.to_owned(),
            _ => {}
        }
    }
    out
}

/// The manager of a distribution, looked up by its own `ID` first and then by every family it says
/// it is like. `None` for a distribution the tool does not know (NixOS, Guix, a new one).
pub fn manager_for(os: &OsRelease) -> Option<Manager> {
    std::iter::once(&os.id)
        .chain(os.like.iter())
        .find_map(|id| manager_of_id(id))
}

fn manager_of_id(id: &str) -> Option<Manager> {
    let manager = match id {
        "debian" | "ubuntu" | "linuxmint" | "pop" | "raspbian" | "kali" | "elementary"
        | "zorin" | "neon" | "devuan" | "mx" | "deepin" | "parrot" => Manager::Apt,
        "fedora" | "rhel" | "centos" | "rocky" | "almalinux" | "ol" | "nobara" | "amzn"
        | "bazzite" | "ultramarine" => Manager::Dnf,
        "arch" | "manjaro" | "endeavouros" | "cachyos" | "garuda" | "artix" | "arcolinux" => {
            Manager::Pacman
        }
        "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" | "sles" | "suse" => Manager::Zypper,
        "alpine" | "postmarketos" => Manager::Apk,
        "void" => Manager::Xbps,
        "gentoo" | "funtoo" => Manager::Emerge,
        _ => return None,
    };
    Some(manager)
}

/// Where the program goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prefix {
    /// `~/.local`: no administrator rights for the files.
    User,
    /// `/usr/local`.
    System,
    /// A folder of the person's choice.
    Dir(PathBuf),
}

impl Prefix {
    /// The folder, given the home folder.
    pub fn dir(&self, home: &Path) -> PathBuf {
        match self {
            Prefix::User => home.join(".local"),
            Prefix::System => PathBuf::from("/usr/local"),
            Prefix::Dir(d) => d.clone(),
        }
    }
}

/// What the person asked `install` to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub prefix: Prefix,
    /// The package manager to use instead of the one the distribution has.
    pub pm: Option<Manager>,
    /// Leave out the installation of packages.
    pub no_deps: bool,
    /// Only install the packages (no build, no files).
    pub deps_only: bool,
    /// Leave out the build: install what `target/release/cursorpie` already is.
    pub no_build: bool,
    /// `Some(true)`: install the udev rule, `Some(false)`: do not, `None`: on a Wayland session.
    pub udev: Option<bool>,
    /// Answer yes to the questions and to the package manager.
    pub yes: bool,
    /// Print the steps and do none of them.
    pub dry_run: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            prefix: Prefix::User,
            pm: None,
            no_deps: false,
            deps_only: false,
            no_build: false,
            udev: None,
            yes: false,
            dry_run: false,
        }
    }
}

/// Reads the arguments after `install`.
pub fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--user" => o.prefix = Prefix::User,
            "--system" => o.prefix = Prefix::System,
            "--prefix" => {
                let dir = it.next().ok_or("--prefix needs a folder")?;
                o.prefix = Prefix::Dir(PathBuf::from(dir));
            }
            "--pm" => {
                let name = it.next().ok_or("--pm needs a package manager")?;
                o.pm = Some(Manager::from_name(name).ok_or_else(|| {
                    let names: Vec<&str> = Manager::ALL.iter().map(|m| m.name()).collect();
                    format!(
                        "unknown package manager '{name}' (known: {})",
                        names.join(", ")
                    )
                })?);
            }
            "--no-deps" => o.no_deps = true,
            "--deps-only" => o.deps_only = true,
            "--no-build" => o.no_build = true,
            "--udev" => o.udev = Some(true),
            "--no-udev" => o.udev = Some(false),
            "--yes" | "-y" => o.yes = true,
            "--dry-run" => o.dry_run = true,
            other => return Err(format!("unknown option '{other}' for install")),
        }
    }
    if o.no_deps && o.deps_only {
        return Err("--no-deps and --deps-only cancel each other".into());
    }
    Ok(o)
}

/// The numbers of a version such as `rustc 1.90.0 (abc 2025-09-14)` or `1.90`: major, minor.
pub fn parse_version(text: &str) -> Option<(u32, u32)> {
    let word = text
        .split_whitespace()
        .find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))?;
    let mut parts = word.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts
        .next()?
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()?;
    Some((major, minor))
}

/// Whether the compiler `have` (the output of `rustc --version`) is at least `need`.
pub fn rust_is_new_enough(have: &str, need: &str) -> bool {
    match (parse_version(have), parse_version(need)) {
        (Some(h), Some(n)) => h >= n,
        _ => false,
    }
}

/// The `rust-version` of a `Cargo.toml`.
pub fn rust_version_of(cargo_toml: &str) -> Option<String> {
    cargo_toml.lines().find_map(|l| {
        let (key, value) = l.split_once('=')?;
        (key.trim() == "rust-version").then(|| value.trim().trim_matches('"').to_owned())
    })
}

/// One file to copy: where it is, where it goes and its mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placed {
    pub from: PathBuf,
    pub to: PathBuf,
    pub mode: u32,
}

/// The files of an installation under `prefix`. `icons` are the file names of `assets/icons`
/// (`cursorpie-128.png`, ...); the ones that do not look like an icon are left out.
pub fn files(root: &Path, prefix: &Path, icons: &[String]) -> Vec<Placed> {
    let mut out = vec![
        Placed {
            from: root.join("target/release/cursorpie"),
            to: prefix.join("bin/cursorpie"),
            mode: 0o755,
        },
        Placed {
            from: root.join("packaging/linux/cursorpie.desktop"),
            to: prefix.join("share/applications/cursorpie.desktop"),
            mode: 0o644,
        },
    ];
    for name in icons {
        let Some(size) = name
            .strip_prefix("cursorpie-")
            .and_then(|s| s.strip_suffix(".png"))
            .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
        else {
            continue;
        };
        out.push(Placed {
            from: root.join("assets/icons").join(name),
            to: prefix.join(format!(
                "share/icons/hicolor/{size}x{size}/apps/cursorpie.png"
            )),
            mode: 0o644,
        });
    }
    out
}

/// The command that gains the rights to run `cmd`: nothing when already administrator, `sudo` or
/// `doas` otherwise. `None` when there is no way to.
pub fn elevate(cmd: Vec<String>, is_root: bool, tool: Option<&str>) -> Option<Vec<String>> {
    if is_root {
        return Some(cmd);
    }
    let tool = tool?;
    let mut v = vec![tool.to_owned()];
    v.extend(cmd);
    Some(v)
}

/// The text of what to install by hand, for a machine whose package manager is not known: the
/// libraries by the names they have in every distribution.
pub fn manual_list() -> String {
    let mut s = String::from(
        "CursorPie needs, to build: a C compiler and linker, pkg-config, and the development files\n\
         (the -dev / -devel package) of libX11, libXi, libXtst and libXrandr.\n\
         To run: OpenGL (Mesa), libxkbcommon-x11 and libXcursor.\n\
         Install them with your package manager, then run again with --no-deps. If your package\n\
         manager is one of these, name it with --pm and the tool installs them for you: ",
    );
    let names: Vec<&str> = Manager::ALL.iter().map(|m| m.name()).collect();
    s.push_str(&names.join(", "));
    s.push('.');
    s
}

/// Runs the installation described by `opts` in the checkout `root`.
#[cfg(target_os = "linux")]
pub fn run(root: &Path, opts: &Options) -> Result<ExitCode, String> {
    let plan = Runner { opts };
    plan.go(root)
}

/// Other systems have their own ways to install: see `scripts/` and `docs/user/install.md`.
#[cfg(not(target_os = "linux"))]
pub fn run(_root: &Path, _opts: &Options) -> Result<ExitCode, String> {
    Err("install is for Linux. On macOS use scripts/bundle.sh, on Windows scripts\\package-windows.ps1".into())
}

#[cfg(target_os = "linux")]
struct Runner<'a> {
    opts: &'a Options,
}

#[cfg(target_os = "linux")]
impl Runner<'_> {
    fn go(&self, root: &Path) -> Result<ExitCode, String> {
        let os = std::fs::read_to_string("/etc/os-release")
            .or_else(|_| std::fs::read_to_string("/usr/lib/os-release"))
            .map(|t| parse_os_release(&t))
            .unwrap_or_default();
        let manager = self.opts.pm.or_else(|| manager_for(&os));
        let shown = if os.pretty.is_empty() {
            "unknown"
        } else {
            os.pretty.as_str()
        };
        println!("System: {shown}");

        if !self.opts.no_deps {
            self.dependencies(manager)?;
        }
        if self.opts.deps_only {
            return Ok(ExitCode::SUCCESS);
        }
        if !self.opts.no_build {
            self.build(root)?;
        }
        self.copy_files(root)?;
        if self.wants_udev() {
            self.udev(root)?;
        }
        println!("\nDone. Start CursorPie from the applications menu, or run: cursorpie");
        let home = home_dir()?;
        let bin = self.opts.prefix.dir(&home).join("bin");
        if !path_has(&bin) {
            println!(
                "{} is not in your PATH: add it, or start the program with its full path.",
                bin.display()
            );
        }
        println!(
            "GNOME with Wayland? For per-app profiles also install the extension in packaging/linux/gnome-extension (docs/user/install.md)."
        );
        Ok(ExitCode::SUCCESS)
    }

    fn dependencies(&self, manager: Option<Manager>) -> Result<(), String> {
        if libraries_present() && have("cc") {
            println!(
                "Libraries: pkg-config finds them and a C compiler is there, nothing to install."
            );
            return Ok(());
        }
        let Some(manager) = manager else {
            println!(
                "\nThis distribution is not known to the tool.\n{}",
                manual_list()
            );
            return Err("could not install the libraries; install them by hand and run again with --no-deps".into());
        };
        let mut packages: Vec<&str> = manager.build_packages().to_vec();
        packages.extend(manager.runtime_packages());
        println!("Packages ({}): {}", manager.name(), packages.join(" "));
        let mut steps = Vec::new();
        steps.extend(manager.refresh_command());
        steps.push(manager.install_command(&packages, self.opts.yes));
        for step in steps {
            let cmd = elevate(step, is_root(), admin_tool()).ok_or(
                "this needs administrator rights and neither sudo nor doas is installed: run it as root",
            )?;
            self.exec(&cmd, true)?;
        }
        Ok(())
    }

    fn build(&self, root: &Path) -> Result<(), String> {
        let have = output_of("rustc", &["--version"]).ok_or(
            "rustc was not found: install Rust with rustup (https://rustup.rs) and open a new terminal",
        )?;
        let need = std::fs::read_to_string(root.join("Cargo.toml"))
            .ok()
            .and_then(|t| rust_version_of(&t));
        if let Some(need) = need.filter(|n| !rust_is_new_enough(&have, n)) {
            return Err(format!(
                "{have} is older than the {need} this version needs: run `rustup update stable`"
            ));
        }
        println!("\nBuilding ({have}) ...");
        let cmd: Vec<String> = ["cargo", "build", "--release"].map(str::to_owned).to_vec();
        self.exec_in(&cmd, root, false)
    }

    fn copy_files(&self, root: &Path) -> Result<(), String> {
        let home = home_dir()?;
        let prefix = self.opts.prefix.dir(&home);
        let icons: Vec<String> = std::fs::read_dir(root.join("assets/icons"))
            .map(|d| {
                d.filter_map(Result::ok)
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
        println!("\nInstalling under {} ...", prefix.display());
        let needs_root = !self.opts.dry_run && !can_write(&prefix);
        for f in files(root, &prefix, &icons) {
            println!("  {}", f.to.display());
            if self.opts.dry_run {
                continue;
            }
            if needs_root {
                let mode = format!("{:o}", f.mode);
                let cmd = elevate(
                    vec!["install".into(), format!("-Dm{mode}"), f.from.display().to_string(), f.to.display().to_string()],
                    is_root(),
                    admin_tool(),
                )
                .ok_or("writing there needs administrator rights and neither sudo nor doas is installed")?;
                self.exec(&cmd, false)?;
            } else {
                copy_file(&f)?;
            }
        }
        if !self.opts.dry_run {
            // each is optional: they only make the desktop notice the new entry and icon at once
            for tool in [
                vec!["gtk-update-icon-cache", "-q", "-t", "-f"],
                vec!["update-desktop-database", "-q"],
            ] {
                let dir = if tool[0].starts_with("gtk") {
                    prefix.join("share/icons/hicolor")
                } else {
                    prefix.join("share/applications")
                };
                let _ = Command::new(tool[0]).args(&tool[1..]).arg(dir).status();
            }
        }
        Ok(())
    }

    fn wants_udev(&self) -> bool {
        self.opts.udev.unwrap_or_else(|| {
            std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v.eq_ignore_ascii_case("wayland"))
        })
    }

    fn udev(&self, root: &Path) -> Result<(), String> {
        let rule = root.join("packaging/linux/70-cursorpie.rules");
        println!("\nUdev rule for Wayland sessions -> /etc/udev/rules.d/70-cursorpie.rules");
        let steps: Vec<Vec<String>> = vec![
            vec![
                "install".into(),
                "-Dm644".into(),
                rule.display().to_string(),
                "/etc/udev/rules.d/70-cursorpie.rules".into(),
            ],
            vec!["udevadm".into(), "control".into(), "--reload".into()],
            vec!["udevadm".into(), "trigger".into()],
        ];
        for step in steps {
            let cmd = elevate(step, is_root(), admin_tool()).ok_or(
                "the udev rule needs administrator rights and neither sudo nor doas is installed",
            )?;
            self.exec(&cmd, true)?;
        }
        println!("Log out and in once for the rule to apply to you.");
        Ok(())
    }

    fn exec(&self, cmd: &[String], ask: bool) -> Result<(), String> {
        self.exec_in(cmd, Path::new("."), ask)
    }

    /// Prints a command and, unless `--dry-run`, runs it (asking first when `ask` and not `--yes`).
    fn exec_in(&self, cmd: &[String], dir: &Path, ask: bool) -> Result<(), String> {
        println!("$ {}", cmd.join(" "));
        if self.opts.dry_run {
            return Ok(());
        }
        if ask && !self.opts.yes && !confirm("Run it?") {
            return Err("stopped: nothing more was done".into());
        }
        let status = Command::new(&cmd[0])
            .args(&cmd[1..])
            .current_dir(dir)
            .status()
            .map_err(|e| format!("could not start {}: {e}", cmd[0]))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("`{}` failed ({status})", cmd.join(" ")))
        }
    }
}

#[cfg(target_os = "linux")]
fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set".to_owned())
}

#[cfg(target_os = "linux")]
fn path_has(dir: &Path) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d == dir))
}

/// Whether a program can be started at all.
#[cfg(target_os = "linux")]
fn have(program: &str) -> bool {
    Command::new(program).arg("--version").output().is_ok()
}

#[cfg(target_os = "linux")]
fn output_of(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    out.status.success().then(|| {
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_owned()
    })
}

#[cfg(target_os = "linux")]
fn is_root() -> bool {
    output_of("id", &["-u"]).is_some_and(|u| u == "0")
}

#[cfg(target_os = "linux")]
fn admin_tool() -> Option<&'static str> {
    ["sudo", "doas"].into_iter().find(|t| have_admin(t))
}

#[cfg(target_os = "linux")]
fn have_admin(tool: &str) -> bool {
    // `doas` has no --version: starting it at all is the test
    Command::new(tool).arg("--help").output().is_ok()
}

/// Whether `pkg-config` finds the X libraries.
#[cfg(target_os = "linux")]
fn libraries_present() -> bool {
    Command::new("pkg-config")
        .arg("--exists")
        .args(PKG_CONFIG_LIBS)
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(target_os = "linux")]
fn can_write(prefix: &Path) -> bool {
    // the nearest folder that exists: the one the first new folder would be made in
    let mut dir = prefix;
    while !dir.exists() {
        match dir.parent() {
            Some(p) => dir = p,
            None => return false,
        }
    }
    Command::new("test")
        .arg("-w")
        .arg(dir)
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(target_os = "linux")]
fn copy_file(f: &Placed) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(dir) = f.to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    // a running program cannot be overwritten in place: remove it first
    let _ = std::fs::remove_file(&f.to);
    std::fs::copy(&f.from, &f.to).map_err(|e| format!("{}: {e}", f.from.display()))?;
    std::fs::set_permissions(&f.to, std::fs::Permissions::from_mode(f.mode))
        .map_err(|e| format!("{}: {e}", f.to.display()))
}

#[cfg(target_os = "linux")]
fn confirm(question: &str) -> bool {
    use std::io::Write;
    print!("{question} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).is_ok() && matches!(line.trim(), "y" | "Y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(text: &str) -> OsRelease {
        parse_os_release(text)
    }

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn os_release_is_read_with_and_without_quotes() {
        let o = os("NAME=\"Fedora Linux\"\nID=fedora\nPRETTY_NAME=\"Fedora Linux 44\"\n");
        assert_eq!(
            (o.id.as_str(), o.pretty.as_str()),
            ("fedora", "Fedora Linux 44")
        );
        let o = os("ID=\"linuxmint\"\nID_LIKE=\"ubuntu debian\"\n");
        assert_eq!(o.id, "linuxmint");
        assert_eq!(o.like, ["ubuntu", "debian"]);
        assert_eq!(os(""), OsRelease::default());
    }

    #[test]
    fn a_distribution_is_known_by_its_id_or_by_its_family() {
        let m = |t: &str| manager_for(&os(t));
        assert_eq!(m("ID=ubuntu"), Some(Manager::Apt));
        assert_eq!(m("ID=fedora"), Some(Manager::Dnf));
        assert_eq!(m("ID=arch"), Some(Manager::Pacman));
        assert_eq!(m("ID=opensuse-tumbleweed"), Some(Manager::Zypper));
        assert_eq!(m("ID=alpine"), Some(Manager::Apk));
        assert_eq!(m("ID=void"), Some(Manager::Xbps));
        assert_eq!(m("ID=gentoo"), Some(Manager::Emerge));
        // one the list does not have, that says what it is like
        assert_eq!(
            m("ID=somethingnew\nID_LIKE=\"rhel centos fedora\""),
            Some(Manager::Dnf)
        );
        assert_eq!(m("ID=myarch\nID_LIKE=arch"), Some(Manager::Pacman));
        // its own id wins over the family
        assert_eq!(m("ID=nobara\nID_LIKE=\"rhel fedora\""), Some(Manager::Dnf));
        // NixOS and a stranger are not guessed
        assert_eq!(m("ID=nixos"), None);
        assert_eq!(m("ID=unheardof"), None);
    }

    #[test]
    fn every_manager_has_a_name_that_reads_back_and_packages_for_the_x_libraries() {
        for m in Manager::ALL {
            assert_eq!(Manager::from_name(m.name()), Some(m));
            let all = m.build_packages().join(" ").to_ascii_lowercase();
            for lib in ["x11", "xi", "xtst", "xrandr"] {
                assert!(all.contains(lib), "{}: {lib} in {all}", m.name());
            }
            assert!(!m.runtime_packages().is_empty());
        }
        assert_eq!(Manager::from_name("YUM"), Some(Manager::Dnf));
        assert_eq!(Manager::from_name("nix"), None);
    }

    #[test]
    fn the_install_commands_answer_yes_only_when_asked() {
        let pk = ["a", "b"];
        assert_eq!(
            Manager::Apt.install_command(&pk, false),
            ["apt-get", "install", "a", "b"]
        );
        assert_eq!(
            Manager::Apt.install_command(&pk, true),
            ["apt-get", "install", "-y", "a", "b"]
        );
        assert_eq!(
            Manager::Pacman.install_command(&pk, true),
            ["pacman", "-S", "--needed", "--noconfirm", "a", "b"]
        );
        assert_eq!(
            Manager::Dnf.install_command(&pk, true),
            ["dnf", "install", "-y", "a", "b"]
        );
        assert_eq!(
            Manager::Apk.install_command(&pk, true),
            ["apk", "add", "a", "b"]
        );
        assert!(Manager::Apt.refresh_command().is_some());
        assert!(Manager::Dnf.refresh_command().is_none());
    }

    #[test]
    fn the_arguments_are_read_and_wrong_ones_are_named() {
        assert_eq!(parse_args(&[]), Ok(Options::default()));
        let o = parse_args(&args(&[
            "--system",
            "--pm",
            "zypper",
            "--yes",
            "--dry-run",
            "--no-udev",
        ]))
        .unwrap();
        assert_eq!(o.prefix, Prefix::System);
        assert_eq!(o.pm, Some(Manager::Zypper));
        assert!(o.yes && o.dry_run);
        assert_eq!(o.udev, Some(false));
        let o = parse_args(&args(&["--prefix", "/opt/cp", "--udev", "--no-build"])).unwrap();
        assert_eq!(o.prefix, Prefix::Dir("/opt/cp".into()));
        assert_eq!((o.udev, o.no_build), (Some(true), true));
        assert!(parse_args(&args(&["--prefix"])).is_err());
        assert!(parse_args(&args(&["--pm"])).is_err());
        assert!(
            parse_args(&args(&["--pm", "nix"]))
                .unwrap_err()
                .contains("known:")
        );
        assert!(parse_args(&args(&["--nope"])).is_err());
        assert!(parse_args(&args(&["--no-deps", "--deps-only"])).is_err());
    }

    #[test]
    fn versions_are_compared_by_major_and_minor() {
        assert_eq!(
            parse_version("rustc 1.90.0 (abc 2025-09-14)"),
            Some((1, 90))
        );
        assert_eq!(parse_version("1.88"), Some((1, 88)));
        assert_eq!(parse_version("rustc 1.91.0-nightly (x)"), Some((1, 91)));
        assert_eq!(parse_version("not found"), None);
        assert!(rust_is_new_enough("rustc 1.90.1 (x)", "1.90"));
        assert!(rust_is_new_enough("rustc 1.100.0 (x)", "1.90"));
        assert!(!rust_is_new_enough("rustc 1.88.0 (x)", "1.90"));
        assert!(!rust_is_new_enough("not found", "1.90"));
    }

    #[test]
    fn the_rust_version_is_the_one_of_cargo_toml() {
        let toml = "[package]\nname = \"cursorpie\"\nrust-version = \"1.90\"\nedition = \"2024\"\n";
        assert_eq!(rust_version_of(toml).as_deref(), Some("1.90"));
        assert_eq!(rust_version_of("[package]\nname = \"x\"\n"), None);
    }

    #[test]
    fn the_files_go_under_the_prefix_and_only_real_icons_are_copied() {
        let icons = args(&[
            "cursorpie-16.png",
            "cursorpie-512.png",
            "cursorpie-mark.png",
            "other.png",
            "cursorpie-.png",
        ]);
        let list = files(Path::new("/src"), Path::new("/home/a/.local"), &icons);
        // compared as paths, not text: Windows joins with `\\`
        let to: Vec<&Path> = list.iter().map(|c| c.to.as_path()).collect();
        assert_eq!(
            to,
            [
                "/home/a/.local/bin/cursorpie",
                "/home/a/.local/share/applications/cursorpie.desktop",
                "/home/a/.local/share/icons/hicolor/16x16/apps/cursorpie.png",
                "/home/a/.local/share/icons/hicolor/512x512/apps/cursorpie.png",
            ]
            .map(Path::new)
        );
        assert_eq!(list[0].mode, 0o755);
        assert_eq!(list[0].from, Path::new("/src/target/release/cursorpie"));
        assert_eq!(
            list[2].from,
            Path::new("/src/assets/icons/cursorpie-16.png")
        );
    }

    #[test]
    fn the_prefixes_resolve() {
        let home = Path::new("/home/a");
        assert_eq!(Prefix::User.dir(home), Path::new("/home/a/.local"));
        assert_eq!(Prefix::System.dir(home), Path::new("/usr/local"));
        assert_eq!(Prefix::Dir("/opt/x".into()).dir(home), Path::new("/opt/x"));
    }

    #[test]
    fn administrator_rights_are_asked_for_only_when_missing() {
        let cmd = || vec!["dnf".to_owned(), "install".to_owned()];
        assert_eq!(elevate(cmd(), true, None), Some(cmd()));
        assert_eq!(
            elevate(cmd(), false, Some("sudo")),
            Some(args(&["sudo", "dnf", "install"]))
        );
        assert_eq!(
            elevate(cmd(), false, Some("doas")),
            Some(args(&["doas", "dnf", "install"]))
        );
        assert_eq!(elevate(cmd(), false, None), None);
    }

    #[test]
    fn the_manual_list_names_the_libraries_and_every_manager() {
        let t = manual_list();
        for word in [
            "libX11",
            "libXi",
            "libXtst",
            "libXrandr",
            "--pm",
            "--no-deps",
        ] {
            assert!(t.contains(word), "{word}");
        }
        for m in Manager::ALL {
            assert!(t.contains(m.name()), "{}", m.name());
        }
    }
}
