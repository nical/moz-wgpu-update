use std::io;
use clap::Parser;

use crate::{Vcs, read_config_file, shell};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct BugzillaArgs {
    message: Option<String>,

    /// Open the bugzilla url in firefox.
    #[arg(short, long)]
    open: bool
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct MachArgs {
    command: Vec<String>,
}


pub fn file_bug(args: &BugzillaArgs) -> io::Result<()> {
    let mut url = "https://bugzilla.mozilla.org/enter_bug.cgi?".to_string();
    url.push_str("&assigned_to=nobody%40mozilla.org");
    url.push_str("&product=Core");
    url.push_str("&component=Graphics%3A%20WebGPU");
    url.push_str("&priority=P3");
    url.push_str("&bug_severity=N%2FA");
    url.push_str("&bug_type=task");
    url.push_str("&bug_status=NEW");
    if let Some(message) = &args.message {
        let msg = message
            .replace(' ', "%20")
            .replace('(', "%28")
            .replace(')', "%29")
            .replace('/', "%2F")
            .replace(':', "%3A")
            .replace('[', "%5B")
            .replace(']', "%5D")
            .replace('|', "%7C")
            .replace('~', "%7E")
            .replace('*', "%2A")
            .replace('@', "%40")
            .replace('\'', "%27");
        url.push_str("&short_desc=");
        url.push_str(&msg);
    }

    println!("{url}");

    if args.open {
        shell(&".".into(), "firefox", &[&url]);
    }

    Ok(())
}

pub fn hg_histedit() -> io::Result<()> {
    let config = read_config_file(&None)?;

    match Vcs::new(&config.vcs) {
        Vcs::Mercurial =>shell(&config.directories.mozilla_central, "git", &["rebasse", "-i", "central"]),
        Vcs::Git => shell(&config.directories.mozilla_central, "hg", &["histedit"]),
    };

    Ok(())
}

pub fn run_mach_command(args: &MachArgs) -> io::Result<()> {
    let config = read_config_file(&None)?;

    println!("mach args: {:?}", args.command);
    let arg_refs: Vec<&str> = args.command.iter().map(String::as_str).collect();

    shell(&config.directories.mozilla_central, "./mach", &arg_refs);

    Ok(())
}

pub fn push_to_try() -> io::Result<()> {
    let config = read_config_file(&None)?;

    shell(&config.directories.mozilla_central, "./mach", &["try", "--preset", "webgpu"]);

    Ok(())
}

