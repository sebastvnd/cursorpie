//! The kit that is handed to an AI agent so it can write a flow: a `.zip` made on demand from the
//! guides of this repository and what the person wants.
//!
//! The Flow tab's "New flow" sheet (see `ui::settings::tabs::flow_new`) lets the person describe the
//! flow they want, then puts this zip on the clipboard. They paste it into any AI agent, add a
//! prompt such as "make this", and import the file the agent returns.
//!
//! The kit holds `START_HERE.md` (what the agent must do), `REQUEST.md` (the person's words),
//! `FLOWS_FOR_AI.md`, `flow.schema.json` and `examples/flows/*`. All of it is compiled into the app
//! with `include_str!`, so the kit can never drift from the version of the app that reads the flow.
//!
//! No part of this module touches the OS or egui: [`kit_files`] and [`zip_stored`] are pure and
//! unit tested, [`write_kit`] takes the directory as a parameter. There is no zip dependency: the
//! archive is written with the *stored* method (no compression), which every unzip tool reads and
//! which is enough for about 100 KB of text.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Most characters of the request that go into the kit. The sheet limits the text field to the same
/// number, and [`request_file`] cuts what is longer.
pub const MAX_REQUEST_CHARS: usize = 4000;

/// The folder inside the app support directory that holds the kit (one file, replaced every time).
pub const KIT_DIR: &str = "ai-kit";

/// The name of the zip file.
pub const KIT_FILE: &str = "CursorPie-flow-kit.zip";

const START_HERE: &str = include_str!("../../docs/flow-kit/START_HERE.md");
const GUIDE: &str = include_str!("../../docs/FLOWS_FOR_AI.md");
const SCHEMA: &str = include_str!("../../docs/flow.schema.json");

/// The examples, under the same relative path the guide links to (`examples/flows/`).
const EXAMPLES: [(&str, &str); 10] = [
    (
        "examples/flows/ask-and-loop.cursorpie-flow.json",
        include_str!("../../docs/examples/flows/ask-and-loop.cursorpie-flow.json"),
    ),
    (
        "examples/flows/config-with-flow.json",
        include_str!("../../docs/examples/flows/config-with-flow.json"),
    ),
    (
        "examples/flows/copy-newest-file-and-paste.cursorpie-flow.json",
        include_str!("../../docs/examples/flows/copy-newest-file-and-paste.cursorpie-flow.json"),
    ),
    (
        "examples/flows/copy-switch-paste.cursorpie-flow.json",
        include_str!("../../docs/examples/flows/copy-switch-paste.cursorpie-flow.json"),
    ),
    (
        "examples/flows/link-wait-command.cursorpie-flow.json",
        include_str!("../../docs/examples/flows/link-wait-command.cursorpie-flow.json"),
    ),
    (
        "examples/flows/open-app-and-new-note.cursorpie-flow.json",
        include_str!("../../docs/examples/flows/open-app-and-new-note.cursorpie-flow.json"),
    ),
    (
        "examples/flows/quick-reply-unread-chats.cursorpie-flow.json",
        include_str!("../../docs/examples/flows/quick-reply-unread-chats.cursorpie-flow.json"),
    ),
    (
        "examples/flows/send-then-wait-for-answer.cursorpie-flow.json",
        include_str!("../../docs/examples/flows/send-then-wait-for-answer.cursorpie-flow.json"),
    ),
    (
        "examples/flows/telegram-folder-triage.cursorpie-flow.json",
        include_str!("../../docs/examples/flows/telegram-folder-triage.cursorpie-flow.json"),
    ),
    (
        "examples/flows/telegram-saved-reply.cursorpie-flow.json",
        include_str!("../../docs/examples/flows/telegram-saved-reply.cursorpie-flow.json"),
    ),
];

/// The text of `REQUEST.md`: what the person wants and the system they use (`system` is a name such
/// as "macOS"; the agent needs it because shortcuts and shell commands differ).
pub fn request_file(request: &str, system: &str) -> String {
    let wanted: String = request.trim().chars().take(MAX_REQUEST_CHARS).collect();
    let wanted = if wanted.is_empty() {
        "(The person did not describe the flow. Ask what it should do.)".to_string()
    } else {
        wanted
    };
    format!("# What the person wants\n\n{wanted}\n\n# Their system\n\n{system}\n")
}

/// Every file of the kit as `(path inside the zip, content)`, in a fixed order.
pub fn kit_files(request: &str, system: &str) -> Vec<(String, Vec<u8>)> {
    let mut files = vec![
        ("START_HERE.md".to_string(), START_HERE.as_bytes().to_vec()),
        (
            "REQUEST.md".to_string(),
            request_file(request, system).into_bytes(),
        ),
        ("FLOWS_FOR_AI.md".to_string(), GUIDE.as_bytes().to_vec()),
        ("flow.schema.json".to_string(), SCHEMA.as_bytes().to_vec()),
    ];
    files.extend(
        EXAMPLES
            .iter()
            .map(|(name, text)| ((*name).to_string(), text.as_bytes().to_vec())),
    );
    files
}

/// The CRC-32 table (polynomial `0xEDB88320`), computed at compile time.
const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

static CRC_TABLE: [u32; 256] = crc_table();

/// The CRC-32 of `data`, as a zip file stores it.
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c = CRC_TABLE[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

/// 1980-01-01 00:00, the date every entry carries: the same input always gives the same bytes.
const DOS_DATE: u16 = (1 << 5) | 1;

fn put16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// A zip archive of `files` with every entry *stored* (not compressed).
///
/// # Errors
///
/// A name that is empty, absolute, holds `..`, `\` or a character outside printable ASCII, more
/// than 65535 files, or a file or total over 4 GB (none of which the kit comes near).
pub fn zip_stored(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let bad = name.is_empty()
            || name.starts_with('/')
            || name.contains('\\')
            || name.split('/').any(|part| part == "..")
            || !name.bytes().all(|b| (0x20..0x7F).contains(&b));
        if bad {
            return Err(format!("not a name for a zip entry: {name:?}"));
        }
        let size = u32::try_from(data.len()).map_err(|_| "a file is too big for a zip")?;
        let offset = u32::try_from(out.len()).map_err(|_| "the zip is too big")?;
        let crc = crc32(data);
        let name_len = u16::try_from(name.len()).map_err(|_| format!("name too long: {name:?}"))?;
        // local file header
        put32(&mut out, 0x0403_4b50);
        put16(&mut out, 20); // version needed
        put16(&mut out, 0); // flags: names are ASCII
        put16(&mut out, 0); // method: stored
        put16(&mut out, 0); // time
        put16(&mut out, DOS_DATE);
        put32(&mut out, crc);
        put32(&mut out, size); // compressed
        put32(&mut out, size); // uncompressed
        put16(&mut out, name_len);
        put16(&mut out, 0); // extra
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        // central directory header
        put32(&mut central, 0x0201_4b50);
        put16(&mut central, (3 << 8) | 20); // made by: Unix, 2.0
        put16(&mut central, 20);
        put16(&mut central, 0);
        put16(&mut central, 0);
        put16(&mut central, 0);
        put16(&mut central, DOS_DATE);
        put32(&mut central, crc);
        put32(&mut central, size);
        put32(&mut central, size);
        put16(&mut central, name_len);
        put16(&mut central, 0); // extra
        put16(&mut central, 0); // comment
        put16(&mut central, 0); // disk
        put16(&mut central, 0); // internal attributes
        put32(&mut central, 0o100_644 << 16); // a regular file, rw-r--r--
        put32(&mut central, offset);
        central.extend_from_slice(name.as_bytes());
    }
    let start = u32::try_from(out.len()).map_err(|_| "the zip is too big")?;
    let size = u32::try_from(central.len()).map_err(|_| "the zip is too big")?;
    out.extend_from_slice(&central);
    // end of central directory
    put32(&mut out, 0x0605_4b50);
    put16(&mut out, 0);
    put16(&mut out, 0);
    let count = u16::try_from(files.len()).map_err(|_| "too many files for a zip")?;
    put16(&mut out, count);
    put16(&mut out, count);
    put32(&mut out, size);
    put32(&mut out, start);
    put16(&mut out, 0);
    Ok(out)
}

/// Writes the kit into `dir` (made if missing) as [`KIT_FILE`], replacing the one that was there,
/// and returns its path. The file appears whole or not at all: it is written beside the target and
/// renamed.
///
/// # Errors
///
/// What the file system says, or the reason the zip could not be made.
pub fn write_kit(dir: &Path, request: &str, system: &str) -> io::Result<PathBuf> {
    let bytes = zip_stored(&kit_files(request, system))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::create_dir_all(dir)?;
    let target = dir.join(KIT_FILE);
    let partial = dir.join(format!("{KIT_FILE}.part"));
    fs::write(&partial, bytes)?;
    fs::rename(&partial, &target)?;
    Ok(target)
}

/// The folder the app writes the kit to: [`KIT_DIR`] inside the app support directory.
pub fn kit_dir() -> PathBuf {
    super::app_support_dir().join(KIT_DIR)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cursorpie-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn le16(b: &[u8], at: usize) -> usize {
        usize::from(u16::from_le_bytes([b[at], b[at + 1]]))
    }

    fn le32(b: &[u8], at: usize) -> usize {
        u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as usize
    }

    /// Reads a stored zip back through its central directory, checking every checksum.
    fn unzip(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        let end = bytes.len() - 22;
        assert_eq!(le32(bytes, end), 0x0605_4b50, "end of central directory");
        let count = le16(bytes, end + 10);
        let mut at = le32(bytes, end + 16);
        let mut found = Vec::new();
        for _ in 0..count {
            assert_eq!(le32(bytes, at), 0x0201_4b50, "central header");
            let crc = le32(bytes, at + 16);
            let size = le32(bytes, at + 24);
            let name_len = le16(bytes, at + 28);
            let skip = le16(bytes, at + 30) + le16(bytes, at + 32);
            let local = le32(bytes, at + 42);
            let name = String::from_utf8(bytes[at + 46..at + 46 + name_len].to_vec()).unwrap();
            assert_eq!(le32(bytes, local), 0x0403_4b50, "local header");
            assert_eq!(le16(bytes, local + 8), 0, "stored");
            let start = local + 30 + le16(bytes, local + 26) + le16(bytes, local + 28);
            let data = bytes[start..start + size].to_vec();
            assert_eq!(crc32(&data) as usize, crc, "checksum of {name}");
            found.push((name, data));
            at += 46 + name_len + skip;
        }
        found
    }

    #[test]
    fn the_checksum_is_the_standard_one() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn a_zip_reads_back_exactly() {
        let files = vec![
            ("a.txt".to_string(), b"hello".to_vec()),
            ("dir/b.json".to_string(), Vec::new()),
            ("dir/c.md".to_string(), "caf\u{e9}".as_bytes().to_vec()),
        ];
        assert_eq!(unzip(&zip_stored(&files).unwrap()), files);
        assert_eq!(unzip(&zip_stored(&[]).unwrap()), vec![]);
    }

    #[test]
    fn the_same_input_gives_the_same_bytes() {
        let files = kit_files("open Notes", "macOS");
        assert_eq!(zip_stored(&files), zip_stored(&files));
    }

    #[test]
    fn names_that_could_escape_the_folder_are_refused() {
        for bad in [
            "",
            "/etc/x",
            "../x",
            "a/../x",
            "a\\b",
            "caf\u{e9}.txt",
            "a\nb",
        ] {
            assert!(
                zip_stored(&[(bad.to_string(), vec![1])]).is_err(),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn the_kit_holds_the_guides_the_request_and_the_examples() {
        let files = kit_files("copy the newest file", "Windows");
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        for want in [
            "START_HERE.md",
            "REQUEST.md",
            "FLOWS_FOR_AI.md",
            "flow.schema.json",
        ] {
            assert!(names.contains(&want), "{want}");
        }
        assert_eq!(
            names
                .iter()
                .filter(|n| n.starts_with("examples/flows/"))
                .count(),
            EXAMPLES.len()
        );
        let request = &files.iter().find(|(n, _)| n == "REQUEST.md").unwrap().1;
        let request = String::from_utf8(request.clone()).unwrap();
        assert!(request.contains("copy the newest file") && request.contains("Windows"));
        // the names are valid entries
        assert!(zip_stored(&files).is_ok());
    }

    #[test]
    fn the_guide_describes_every_example_of_the_kit() {
        for (path, _) in EXAMPLES {
            let file = path.rsplit('/').next().unwrap_or(path);
            assert!(
                GUIDE.contains(file),
                "{file} is not in docs/FLOWS_FOR_AI.md"
            );
        }
    }

    #[test]
    fn the_kit_has_the_chat_examples_and_each_is_a_flow_file() {
        let files = kit_files("answer my chats", "Linux");
        for want in [
            "examples/flows/quick-reply-unread-chats.cursorpie-flow.json",
            "examples/flows/send-then-wait-for-answer.cursorpie-flow.json",
        ] {
            let (_, bytes) = files
                .iter()
                .find(|(n, _)| n == want)
                .unwrap_or_else(|| panic!("{want} is not in the kit"));
            let text = String::from_utf8(bytes.clone()).unwrap();
            assert!(crate::config::flow::parse_flow(&text).is_ok(), "{want}");
        }
    }

    #[test]
    fn the_start_page_names_every_file_it_sends_the_agent_to() {
        for name in [
            "REQUEST.md",
            "FLOWS_FOR_AI.md",
            "flow.schema.json",
            "examples/flows/",
        ] {
            assert!(START_HERE.contains(name), "{name}");
        }
        assert!(START_HERE.contains(".cursorpie-flow.json"));
    }

    #[test]
    fn a_long_request_is_cut_and_an_empty_one_says_so() {
        let long = "q".repeat(MAX_REQUEST_CHARS + 500);
        let text = request_file(&long, "Linux");
        assert_eq!(text.matches('q').count(), MAX_REQUEST_CHARS);
        assert!(request_file("  \n ", "Linux").contains("did not describe"));
    }

    #[test]
    fn writing_the_kit_replaces_the_old_one_and_leaves_nothing_else() {
        let d = scratch("flow-kit");
        let first = write_kit(&d, "one", "macOS").unwrap();
        let second = write_kit(&d, "two", "macOS").unwrap();
        assert_eq!(first, second);
        let files = unzip(&fs::read(&second).unwrap());
        let request = files.iter().find(|(n, _)| n == "REQUEST.md").unwrap();
        assert!(String::from_utf8_lossy(&request.1).contains("two"));
        assert_eq!(fs::read_dir(&d).unwrap().count(), 1, "no .part file left");
        let _ = fs::remove_dir_all(&d);
    }
}
