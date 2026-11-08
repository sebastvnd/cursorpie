//! Installing the newest release: finding the right package, downloading it, checking it and
//! swapping it in. The same steps on macOS, Windows and Linux; only the last two differ.
//!
//! | Step | What happens |
//! |---|---|
//! | choose | the release's package for this system: [`asset_name`] |
//! | download | with the system's `curl`, from the release's own address, never from an address in the answer |
//! | check | the size and the SHA-256 that GitHub publishes for the file ([`verify`]); with no checksum nothing is installed |
//! | stage | macOS: the app is copied out of the disk image next to the installed one; Linux: the program is unpacked from the archive next to the installed one; both are started once with `--selftest`; Windows: the Setup program is kept as it is |
//! | swap | after CursorPie quit: macOS and Linux replace the old copy and start the new one (`APPLY_SCRIPT`); Windows runs Setup silently, which replaces the files and starts the new one |
//!
//! Everything up to *stage* runs while CursorPie keeps working, on a worker thread, and any failure
//! leaves the installed copy exactly as it was. Only an ordinary install is updated: a copy run from
//! a build folder, from an unzipped Windows download or from a folder the user cannot write to is
//! left alone and the person is sent to the release page (see [`InstallError`]).

use super::{CHECK_TIMEOUT_S, Failure, MAX_ANSWER_BYTES, Version, sha256};
use crate::about::{APP_NAME, LATEST_RELEASE_API_URL, RELEASES_URL, SELFTEST_ARG};
use crate::config::app_support_dir;
use crate::msg::{Msg, Notifier};
use crate::platform::http::{self, HttpError};
use crate::platform::{self, Os};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Longest a download takes before it is given up, in seconds.
const DOWNLOAD_TIMEOUT_S: u64 = 600;

/// Biggest package that is downloaded. The packages are a few MiB.
const MAX_PACKAGE_BYTES: u64 = 200 * 1024 * 1024;

/// Biggest checksum file that is read.
const MAX_CHECKSUM_BYTES: u64 = 4 * 1024;

/// Folder of the downloaded package, inside the app support directory. Emptied before every download.
const UPDATES_DIR: &str = "updates";

/// What the swap leaves next to the installed copy while it works.
const NEW_SUFFIX: &str = ".cursorpie-new";

/// How Setup is started on Windows: silent, no message boxes, no restart of the PC, the "start at
/// login" task left as the person has it, and the new program started when it is done
/// (`installer/cursorpie.iss` reads `RESTARTAPP`).
const SETUP_ARGS: [&str; 5] = [
    "/VERYSILENT",
    "/SUPPRESSMSGBOXES",
    "/NORESTART",
    "/MERGETASKS=!startup",
    "/RESTARTAPP=1",
];

/// The swap on macOS and Linux, run by `/bin/sh` after CursorPie quit. Arguments: the process id to
/// wait for, the staged copy, the installed copy and the mode (`mac` or anything else). The installed
/// copy is moved aside first and moved back when the new one cannot take its place, so the person
/// always has a program to start; the program is started in every case but "the old one is still
/// running".
const APPLY_SCRIPT: &str = r#"#!/bin/sh
pid=$1
staged=$2
target=$3
mode=$4
n=0
while kill -0 "$pid" 2>/dev/null && [ "$n" -lt 100 ]; do
    sleep 0.3
    n=$((n + 1))
done
if kill -0 "$pid" 2>/dev/null; then
    rm -rf "$staged"
    exit 1
fi
old="$target.cursorpie-old"
rm -rf "$old"
if mv "$target" "$old"; then
    if mv "$staged" "$target"; then
        rm -rf "$old"
    else
        mv "$old" "$target"
    fi
fi
rm -rf "$staged"
case "$mode" in
    mac) open "$target" ;;
    *) nohup "$target" >/dev/null 2>&1 & ;;
esac
"#;

// ---------------------------------------------------------------------------- the release

/// One file attached to a release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    /// The file name, letters, digits, `.`, `_` and `-` only.
    pub name: String,
    /// Its size in bytes (0 = not said).
    pub size: u64,
    /// The SHA-256 GitHub publishes for it, 64 lower-case hex digits.
    pub digest: Option<String>,
}

/// The newest stable release as far as an update needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// The tag, `v` and the version, or the version alone: the only forms an address is made from.
    pub tag: String,
    pub assets: Vec<Asset>,
}

/// A name that is safe in a file path and in an address.
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// 64 hex digits, lower-cased.
fn hex64(text: &str) -> Option<String> {
    let t = text.trim();
    (t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit())).then(|| t.to_ascii_lowercase())
}

/// Reads the answer of GitHub's "latest release". `None` for a draft, a pre-release, a tag that is
/// not `vX.Y.Z` or `X.Y.Z`, or text that is not JSON. Assets with an unsafe name are left out.
pub fn parse_release(json: &str) -> Option<Release> {
    let answer: serde_json::Value = serde_json::from_str(json).ok()?;
    let flagged = |key: &str| answer.get(key).and_then(serde_json::Value::as_bool) == Some(true);
    if flagged("draft") || flagged("prerelease") {
        return None;
    }
    let tag = answer.get("tag_name")?.as_str()?.to_string();
    let version = Version::parse(&tag)?;
    if tag != format!("v{version}") && tag != version.to_string() {
        return None;
    }
    let mut assets = Vec::new();
    let listed = answer.get("assets").and_then(serde_json::Value::as_array);
    for item in listed.into_iter().flatten() {
        let Some(name) = item.get("name").and_then(|n| n.as_str()) else {
            continue;
        };
        if !plain_name(name) {
            continue;
        }
        let size = item.get("size").and_then(|s| s.as_u64()).unwrap_or(0);
        let digest = item
            .get("digest")
            .and_then(|d| d.as_str())
            .and_then(|d| d.strip_prefix("sha256:"))
            .and_then(hex64);
        assets.push(Asset {
            name: name.to_string(),
            size,
            digest,
        });
    }
    Some(Release {
        version,
        tag,
        assets,
    })
}

/// The address of a file of `release`: GitHub's own download address for its tag. Made here, never
/// taken from the answer, so the answer cannot point the download anywhere else.
pub fn asset_url(release: &Release, name: &str) -> String {
    format!("{RELEASES_URL}/download/{}/{name}", release.tag)
}

/// The checksum in a `.sha256` file as `shasum` writes it: the hash, then the file name.
pub fn parse_checksum_file(text: &str) -> Option<String> {
    hex64(text.split_whitespace().next()?)
}

// ---------------------------------------------------------------------------- choosing

/// Whether an `os-release` file names a distribution of the Fedora family (the `ID` or one of the
/// `ID_LIKE` entries).
fn rpm_family(os_release: &str) -> bool {
    os_release
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            matches!(key.trim(), "ID" | "ID_LIKE").then_some(value)
        })
        .flat_map(|value| value.trim_matches(['"', '\'']).split_whitespace())
        .any(|id| matches!(id, "fedora" | "rhel" | "centos"))
}

/// The package of a release for a system: `arch` is `std::env::consts::ARCH`, `os_release` the text
/// of `/etc/os-release` (Linux only). `None` when the release has no package for it. These are the
/// stable names the release workflow gives its files (`.github/workflows/ci.yml`).
pub fn asset_name(os: Os, arch: &str, os_release: &str) -> Option<&'static str> {
    match os {
        Os::Mac => Some("CursorPie-macos-universal.dmg"),
        Os::Windows => {
            matches!(arch, "x86_64" | "aarch64").then_some("CursorPie-windows-setup.exe")
        }
        Os::Linux if arch != "x86_64" => None,
        Os::Linux if rpm_family(os_release) => Some("CursorPie-fedora-44-x86_64.tar.gz"),
        Os::Linux => Some("CursorPie-ubuntu-24.04-x86_64.tar.gz"),
    }
}

/// Where an update of the running copy goes: the app bundle on macOS, the installed `.exe` on
/// Windows, the program on Linux. `None` when this copy is not an ordinary install: a copy run from
/// a build folder (`target/release`), from an unzipped Windows download, or a program that was
/// already replaced. `exe` is the path of the running program, `local_app_data` is `%LOCALAPPDATA%`.
pub fn install_target(os: Os, exe: &str, local_app_data: Option<&str>) -> Option<PathBuf> {
    match os {
        Os::Mac => {
            let at = exe.find(".app/Contents/MacOS/")?;
            Some(PathBuf::from(&exe[..at + ".app".len()]))
        }
        Os::Windows => {
            let local = local_app_data?.trim_end_matches(['\\', '/']);
            let installed = format!(r"{local}\Programs\{APP_NAME}\cursorpie.exe");
            let same = |a: &str, b: &str| {
                a.replace('/', "\\").to_lowercase() == b.replace('/', "\\").to_lowercase()
            };
            same(exe, &installed).then(|| PathBuf::from(exe))
        }
        Os::Linux => {
            let in_build_folder =
                exe.contains("/target/release/") || exe.contains("/target/debug/");
            let replaced = exe.ends_with(" (deleted)");
            (!in_build_folder && !replaced).then(|| PathBuf::from(exe))
        }
    }
}

// ---------------------------------------------------------------------------- errors

/// Why an update could not be installed. In every case the installed copy is untouched, and the
/// release page is still the way to update by hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallError {
    /// The release has no package for this system, or no newer release exists any more.
    NoPackage,
    /// This copy is not an ordinary install, or its folder cannot be written.
    NotInstalled,
    /// The release gives no checksum to check the download with.
    NoChecksum,
    /// The release or the package could not be fetched.
    Network(Failure),
    /// The download is not the file the release published.
    Corrupt,
    /// The package could not be opened, or does not hold CursorPie.
    Unpack,
    /// The downloaded program does not start on this system.
    SelfTest,
    /// The step that swaps the files could not be started.
    Launch,
}

impl InstallError {
    /// One sentence for the About tab.
    pub fn explain(self) -> &'static str {
        match self {
            InstallError::NoPackage => "This release has no package for this system.",
            InstallError::NotInstalled => {
                "This copy of CursorPie was not installed in the usual way, or its folder cannot be written."
            }
            InstallError::NoChecksum => {
                "The release does not publish a checksum to check the download with."
            }
            InstallError::Network(why) => why.explain(),
            InstallError::Corrupt => {
                "The download is not the file the release published, so it was thrown away."
            }
            InstallError::Unpack => "The downloaded package could not be opened.",
            InstallError::SelfTest => "The downloaded program does not start on this system.",
            InstallError::Launch => "The step that replaces CursorPie could not be started.",
        }
    }
}

impl From<HttpError> for InstallError {
    fn from(e: HttpError) -> Self {
        InstallError::Network(Failure::from(e))
    }
}

// ---------------------------------------------------------------------------- checking

/// Checks the downloaded file against what the release says: the size (when it says one) and the
/// SHA-256.
///
/// # Errors
///
/// [`InstallError::Corrupt`] when either differs or the file cannot be read.
pub fn verify(package: &Path, size: u64, expected_hex: &str) -> Result<(), InstallError> {
    let len = fs::metadata(package)
        .map_err(|_| InstallError::Corrupt)?
        .len();
    if size != 0 && len != size {
        return Err(InstallError::Corrupt);
    }
    let actual = sha256::file_hex(package).map_err(|_| InstallError::Corrupt)?;
    if actual.eq_ignore_ascii_case(expected_hex) {
        Ok(())
    } else {
        Err(InstallError::Corrupt)
    }
}

// ---------------------------------------------------------------------------- staging

/// An update that is downloaded, checked and ready to be swapped in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Staged {
    pub version: Version,
    pub os: Os,
    /// The downloaded file (the Setup program on Windows).
    pub package: PathBuf,
    /// What replaces the installed copy: the new app bundle or program next to it (macOS, Linux), the
    /// Setup program itself (Windows).
    pub staged: PathBuf,
    /// The installed copy that is replaced.
    pub target: PathBuf,
}

/// `target`'s name and `suffix`, in the same folder (the same file system, so a rename is atomic).
fn sibling(target: &Path, suffix: &str) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    target.with_file_name(name)
}

/// Whether files can be made in `dir`: the swap needs it, and an app in `/Applications` or
/// `/usr/local/bin` that the person cannot write to is not updated behind their back.
fn can_write(dir: &Path) -> bool {
    let probe = dir.join(".cursorpie-write-test");
    let ok = fs::File::create(&probe).is_ok();
    let _ = fs::remove_file(&probe);
    ok
}

/// Runs `program` with `args` and no input or output; whether it ended well.
fn succeeds(program: impl AsRef<std::ffi::OsStr>, args: &[&std::ffi::OsStr]) -> bool {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Starts `program --selftest` (see [`SELFTEST_ARG`]): whether the program runs on this system.
fn starts(program: &Path) -> bool {
    succeeds(program, &[std::ffi::OsStr::new(SELFTEST_ARG)])
}

/// Makes `path` runnable (a no-op where files have no such bit).
#[cfg(unix)]
fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

/// Makes `path` runnable (a no-op where files have no such bit).
#[cfg(not(unix))]
fn make_executable(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// macOS: copies `CursorPie.app` out of the disk image to `target`'s folder and starts it once.
fn stage_mac(package: &Path, target: &Path, dir: &Path) -> Result<PathBuf, InstallError> {
    use std::ffi::OsStr;
    let mount = dir.join("mount");
    fs::create_dir_all(&mount).map_err(|_| InstallError::Unpack)?;
    let attach = [
        "attach",
        "-nobrowse",
        "-readonly",
        "-noverify",
        "-noautoopen",
        "-quiet",
        "-mountpoint",
    ];
    let mut args: Vec<&OsStr> = attach.iter().map(|a| OsStr::new(*a)).collect();
    args.push(mount.as_os_str());
    args.push(package.as_os_str());
    if !succeeds("hdiutil", &args) {
        return Err(InstallError::Unpack);
    }
    let staged = sibling(target, NEW_SUFFIX);
    let copied = copy_app(&mount.join(format!("{APP_NAME}.app")), &staged);
    // the image must be let go of whatever happened; `-force` for a Finder that still looks at it
    let detach = [
        OsStr::new("detach"),
        mount.as_os_str(),
        OsStr::new("-quiet"),
    ];
    if !succeeds("hdiutil", &detach) {
        let forced = [
            OsStr::new("detach"),
            mount.as_os_str(),
            OsStr::new("-force"),
            OsStr::new("-quiet"),
        ];
        succeeds("hdiutil", &forced);
    }
    copied?;
    let program = staged.join("Contents").join("MacOS").join(APP_NAME);
    if !starts(&program) {
        let _ = fs::remove_dir_all(&staged);
        return Err(InstallError::SelfTest);
    }
    Ok(staged)
}

/// macOS: `ditto` copies the bundle with everything macOS keeps on its files; an app that does not
/// hold the program is refused.
fn copy_app(app: &Path, staged: &Path) -> Result<(), InstallError> {
    let program = app.join("Contents").join("MacOS").join(APP_NAME);
    if !program.is_file() {
        return Err(InstallError::Unpack);
    }
    let _ = fs::remove_dir_all(staged);
    if succeeds("ditto", &[app.as_os_str(), staged.as_os_str()]) {
        Ok(())
    } else {
        let _ = fs::remove_dir_all(staged);
        Err(InstallError::Unpack)
    }
}

/// Linux: unpacks the archive, copies the program next to the installed one and starts it once.
fn stage_linux(package: &Path, target: &Path, dir: &Path) -> Result<PathBuf, InstallError> {
    use std::ffi::OsStr;
    let unpacked = dir.join("unpacked");
    fs::create_dir_all(&unpacked).map_err(|_| InstallError::Unpack)?;
    let args = [
        OsStr::new("-xzf"),
        package.as_os_str(),
        OsStr::new("-C"),
        unpacked.as_os_str(),
    ];
    if !succeeds("tar", &args) {
        return Err(InstallError::Unpack);
    }
    // the archive holds one folder, `cursorpie-<version>-linux-<arch>`, with the program in it
    let found = fs::read_dir(&unpacked)
        .map_err(|_| InstallError::Unpack)?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("cursorpie"))
        .find(|program| program.is_file())
        .ok_or(InstallError::Unpack)?;
    let staged = sibling(target, NEW_SUFFIX);
    let _ = fs::remove_file(&staged);
    fs::copy(&found, &staged).map_err(|_| InstallError::Unpack)?;
    if make_executable(&staged).is_err() || !starts(&staged) {
        let _ = fs::remove_file(&staged);
        return Err(InstallError::SelfTest);
    }
    Ok(staged)
}

/// Makes the package ready to be swapped in (see the table at the top).
fn stage(
    version: Version,
    os: Os,
    package: PathBuf,
    target: PathBuf,
    dir: &Path,
) -> Result<Staged, InstallError> {
    let staged = match os {
        Os::Windows => package.clone(),
        Os::Mac => stage_mac(&package, &target, dir)?,
        Os::Linux => stage_linux(&package, &target, dir)?,
    };
    Ok(Staged {
        version,
        os,
        package,
        staged,
        target,
    })
}

// ---------------------------------------------------------------------------- the worker

/// Folder of the downloaded package, emptied and made again.
fn fresh_updates_dir() -> io::Result<PathBuf> {
    let dir = app_support_dir().join(UPDATES_DIR);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// The checksum of `name`: the one in the release's answer, else the one in its `name.sha256` file.
fn expected_checksum(release: &Release, asset: &Asset) -> Result<String, InstallError> {
    if let Some(digest) = &asset.digest {
        return Ok(digest.clone());
    }
    let sidecar = format!("{}.sha256", asset.name);
    if !release.assets.iter().any(|a| a.name == sidecar) {
        return Err(InstallError::NoChecksum);
    }
    let url = asset_url(release, &sidecar);
    let text = http::get_text(&url, CHECK_TIMEOUT_S, MAX_CHECKSUM_BYTES)?;
    parse_checksum_file(&text).ok_or(InstallError::NoChecksum)
}

/// Where an update of the running copy would go, or `None` when this copy is not one that is updated
/// in place (see [`install_target`]) or its folder cannot be written. Makes and removes one empty file,
/// so call it when something opens, never on every frame.
pub fn installable_here() -> Option<PathBuf> {
    let os = platform::OS;
    let exe = std::env::current_exe().ok()?;
    let local = std::env::var("LOCALAPPDATA").ok();
    let target = install_target(os, &exe.to_string_lossy(), local.as_deref())?;
    let writable = os == Os::Windows || target.parent().is_some_and(can_write);
    writable.then_some(target)
}

/// Downloads the newest release's package for this system, checks it and stages it. Blocks for as
/// long as the download takes: call it from a worker thread. Asks GitHub again instead of trusting an
/// earlier answer, so a release that came out meanwhile is the one installed.
///
/// # Errors
///
/// See [`InstallError`]. Nothing outside the updates folder and the folder of the installed copy is
/// touched, and what was made there is removed again on failure.
pub fn prepare() -> Result<Staged, InstallError> {
    let os = platform::OS;
    let target = installable_here().ok_or(InstallError::NotInstalled)?;
    let os_release = fs::read_to_string("/etc/os-release").unwrap_or_default();
    let name =
        asset_name(os, std::env::consts::ARCH, &os_release).ok_or(InstallError::NoPackage)?;

    let json = http::get_text(LATEST_RELEASE_API_URL, CHECK_TIMEOUT_S, MAX_ANSWER_BYTES)?;
    let release = parse_release(&json).ok_or(InstallError::Network(Failure::BadAnswer))?;
    if !Version::current().is_some_and(|current| release.version > current) {
        return Err(InstallError::NoPackage);
    }
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == name)
        .ok_or(InstallError::NoPackage)?;
    let expected = expected_checksum(&release, asset)?;

    let dir = fresh_updates_dir().map_err(|_| InstallError::NotInstalled)?;
    let package = dir.join(name);
    let url = asset_url(&release, name);
    http::download(&url, &package, DOWNLOAD_TIMEOUT_S, MAX_PACKAGE_BYTES)?;
    let outcome = verify(&package, asset.size, &expected)
        .and_then(|()| stage(release.version, os, package, target, &dir));
    if outcome.is_err() {
        let _ = fs::remove_dir_all(&dir);
    }
    outcome
}

/// Runs [`prepare`] on a worker thread; the result arrives as [`Msg::UpdateStaged`].
pub fn spawn_prepare(notifier: Notifier) {
    std::thread::spawn(move || {
        let result = prepare();
        let ok = result.is_ok();
        diag::log!("ui", "update_staged", ok = ok);
        notifier.send(Msg::UpdateStaged(result));
    });
}

// ---------------------------------------------------------------------------- swapping

/// Starts the swap of a staged update. The caller quits CursorPie right after: the swap waits for it
/// (macOS, Linux) or ends it (Windows Setup).
///
/// # Errors
///
/// [`InstallError::Launch`] when the step that swaps the files could not be started; nothing has
/// changed then.
pub fn apply(staged: &Staged) -> Result<(), InstallError> {
    let started = match staged.os {
        Os::Windows => apply_windows(staged),
        Os::Mac | Os::Linux => apply_unix(staged),
    };
    started.map_err(|_| InstallError::Launch)
}

/// Windows: runs Setup silently and detached, so it outlives this program.
fn apply_windows(staged: &Staged) -> io::Result<()> {
    let mut cmd = Command::new(&staged.staged);
    cmd.args(SETUP_ARGS)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0208); // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
    }
    cmd.spawn().map(|_| ())
}

/// macOS and Linux: writes the swap script next to the package and starts it in a process group of
/// its own, so it goes on when this program quits.
fn apply_unix(staged: &Staged) -> io::Result<()> {
    let script = staged.package.with_file_name("apply.sh");
    fs::write(&script, APPLY_SCRIPT)?;
    let mode = if staged.os == Os::Mac { "mac" } else { "linux" };
    let mut cmd = Command::new("/bin/sh");
    cmd.arg(&script)
        .arg(std::process::id().to_string())
        .arg(&staged.staged)
        .arg(&staged.target)
        .arg(mode)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let id = std::process::id();
        let d = std::env::temp_dir().join(format!("cursorpie-install-{id}-{name}"));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    const HASH: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn a_release_is_read_with_its_files_and_their_checksums() {
        let json = format!(
            r#"{{"tag_name": "v0.9.0", "draft": false, "prerelease": false, "assets": [
                {{"name": "CursorPie-macos-universal.dmg", "size": 123, "digest": "sha256:{HASH}"}},
                {{"name": "CursorPie-windows.zip", "size": 7}},
                {{"name": "../evil", "size": 1}},
                {{"name": ".hidden", "size": 1}},
                {{"size": 1}}
            ]}}"#
        );
        let r = parse_release(&json).expect("a release");
        assert_eq!(r.version, Version::parse("0.9.0").unwrap());
        assert_eq!(r.tag, "v0.9.0");
        let names: Vec<&str> = r.assets.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(
            names,
            ["CursorPie-macos-universal.dmg", "CursorPie-windows.zip"]
        );
        assert_eq!(r.assets[0].size, 123);
        assert_eq!(r.assets[0].digest.as_deref(), Some(HASH));
        assert_eq!(r.assets[1].digest, None);
    }

    #[test]
    fn a_bad_checksum_text_is_not_a_checksum() {
        let release = |digest: &str| {
            let item = format!(r#"{{"name": "a.dmg", "digest": "{digest}"}}"#);
            parse_release(&format!(r#"{{"tag_name": "0.9.0", "assets": [{item}]}}"#)).unwrap()
        };
        assert_eq!(release("md5:abc").assets[0].digest, None);
        assert_eq!(release("sha256:abc").assets[0].digest, None);
        assert_eq!(
            release(&format!("sha256:{}", HASH.to_uppercase())).assets[0]
                .digest
                .as_deref(),
            Some(HASH)
        );
    }

    #[test]
    fn a_tag_that_is_not_a_plain_version_is_refused() {
        for tag in [
            "v0.9.0-rc1",
            "release-0.9.0",
            "V0.9.0",
            "vv0.9.0",
            " v0.9.0",
            "v0.9",
        ] {
            let json = format!(r#"{{"tag_name": "{tag}", "assets": []}}"#);
            assert_eq!(parse_release(&json), None, "{tag:?}");
        }
        assert!(parse_release(r#"{"tag_name": "v0.9.0", "draft": true, "assets": []}"#).is_none());
        let bare = parse_release(r#"{"tag_name": "v0.9.0"}"#).expect("a release");
        assert!(bare.assets.is_empty(), "no assets list means no assets");
    }

    #[test]
    fn the_address_of_a_file_is_made_from_the_tag_and_the_name() {
        let r = parse_release(r#"{"tag_name": "v0.9.0", "assets": []}"#).unwrap();
        assert_eq!(
            asset_url(&r, "CursorPie-windows-setup.exe"),
            "https://github.com/sebastvnd/cursorpie/releases/download/v0.9.0/CursorPie-windows-setup.exe"
        );
    }

    #[test]
    fn a_checksum_file_is_read_like_shasum_writes_it() {
        let text = format!("{HASH}  CursorPie-macos-universal.dmg\n");
        assert_eq!(parse_checksum_file(&text).as_deref(), Some(HASH));
        assert_eq!(parse_checksum_file(""), None);
        assert_eq!(parse_checksum_file("not a hash  file"), None);
        assert_eq!(parse_checksum_file(&HASH[..60]), None);
    }

    #[test]
    fn each_system_gets_its_own_package() {
        assert_eq!(
            asset_name(Os::Mac, "aarch64", ""),
            Some("CursorPie-macos-universal.dmg")
        );
        assert_eq!(
            asset_name(Os::Mac, "x86_64", ""),
            Some("CursorPie-macos-universal.dmg")
        );
        assert_eq!(
            asset_name(Os::Windows, "x86_64", ""),
            Some("CursorPie-windows-setup.exe")
        );
        assert_eq!(asset_name(Os::Windows, "x86", ""), None);
    }

    #[test]
    fn linux_picks_by_distribution_and_only_for_x86_64() {
        let ubuntu = "NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\n";
        let fedora = "NAME=\"Fedora Linux\"\nID=fedora\n";
        let rocky = "ID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\n";
        let arch = "ID=arch\n";
        assert_eq!(
            asset_name(Os::Linux, "x86_64", ubuntu),
            Some("CursorPie-ubuntu-24.04-x86_64.tar.gz")
        );
        assert_eq!(
            asset_name(Os::Linux, "x86_64", fedora),
            Some("CursorPie-fedora-44-x86_64.tar.gz")
        );
        assert_eq!(
            asset_name(Os::Linux, "x86_64", rocky),
            Some("CursorPie-fedora-44-x86_64.tar.gz")
        );
        assert_eq!(
            asset_name(Os::Linux, "x86_64", arch),
            Some("CursorPie-ubuntu-24.04-x86_64.tar.gz")
        );
        assert_eq!(
            asset_name(Os::Linux, "x86_64", ""),
            Some("CursorPie-ubuntu-24.04-x86_64.tar.gz")
        );
        assert_eq!(asset_name(Os::Linux, "aarch64", ubuntu), None);
    }

    #[test]
    fn only_an_ordinary_install_is_updated_on_macos() {
        let app = "/Applications/CursorPie.app/Contents/MacOS/CursorPie";
        assert_eq!(
            install_target(Os::Mac, app, None),
            Some(PathBuf::from("/Applications/CursorPie.app"))
        );
        let user = "/Users/ann/Applications/CursorPie.app/Contents/MacOS/CursorPie";
        assert_eq!(
            install_target(Os::Mac, user, None),
            Some(PathBuf::from("/Users/ann/Applications/CursorPie.app"))
        );
        assert_eq!(
            install_target(
                Os::Mac,
                "/Users/ann/cursorpie/target/release/cursorpie",
                None
            ),
            None
        );
    }

    #[test]
    fn only_the_installed_exe_is_updated_on_windows() {
        let local = Some(r"C:\Users\Ann\AppData\Local");
        let installed = r"C:\Users\Ann\AppData\Local\Programs\CursorPie\cursorpie.exe";
        assert!(install_target(Os::Windows, installed, local).is_some());
        let shouty = r"c:\users\ann\appdata\local\programs\cursorpie\CURSORPIE.EXE";
        assert!(
            install_target(Os::Windows, shouty, local).is_some(),
            "case and slashes do not matter"
        );
        let slashes = "C:/Users/Ann/AppData/Local/Programs/CursorPie/cursorpie.exe";
        assert!(install_target(Os::Windows, slashes, local).is_some());
        let unzipped = r"C:\Users\Ann\Downloads\CursorPie\cursorpie.exe";
        assert_eq!(install_target(Os::Windows, unzipped, local), None);
        assert_eq!(
            install_target(Os::Windows, installed, None),
            None,
            "no profile folder known"
        );
    }

    #[test]
    fn a_linux_build_folder_or_a_replaced_program_is_left_alone() {
        let path = |p: &str| install_target(Os::Linux, p, None);
        assert_eq!(
            path("/usr/local/bin/cursorpie"),
            Some(PathBuf::from("/usr/local/bin/cursorpie"))
        );
        assert!(path("/home/ann/.local/bin/cursorpie").is_some());
        assert_eq!(
            path("/home/ann/src/cursorpie/target/release/cursorpie"),
            None
        );
        assert_eq!(path("/home/ann/src/cursorpie/target/debug/cursorpie"), None);
        assert_eq!(path("/usr/local/bin/cursorpie (deleted)"), None);
    }

    #[test]
    fn a_download_is_accepted_only_if_size_and_checksum_match() {
        let d = temp_dir("verify");
        let file = d.join("abc.bin");
        fs::write(&file, b"abc").unwrap();
        assert_eq!(verify(&file, 3, HASH), Ok(()));
        assert_eq!(
            verify(&file, 0, HASH),
            Ok(()),
            "a release that does not say the size"
        );
        assert_eq!(verify(&file, 3, &HASH.to_uppercase()), Ok(()));
        assert_eq!(
            verify(&file, 4, HASH),
            Err(InstallError::Corrupt),
            "wrong size"
        );
        assert_eq!(
            verify(&file, 3, &HASH.replace('b', "c")),
            Err(InstallError::Corrupt)
        );
        assert_eq!(
            verify(&d.join("missing"), 3, HASH),
            Err(InstallError::Corrupt)
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn the_staged_copy_is_made_next_to_the_installed_one() {
        let app = Path::new("/Applications/CursorPie.app");
        assert_eq!(
            sibling(app, NEW_SUFFIX),
            Path::new("/Applications/CursorPie.app.cursorpie-new")
        );
        let bin = Path::new("/usr/local/bin/cursorpie");
        assert_eq!(
            sibling(bin, NEW_SUFFIX),
            Path::new("/usr/local/bin/cursorpie.cursorpie-new")
        );
    }

    #[test]
    fn a_folder_that_cannot_be_written_is_told_apart() {
        let d = temp_dir("write");
        assert!(can_write(&d));
        assert!(!can_write(&d.join("missing")));
        assert!(
            !d.join(".cursorpie-write-test").exists(),
            "the probe is removed"
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn every_package_the_update_looks_for_is_built_by_the_release_workflow() {
        let workflow = include_str!("../../.github/workflows/ci.yml");
        let fedora = "ID=fedora\n";
        let wanted = [
            asset_name(Os::Mac, "aarch64", ""),
            asset_name(Os::Windows, "x86_64", ""),
            asset_name(Os::Linux, "x86_64", ""),
            asset_name(Os::Linux, "x86_64", fedora),
        ];
        for name in wanted.into_iter().flatten() {
            assert!(workflow.contains(name), "ci.yml no longer builds {name}");
        }
        assert!(workflow.contains("CursorPie-macos-universal.dmg.sha256"));
    }

    #[test]
    fn every_error_has_its_own_sentence() {
        let all = [
            InstallError::NoPackage,
            InstallError::NotInstalled,
            InstallError::NoChecksum,
            InstallError::Network(Failure::Unreachable),
            InstallError::Corrupt,
            InstallError::Unpack,
            InstallError::SelfTest,
            InstallError::Launch,
        ];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a.explain(), b.explain());
            }
        }
        assert_eq!(
            InstallError::from(HttpError::NoClient),
            InstallError::Network(Failure::NoClient)
        );
    }

    #[test]
    fn the_swap_script_waits_swaps_back_on_failure_and_starts_the_program() {
        let s = APPLY_SCRIPT;
        assert!(s.starts_with("#!/bin/sh\n"));
        assert!(
            s.contains("kill -0 \"$pid\""),
            "waits for CursorPie to quit"
        );
        assert!(
            s.contains("mv \"$old\" \"$target\""),
            "puts the old copy back if the swap fails"
        );
        assert!(s.contains("open \"$target\"") && s.contains("nohup \"$target\""));
        // every argument is quoted, so a path with a space or a quote is one word
        for var in ["$pid", "$staged", "$target", "$mode", "$old"] {
            assert!(!s.contains(&format!(" {var} ")), "{var} unquoted");
        }
    }

    #[test]
    fn setup_is_started_silently_and_keeps_the_login_choice() {
        assert!(SETUP_ARGS.contains(&"/VERYSILENT"));
        assert!(SETUP_ARGS.contains(&"/MERGETASKS=!startup"));
        assert!(SETUP_ARGS.contains(&"/RESTARTAPP=1"));
    }
}
