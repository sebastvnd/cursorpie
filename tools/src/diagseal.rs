//! The seal of `diag/`: what was published to crates.io as `cursorpie-diag`, and the guard that a change
//! to `diag/` never goes out under a version that is already published.
//!
//! `cargo publish` of the app removes the `path` of its `diag` dependency and takes the crate of that
//! version from crates.io. When `diag/` changed (a new module the app uses) without a new version, the
//! package of the app does not compile there: `unresolved import diag::problems`. This is noticed only
//! after minutes of packaging, so `cargo tools check` (part of `cargo tools ci`) notices it earlier.
//!
//! `cargo tools diag-seal`, run right after `cargo publish -p cursorpie-diag`, writes `diag/.published`:
//! the version and the SHA-256 of every file under `diag/` (without `target/`). `check` then reports a
//! `diag/` whose files differ from the seal while its version is still the sealed one. A different version
//! passes: it is the bump, the publish is next. Without a seal file nothing is checked.

use crate::sha256::{Sha256, hex_of};
use std::fs;
use std::path::Path;

/// The seal file, from the root of the repository.
const SEAL: &str = "diag/.published";

/// The version of the package in a `Cargo.toml`, from its `[package]` table.
fn version_of(manifest: &str) -> Option<&str> {
    let mut in_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package && let Some(rest) = line.strip_prefix("version") {
            return rest
                .trim_start()
                .strip_prefix('=')?
                .trim()
                .strip_prefix('"')?
                .split('"')
                .next();
        }
    }
    None
}

/// Every file under `dir`, as `/`-separated paths relative to `base`, sorted; `target/` and the seal
/// are not part of the package.
fn files(base: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        if matches!(name.as_deref(), Some("target" | ".published")) {
            continue;
        }
        if path.is_dir() {
            files(base, &path, out)?;
        } else if let Ok(rel) = path.strip_prefix(base) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    out.sort_unstable();
    Ok(())
}

/// The SHA-256 of the package folder: each path and the hash of its content, with the line endings
/// of a Windows checkout made the same as the others.
fn content_hash(dir: &Path) -> Result<String, String> {
    let mut names = Vec::new();
    files(dir, dir, &mut names)?;
    let mut all = Sha256::new();
    for name in names {
        let bytes = fs::read(dir.join(&name)).map_err(|e| format!("{name}: {e}"))?;
        let text: Vec<u8> = String::from_utf8_lossy(&bytes)
            .replace("\r\n", "\n")
            .into_bytes();
        all.update(format!("{name}\0{}\n", hex_of(&text)).as_bytes());
    }
    Ok(all.finish_hex())
}

/// The sealed version and hash, when there is a seal.
fn sealed(root: &Path) -> Option<(String, String)> {
    let text = fs::read_to_string(root.join(SEAL)).ok()?;
    let (version, hash) = text.split_once(' ')?;
    Some((version.to_string(), hash.trim().to_string()))
}

/// What is wrong with `diag/`: empty when it is as sealed, bumped since, or not sealed yet.
///
/// # Errors
/// When `diag/` cannot be read.
pub fn problems(root: &Path) -> Result<Vec<String>, String> {
    let dir = root.join("diag");
    let Ok(manifest) = fs::read_to_string(dir.join("Cargo.toml")) else {
        return Ok(Vec::new());
    };
    let (Some(version), Some((sealed_version, sealed_hash))) =
        (version_of(&manifest), sealed(root))
    else {
        return Ok(Vec::new());
    };
    if version == sealed_version && content_hash(&dir)? != sealed_hash {
        return Ok(vec![format!(
            "diag/ changed since cursorpie-diag {version} was published: raise its version in diag/Cargo.toml \
             and in the diag line of Cargo.toml, publish it (`cargo publish -p cursorpie-diag`), then run \
             `cargo tools diag-seal`"
        )]);
    }
    Ok(Vec::new())
}

/// `cargo tools diag-seal`: writes the seal for the `diag/` that was just published.
///
/// # Errors
/// When `diag/` cannot be read or the seal cannot be written.
pub fn seal(root: &Path) -> Result<(), String> {
    let dir = root.join("diag");
    let manifest =
        fs::read_to_string(dir.join("Cargo.toml")).map_err(|e| format!("diag/Cargo.toml: {e}"))?;
    let version = version_of(&manifest).ok_or("diag/Cargo.toml has no version")?;
    let line = format!("{version} {}\n", content_hash(&dir)?);
    fs::write(root.join(SEAL), line).map_err(|e| format!("{SEAL}: {e}"))?;
    println!("Sealed cursorpie-diag {version} ({SEAL}). Commit that file.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throw-away repository folder with a `diag/` package; removed when dropped.
    struct Tree(std::path::PathBuf);

    impl Tree {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("cursorpie-diagseal-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("diag/src")).unwrap();
            let tree = Self(root);
            tree.write(
                "diag/Cargo.toml",
                "[package]\nname = \"cursorpie-diag\"\nversion = \"0.2.0\"\n",
            );
            tree.write("diag/src/lib.rs", "pub mod problems;\n");
            tree
        }
        fn write(&self, name: &str, text: &str) {
            fs::write(self.0.join(name), text).unwrap();
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_version_is_the_one_of_the_package_table() {
        let toml = "[package]\nname = \"x\"\nversion = \"1.2.3\"\n[dependencies]\nfoo = { version = \"9\" }\n";
        assert_eq!(version_of(toml), Some("1.2.3"));
        assert_eq!(version_of("[dependencies]\nversion = \"9\"\n"), None);
    }

    #[test]
    fn nothing_is_checked_before_the_first_seal() {
        let tree = Tree::new("none");
        assert!(problems(&tree.0).unwrap().is_empty());
    }

    #[test]
    fn a_change_under_the_sealed_version_is_reported() {
        let tree = Tree::new("changed");
        seal(&tree.0).unwrap();
        assert!(problems(&tree.0).unwrap().is_empty(), "as sealed");
        tree.write("diag/src/lib.rs", "pub mod problems;\npub mod more;\n");
        let found = problems(&tree.0).unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("0.2.0") && found[0].contains("diag-seal"));
    }

    #[test]
    fn a_new_version_is_the_way_out_and_a_new_seal_closes_it() {
        let tree = Tree::new("bump");
        seal(&tree.0).unwrap();
        tree.write("diag/src/lib.rs", "pub mod problems;\npub mod more;\n");
        tree.write(
            "diag/Cargo.toml",
            "[package]\nname = \"cursorpie-diag\"\nversion = \"0.3.0\"\n",
        );
        assert!(
            problems(&tree.0).unwrap().is_empty(),
            "bumped: publish is next"
        );
        seal(&tree.0).unwrap();
        tree.write("diag/src/lib.rs", "x\n");
        assert_eq!(
            problems(&tree.0).unwrap().len(),
            1,
            "the new version is sealed now"
        );
    }

    #[test]
    fn line_endings_and_target_do_not_count() {
        let tree = Tree::new("eol");
        seal(&tree.0).unwrap();
        tree.write("diag/src/lib.rs", "pub mod problems;\r\n");
        fs::create_dir_all(tree.0.join("diag/target")).unwrap();
        tree.write("diag/target/junk", "build output");
        assert!(problems(&tree.0).unwrap().is_empty());
    }

    #[test]
    fn a_repository_without_diag_has_nothing_to_check() {
        let dir =
            std::env::temp_dir().join(format!("cursorpie-diagseal-empty-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert!(problems(&dir).unwrap().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
