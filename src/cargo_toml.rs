use std::io::{self, BufRead, Read, Write};

use crate::Version;

pub fn update_cargo_toml<In: Read, Out: Write>(
    input: io::BufReader<In>,
    mut output: Out,
    updates: &[(&str, &Version)],
    override_repository: &str,
) -> io::Result<()> {
    //let mut group = String::new();
    let mut new_revision = None;
    let mut saw_rev = false;

    for line in input.lines() {
        let line = line?;

        let trimmed = line.trim().split('#').next().unwrap();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            //group = trimmed.to_string();
            new_revision = None;
            saw_rev = false;

            let group_name = trimmed.trim_start_matches('[').trim_end_matches(']');
            for (crate_name, new_rev) in updates {
                //println!("{:?} {:?}", crate_name, group_name);
                if group_name.ends_with(&format!("dependencies.{crate_name}")) {
                    new_revision = Some(new_rev);
                    break;
                }
            }
        }

        let tokens = tokenize(&line);

        if let Some(package_name) =
            parse_package_name(tokens.clone()).or_else(|| parse_git(tokens.clone()))
        {
            if saw_rev {
                eprintln!("Warning: found the package name or url after the revision or version, updates may have been missed.");
            }

            for (crate_name, new_rev) in updates {
                if package_name == *crate_name {
                    new_revision = Some(new_rev);
                    break;
                }
            }
        }

        if let Some(Version { git_hash, semver }) = new_revision {
            if parse_git(tokens.clone())
                .map(|url| url.contains("wgpu"))
                .unwrap_or(false)
            {
                writeln!(output, "git = \"{override_repository}\"")?;
                continue;
            }

            if !git_hash.is_empty() && parse_rev(tokens.clone()).is_some() {
                saw_rev = true;
                writeln!(output, "rev = \"{git_hash}\"")?;
                continue;
            }

            if !semver.is_empty() && parse_version(tokens.clone()).is_some() {
                saw_rev = true;
                writeln!(output, "version = \"{semver}\"")?;
                continue;
            }
        }

        writeln!(output, "{line}")?;
    }

    Ok(())
}

fn tokenize(src: &str) -> impl Iterator<Item = &str> + Clone {
    let trimmed = src.trim().split('#').next().unwrap();
    trimmed.split_ascii_whitespace()
}

fn parse_string_attribute<'a, 'b>(
    mut src: impl Iterator<Item = &'a str>,
    attrib_name: &'b str,
) -> Option<&'a str> {
    if src.next() == Some(attrib_name) && src.next() == Some("=") {
        let name = src.next()?.strip_prefix('"')?.strip_suffix('"')?;
        return Some(name);
    }

    None
}

fn parse_package_name<'a>(src: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    parse_string_attribute(src, "package")
}

fn parse_rev<'a>(src: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    parse_string_attribute(src, "rev")
}

fn parse_version<'a>(src: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    parse_string_attribute(src, "version")
}

fn parse_git<'a>(src: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    parse_string_attribute(src, "git")
}
