use clap::Parser;
use std::{io, path::PathBuf, str::FromStr};

use crate::{read_config_file, shell, Vcs};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct BugzillaArgs {
    message: Option<String>,

    /// Open the bugzilla url in firefox.
    #[arg(short, long)]
    open: bool,
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
    url.push_str("&blocked=webgpu-update-wgpu");
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
        shell(&PathBuf::from("."), "firefox", &[&url])?;
    }

    Ok(())
}

pub fn hg_histedit() -> io::Result<()> {
    let config = read_config_file(&None)?;

    match config
        .gecko
        .vcs
        .as_deref()
        .map(Vcs::from_str)
        .unwrap()
        .unwrap_or_default()
    {
        Vcs::Mercurial => shell(&config.gecko.path, "hg", &["histedit"])?,
        Vcs::Git => shell(&config.gecko.path, "git", &["rebasse", "-i", "central"])?,
    };

    Ok(())
}

pub fn run_mach_command(args: &MachArgs) -> io::Result<()> {
    let config = read_config_file(&None)?;

    println!("mach args: {:?}", args.command);
    let arg_refs: Vec<&str> = args.command.iter().map(String::as_str).collect();

    shell(&config.gecko.path, "./mach", &arg_refs)?;

    Ok(())
}

pub fn push_to_try(rebuild: Option<u8>) -> io::Result<()> {
    let config = read_config_file(&None)?;

    let mut args: Vec<&str> = vec!["try", "--preset", "webgpu"];
    let tmp;

    if let Some(count) = rebuild {
        tmp = format!("{count}");
        args.push("--rebuild");
        args.push(&tmp);
    }

    shell(&config.gecko.path, "./mach", &args)?;

    Ok(())
}
