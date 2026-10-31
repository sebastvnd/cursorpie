//! `cargo tools zip`: packs the project into one `.zip` file, without the build folder.
//!
//! Handy for sending the sources to someone, or for a backup: `target/` (gigabytes after a few
//! builds) and `.git/` are left out. When the project is a Git clone, the files are the ones Git
//! knows (tracked, plus new files that are not ignored), so `.gitignore` decides; otherwise the
//! folder is walked and a fixed list of build and tool folders is skipped.
//!
//! | Option | Meaning |
//! |---|---|
//! | `-o FILE`, `--output FILE` | where to write the archive (default: next to the project folder, `NAME-VERSION.zip`) |
//! | `--list` | print the files that would be packed and write nothing |
//! | `--no-git` | walk the folder even when it is a Git clone |
//!
//! Always left out, whatever Git says: a `target` folder at the top of the project or of one of its
//! crates, `.git`, `node_modules`, `.DS_Store` and `Thumbs.db`. Symbolic links are not followed.
//!
//! The archive is written by a small writer of this file (standard library only): files are
//! stored without compression, which keeps it simple and the result readable by every unzip tool.
//! A source tree is small, so the size does not matter. Names are UTF-8 and the executable bit of
//! scripts is kept.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::UNIX_EPOCH;

use crate::clean::human_bytes;
use crate::git::git;

/// What to pack and where to put it.
#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    /// The archive to write; `None` is the default name next to the project.
    pub output: Option<PathBuf>,
    /// Only print the file list.
    pub list: bool,
    /// Walk the folder even when it is a Git clone.
    pub no_git: bool,
}

/// Reads the arguments after `zip`.
///
/// # Errors
/// On an unknown option or a missing file name.
pub fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut opts = Options {
        output: None,
        list: false,
        no_git: false,
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-o" | "--output" => {
                let file = args.next().ok_or("--output needs a file name")?;
                opts.output = Some(PathBuf::from(file));
            }
            "--list" => opts.list = true,
            "--no-git" => opts.no_git = true,
            other => return Err(format!("unknown option '{other}' for zip")),
        }
    }
    Ok(opts)
}

// ------------------------------------------------------------------------------ the file list

/// Whether a path of the project (relative, with `/`) is never packed.
pub fn is_excluded(rel: &str) -> bool {
    let parts: Vec<&str> = rel.split('/').collect();
    let last = parts.len() - 1;
    parts.iter().enumerate().any(|(i, part)| {
        let is_dir = i < last;
        (is_dir && matches!(*part, ".git" | "node_modules"))
            // the build folder of the project or of one of its crates, not a source folder
            // that happens to be called `target` deeper down
            || (is_dir && *part == "target" && i <= 1)
            || (i == last && matches!(*part, ".DS_Store" | "Thumbs.db"))
    })
}

/// The files of the project, relative and with `/`, sorted.
fn collect(root: &Path, use_git: bool) -> Result<Vec<String>, String> {
    let mut files = match from_git(root, use_git) {
        Some(files) => files,
        None => {
            let mut files = Vec::new();
            walk(root, "", &mut files)?;
            files
        }
    };
    files.retain(|f| !is_excluded(f));
    files.sort();
    files.dedup();
    Ok(files)
}

/// What Git lists, when the project is a clone and Git works.
fn from_git(root: &Path, use_git: bool) -> Option<Vec<String>> {
    if !use_git || !root.join(".git").exists() {
        return None;
    }
    let out = git(
        root,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )
    .ok()?;
    Some(
        out.split('\0')
            .filter(|p| !p.is_empty())
            .filter(|p| root.join(p).is_file()) // a tracked file that was deleted is not packed
            .map(str::to_string)
            .collect(),
    )
}

/// Adds every file below `dir` (the folder `rel` of the project) to `files`.
fn walk(dir: &Path, rel: &str, files: &mut Vec<String>) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue; // a name that is not UTF-8 cannot go into the archive
        };
        let path = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if !is_excluded(&format!("{path}/_")) {
                walk(&entry.path(), &path, files)?;
            }
        } else if kind.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

// ------------------------------------------------------------------------------------ naming

/// The `name` and the `version` of the package in the text of a `Cargo.toml`.
pub fn package_id(cargo_toml: &str) -> (Option<String>, Option<String>) {
    let (mut name, mut version) = (None, None);
    let mut in_package = false;
    for line in cargo_toml.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !in_package {
            continue;
        }
        match key.trim() {
            "name" if name.is_none() => name = quoted(value),
            "version" if version.is_none() => version = quoted(value),
            _ => {}
        }
    }
    (name, version)
}

/// The text inside the first pair of double quotes of `value`.
fn quoted(value: &str) -> Option<String> {
    let rest = value.trim().strip_prefix('"')?;
    Some(rest[..rest.find('"')?].to_string())
}

/// The name of the archive and of the folder inside it: `cursorpie-0.5.2`.
pub fn archive_stem(name: Option<&str>, version: Option<&str>) -> String {
    match (name, version) {
        (Some(n), Some(v)) => format!("{n}-{v}"),
        (Some(n), None) => n.to_string(),
        _ => "project".to_string(),
    }
}

// ------------------------------------------------------------------------------- the writer

/// What the central directory needs to know about a file already written.
struct Entry {
    name: String,
    crc: u32,
    size: u32,
    offset: u32,
    time: u16,
    date: u16,
    mode: u32,
}

/// Bit 11 of the flags: the names are UTF-8.
const FLAG_UTF8: u16 = 0x0800;

fn put16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn put32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

/// The CRC-32 of `data` (the checksum a zip file keeps of each file).
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A Unix time as the two 16-bit numbers of a zip file: `(time, date)`, in UTC. Times before 1980
/// become 1980-01-01 and times after 2107 become the last day the format can hold.
pub fn dos_datetime(unix_secs: u64) -> (u16, u16) {
    let days = i64::try_from(unix_secs / 86_400).unwrap_or(0);
    let rem = unix_secs % 86_400;
    // civil date from a day count (H. Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    if year < 1980 {
        return (0, (1 << 5) | 1);
    }
    let year = u16::try_from((year - 1980).min(127)).unwrap_or(127);
    let date =
        (year << 9) | (u16::try_from(month).unwrap_or(1) << 5) | u16::try_from(day).unwrap_or(1);
    let hour = u16::try_from(rem / 3_600).unwrap_or(0);
    let minute = u16::try_from(rem % 3_600 / 60).unwrap_or(0);
    let second = u16::try_from(rem % 60 / 2).unwrap_or(0);
    ((hour << 11) | (minute << 5) | second, date)
}

/// Writes a zip file entry by entry, then the directory at the end.
pub struct ZipWriter<W: Write> {
    out: W,
    offset: u64,
    entries: Vec<Entry>,
}

impl<W: Write> ZipWriter<W> {
    /// A writer that puts the archive into `out`.
    pub fn new(out: W) -> Self {
        ZipWriter {
            out,
            offset: 0,
            entries: Vec::new(),
        }
    }

    /// Adds one file. `unix_secs` is its modification time; `executable` keeps the script bit.
    ///
    /// # Errors
    /// When the file or the archive is too big for the classic zip format (4 GiB, 65535 files),
    /// the name is too long, or writing fails.
    pub fn add(
        &mut self,
        name: &str,
        data: &[u8],
        unix_secs: u64,
        executable: bool,
    ) -> Result<(), String> {
        if self.entries.len() >= usize::from(u16::MAX) {
            return Err("too many files for a zip file (65535)".to_string());
        }
        let size =
            u32::try_from(data.len()).map_err(|_| format!("{name} is too big for a zip file"))?;
        let offset = u32::try_from(self.offset)
            .map_err(|_| "the archive is bigger than 4 GiB".to_string())?;
        let name_len =
            u16::try_from(name.len()).map_err(|_| format!("the path is too long: {name}"))?;
        let (time, date) = dos_datetime(unix_secs);
        let crc = crc32(data);
        let mut head = Vec::with_capacity(30 + name.len());
        put32(&mut head, 0x0403_4b50); // local file header
        put16(&mut head, 20); // version needed
        put16(&mut head, FLAG_UTF8);
        put16(&mut head, 0); // method 0: stored
        put16(&mut head, time);
        put16(&mut head, date);
        put32(&mut head, crc);
        put32(&mut head, size); // compressed size
        put32(&mut head, size); // size
        put16(&mut head, name_len);
        put16(&mut head, 0); // no extra field
        head.extend_from_slice(name.as_bytes());
        self.out.write_all(&head).map_err(|e| e.to_string())?;
        self.out.write_all(data).map_err(|e| e.to_string())?;
        self.offset += head.len() as u64 + data.len() as u64;
        self.entries.push(Entry {
            name: name.to_string(),
            crc,
            size,
            offset,
            time,
            date,
            mode: if executable { 0o100_755 } else { 0o100_644 },
        });
        Ok(())
    }

    /// Writes the directory and returns the underlying writer.
    ///
    /// # Errors
    /// When writing fails or the directory does not fit the classic format.
    pub fn finish(mut self) -> Result<W, String> {
        let cd_offset = u32::try_from(self.offset)
            .map_err(|_| "the archive is bigger than 4 GiB".to_string())?;
        let mut dir = Vec::new();
        for e in &self.entries {
            put32(&mut dir, 0x0201_4b50); // central directory header
            put16(&mut dir, (3 << 8) | 20); // made by: Unix, zip 2.0
            put16(&mut dir, 20); // version needed
            put16(&mut dir, FLAG_UTF8);
            put16(&mut dir, 0); // stored
            put16(&mut dir, e.time);
            put16(&mut dir, e.date);
            put32(&mut dir, e.crc);
            put32(&mut dir, e.size);
            put32(&mut dir, e.size);
            put16(&mut dir, u16::try_from(e.name.len()).unwrap_or(u16::MAX));
            put16(&mut dir, 0); // extra field
            put16(&mut dir, 0); // comment
            put16(&mut dir, 0); // disk
            put16(&mut dir, 0); // internal attributes
            put32(&mut dir, e.mode << 16); // Unix permissions
            put32(&mut dir, e.offset);
            dir.extend_from_slice(e.name.as_bytes());
        }
        let count = u16::try_from(self.entries.len()).map_err(|_| "too many files".to_string())?;
        let dir_size =
            u32::try_from(dir.len()).map_err(|_| "the directory is too big".to_string())?;
        put32(&mut dir, 0x0605_4b50); // end of central directory
        put16(&mut dir, 0);
        put16(&mut dir, 0);
        put16(&mut dir, count);
        put16(&mut dir, count);
        put32(&mut dir, dir_size);
        put32(&mut dir, cd_offset);
        put16(&mut dir, 0); // no comment
        self.out.write_all(&dir).map_err(|e| e.to_string())?;
        self.out.flush().map_err(|e| e.to_string())?;
        Ok(self.out)
    }
}

// ------------------------------------------------------------------------------- the command

/// Whether a file is a script that must stay executable.
#[cfg(unix)]
fn is_executable(_rel: &str, meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

/// Whether a file is a script that must stay executable: outside Unix the file system has no such
/// bit, so the shell scripts of the project are the ones.
#[cfg(not(unix))]
fn is_executable(rel: &str, _meta: &std::fs::Metadata) -> bool {
    rel.ends_with(".sh")
}

/// Packs the project at `root`.
///
/// # Errors
/// When the files cannot be listed, read or written.
pub fn run(root: &Path, opts: &Options) -> Result<ExitCode, String> {
    let files = collect(root, !opts.no_git)?;
    if opts.list {
        for f in &files {
            println!("{f}");
        }
        println!("{} files", files.len());
        return Ok(ExitCode::SUCCESS);
    }
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();
    let (name, version) = package_id(&manifest);
    let stem = archive_stem(name.as_deref(), version.as_deref());
    let output = match &opts.output {
        Some(p) => p.clone(),
        None => root
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
            .join(format!("{stem}.zip")),
    };
    // never pack the archive into itself, when it is written inside the project
    let root_real = std::fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let out_real = output_path(&output);
    let files: Vec<String> = files
        .into_iter()
        .filter(|f| Some(root_real.join(f)) != out_real)
        .collect();

    match write_archive(root, &files, &stem, &output) {
        Ok((count, bytes)) => {
            println!(
                "Wrote {} ({count} files, {}). Left out: target/, .git/ and what Git ignores.",
                output.display(),
                human_bytes(bytes)
            );
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&output);
            Err(e)
        }
    }
}

/// The absolute form of the archive path, even when the file does not exist yet.
fn output_path(output: &Path) -> Option<PathBuf> {
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let dir = std::fs::canonicalize(parent).ok()?;
    Some(dir.join(output.file_name()?))
}

/// Writes the archive and returns how many files it holds and how big it is.
fn write_archive(
    root: &Path,
    files: &[String],
    stem: &str,
    output: &Path,
) -> Result<(usize, u64), String> {
    let file =
        File::create(output).map_err(|e| format!("cannot create {}: {e}", output.display()))?;
    let mut zip = ZipWriter::new(BufWriter::new(file));
    let mut count = 0;
    for rel in files {
        let path = root.join(rel);
        let (Ok(data), Ok(meta)) = (std::fs::read(&path), std::fs::metadata(&path)) else {
            eprintln!("skipped (cannot be read): {rel}");
            continue;
        };
        let secs = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        zip.add(
            &format!("{stem}/{rel}"),
            &data,
            secs,
            is_executable(rel, &meta),
        )?;
        count += 1;
    }
    zip.finish()?;
    let bytes = std::fs::metadata(output).map_or(0, |m| m.len());
    Ok((count, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_options_are_read() {
        let o = parse_args(&words(&[])).unwrap();
        assert_eq!(
            o,
            Options {
                output: None,
                list: false,
                no_git: false
            }
        );
        let o = parse_args(&words(&["-o", "a.zip", "--list", "--no-git"])).unwrap();
        assert_eq!(o.output, Some(PathBuf::from("a.zip")));
        assert!(o.list && o.no_git);
        assert!(parse_args(&words(&["-o"])).is_err());
        assert!(parse_args(&words(&["--fast"])).is_err());
    }

    #[test]
    fn build_and_tool_folders_are_left_out() {
        assert!(is_excluded("target/debug/app"));
        assert!(is_excluded("tools/target/release/x"));
        assert!(is_excluded(".git/config"));
        assert!(is_excluded("web/node_modules/a/b.js"));
        assert!(is_excluded("icon/.DS_Store"));
        assert!(!is_excluded("src/main.rs"));
        assert!(
            !is_excluded("src/tap/target.rs"),
            "a file called target is a source file"
        );
        assert!(
            !is_excluded("src/a/b/target/x.rs"),
            "only the build folders at the top"
        );
        assert!(
            !is_excluded("docs/.DS_Store/x"),
            "a name is only a junk file when it is the file"
        );
    }

    #[test]
    fn the_name_and_version_come_from_the_package_table() {
        let toml = "[package]\nname = \"cursorpie\"\nversion = \"0.5.2\" # keep\n\n[dependencies]\nserde = { version = \"1\" }\nname = \"other\"\n";
        assert_eq!(
            package_id(toml),
            (Some("cursorpie".into()), Some("0.5.2".into()))
        );
        assert_eq!(package_id("[workspace]\nmembers = []\n"), (None, None));
        assert_eq!(archive_stem(Some("a"), Some("1.0.0")), "a-1.0.0");
        assert_eq!(archive_stem(Some("a"), None), "a");
        assert_eq!(archive_stem(None, None), "project");
    }

    #[test]
    fn the_checksum_is_the_standard_one() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn times_become_dos_times() {
        // 2026-10-26 07:33:20 UTC
        let date = (46 << 9) | (10 << 5) | 26;
        let time = (7 << 11) | (33 << 5) | 10;
        assert_eq!(dos_datetime(1_793_000_000), (time, date));
        assert_eq!(dos_datetime(0), (0, 33), "before 1980 is 1980-01-01");
    }

    fn le16(b: &[u8], at: usize) -> usize {
        usize::from(u16::from_le_bytes([b[at], b[at + 1]]))
    }

    fn le32(b: &[u8], at: usize) -> usize {
        u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as usize
    }

    #[test]
    fn an_archive_has_headers_data_and_a_directory() {
        let mut zip = ZipWriter::new(Vec::new());
        zip.add("p/a.txt", b"hello", 1_793_000_000, false).unwrap();
        zip.add("p/run.sh", b"#!/bin/sh\n", 1_793_000_000, true)
            .unwrap();
        let bytes = zip.finish().unwrap();

        assert_eq!(&bytes[..4], b"PK\x03\x04");
        assert_eq!(&bytes[30..37], b"p/a.txt");
        assert_eq!(&bytes[37..42], b"hello");

        let end = bytes.len() - 22;
        assert_eq!(&bytes[end..end + 4], b"PK\x05\x06");
        assert_eq!(le16(&bytes, end + 8), 2, "two files on this disk");
        assert_eq!(le16(&bytes, end + 10), 2, "two files in all");
        let (dir_size, dir_offset) = (le32(&bytes, end + 12), le32(&bytes, end + 16));
        assert_eq!(
            dir_offset + dir_size,
            end,
            "the directory ends where the end record starts"
        );
        assert_eq!(&bytes[dir_offset..dir_offset + 4], b"PK\x01\x02");
        // the first entry points back to the start, and holds the checksum of "hello"
        assert_eq!(le32(&bytes, dir_offset + 42), 0);
        assert_eq!(le32(&bytes, dir_offset + 16), crc32(b"hello") as usize);
        // the second file is marked executable in the Unix permissions
        let second = dir_offset + 46 + "p/a.txt".len();
        assert_eq!(le32(&bytes, second + 38) >> 16, 0o100_755);
        assert_eq!(le32(&bytes, dir_offset + 38) >> 16, 0o100_644);
    }
}
