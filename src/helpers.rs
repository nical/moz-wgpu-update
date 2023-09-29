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
    let mut params = vec![
        ("assigned_to", "nobody@mozilla.org"),
        ("product", "Core"),
        ("component", "Graphics: WebGPU"),
        ("priority", "P3"),
        ("bug_severity", "N/A"),
        ("bug_type", "task"),
        ("bug_status", "NEW"),
        ("blocked", "webgpu-update-wgpu"),
    ];
    if let Some(msg) = &args.message {
        params.push(("short_desc", msg));
    }
    let url =
        url::Url::parse_with_params("https://bugzilla.mozilla.org/enter_bug.cgi", params).unwrap();

    println!("{url}");

    if args.open {
        shell(&PathBuf::from("."), "firefox", &[url.as_str()])?;
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
        Vcs::Git => shell(&config.gecko.path, "git", &["rebase", "-i", "central"])?,
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
