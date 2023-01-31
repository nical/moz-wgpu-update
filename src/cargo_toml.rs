use std::io::{self, Read, Write, BufRead};

pub fn update_cargo_toml<In: Read, Out: Write>(
    input: io::BufReader<In>,
    mut output: Out,
    updates: &[(&str, &str)],
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
        }

        let tokens = tokenize(&line);

        if let Some(package_name) = parse_package_name(tokens.clone()).or_else(|| parse_git(tokens.clone())) {
            if saw_rev {
                eprintln!("Warning: found the package name or url after the revision, updates may have been missed.");
            }

            for (crate_name, new_rev) in updates {
                if package_name == *crate_name {
                    new_revision = Some(new_rev);
                    break;
                }
            }
        }

        if let Some(new_rev) = new_revision {
            if let Some(_old_rev) = parse_rev(tokens.clone()) {
                saw_rev = true;
                writeln!(output, "rev = \"{new_rev}\"")?;
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

fn parse_string_attribute<'a, 'b>(mut src: impl Iterator<Item = &'a str>, attrib_name: &'b str) -> Option<&'a str> {
    if src.next() == Some(attrib_name) && src.next() == Some("=") {
        let name = src.next()?
            .strip_prefix('"')?
            .strip_suffix('"')?;
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

fn parse_git<'a>(src: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    parse_string_attribute(src, "git")
}
