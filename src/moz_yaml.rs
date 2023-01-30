use std::io::{self, Read, Write, BufRead};

pub fn update_moz_yaml<In: Read, Out: Write>(
    input: io::BufReader<In>,
    mut output: Out,
    updates: &[(&str, &str)],
) -> io::Result<Vec<String>> {

    let mut saw_rev_or_release = false;
    let mut new_revision = None;
    let mut prev_indent = String::new();

    let mut previous_revs = Vec::new();

    for line in input.lines() {
        let line = line?;

        if let Some((indent, key, value, comment)) = parse_line(&line[..]) {
            if &prev_indent[..] != indent {
                new_revision = None;
                prev_indent = indent.to_string();
                saw_rev_or_release = false;
            }
            let space_before_comment = if comment.is_empty() { "" } else { " " };
            match (key, new_revision) {
                ("url", _) | ("name", _) => {
                    for (name, new_rev) in updates {
                        if *name == value {
                            if saw_rev_or_release {
                                eprintln!("Warning: found url or name attribute after revision or release some updates may have been missed,");
                            }
                            new_revision = Some(new_rev);
                            break;
                        }
                    }
                }
                ("revision", Some(new_rev)) => {
                    writeln!(output, "{indent}revision: {new_rev}{space_before_comment}{comment}")?;
                    // TODO: should set at specific index matching the input updates instead of pushing in
                    // random order.
                    assert!(previous_revs.is_empty());
                    previous_revs.push(value.to_string());
                    saw_rev_or_release = true;
                    continue;
                }
                ("release", Some(new_rev)) => {
                    writeln!(output, "{indent}release: commit {new_rev}{space_before_comment}{comment}")?;
                    saw_rev_or_release = true;
                    continue;
                }
                _ => {}
            }
        }

        writeln!(output, "{}", line)?;
    }

    Ok(previous_revs)
}

fn split_indentation(src: &str) -> (&str, &str) {
    let mut indent = 0;
    for char in src.chars() {
        if char == ' ' || char == '\t' {
            indent += 1;
        } else {
            break;
        }
    }

    (&src[..indent], &src[indent..])
}

fn parse_line(src: &str) -> Option<(&str, &str, &str, &str)> {
    let (indentation, src) = split_indentation(src);

    // Here we make a gross simplification by assuming that # is only ever
    // used for comments and just strip everything after it out.
    let (src, comment) = match src.find('#') {
        Some(idx) => (&src[..idx], &src[idx..]),
        None => (src, ""),
    };

    let s = src.find(':')?;

    let key = src[..s].trim();
    let value = src[(s+1)..].trim();

    Some((indentation, key, value, comment))
}
