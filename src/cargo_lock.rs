use std::io::{self, BufRead};
use std::fs::File;
use std::path::Path;
use crate::update::Version;

// Try to extract the wgpu semver version and commit hash from mozilla-central's Cargo.lock.
pub fn find_version(name: &str, gecko_path: &Path) -> io::Result<Version> {
    let mut cargo_lock_path = gecko_path.to_path_buf();
    cargo_lock_path.push("Cargo.lock");

    let mut found_package = false;
    let mut semver = None;
    let mut git_hash = None;

    let package_name_line = format!("name = \"{name}\"");

    for line in io::BufReader::new(File::open(&cargo_lock_path)?).lines() {
        let line = line.unwrap();
        if line.starts_with(&package_name_line) {
            found_package = true;
        }

        if line.starts_with("[[package]]") && found_package {
            break;
        }

        if !found_package {
            continue;
        }

        if line.starts_with("version = ") {
            semver = Some(line[11..].trim_matches('"').to_string());
        }

        if line.starts_with("source = \"git") {
            // Parsing something of the form "source = "git+https://github.com/gfx-rs/wgpu?rev={short_hash}#{long_hash}"
            let start = line.rfind('#').unwrap_or(line.len() - 1) + 1;
            let end = start.max(line.len() - 1);
            git_hash = Some(line[start..end].to_string());
        }

        if line.starts_with("source = \"registry") {
            git_hash = Some(String::new());
        }
    }

    assert!(found_package, "Could no find {name} in the Cargo.toml file.");

    Ok(Version {
        semver: semver.unwrap(),
        git_hash: git_hash.unwrap(),
    })
}