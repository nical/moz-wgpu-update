use std::io::{self, BufRead};
use std::fs::File;
use crate::Config;

// Try to extract the wgpu semver version and commit hash from mozilla-central's Cargo.lock.
pub fn find_previous_wgpu_version(config: &Config) -> io::Result<(String, String)> {
    let mut cargo_lock_path = config.directories.mozilla_central.clone();
    cargo_lock_path.push("Cargo.lock");

    let mut reached_wgpu_package = false;
    let mut version = None;
    let mut revision = None;

    for line in io::BufReader::new(File::open(&cargo_lock_path)?).lines() {
        let line = line.unwrap();
        if line.starts_with("name = \"wgpu-core\"") {
            reached_wgpu_package = true;
        }

        if line.starts_with("[[package]]") {
            reached_wgpu_package = false;
        }

        if !reached_wgpu_package {
            continue;
        }

        if line.starts_with("version = ") {
            version = Some(line[11..].trim_matches('"').to_string());
        }

        if line.starts_with("source = ") {
            // parsing something of the form "source = "git+https://github.com/gfx-rs/wgpu?rev={revision}#{revision}"
            let start = line.find("rev=").unwrap() + 4;
            let end = line.rfind('#').unwrap_or(line.len());
            revision = Some(line[start..end].to_string());
        }

        if let (Some(version), Some(revision)) = (&version, &revision) {
            return Ok((version.clone(), revision.clone()));
        }
    }

    let error = match (version.is_some(), revision.is_some()) {
        (false, false) => "Could not find wgpu-core in the Cargo.lock file.",
        (true, false) => "Could not find wgp-core's git revision in the Cargo.lock file.",
        (false, true) => "Could not find wgp-core's version in the Cargo.lock file.",
        (true, true) => { unreachable!() }
    };

    panic!("{}", error);
}