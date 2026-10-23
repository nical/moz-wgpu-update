use anyhow::Context;
use clap::Parser;
use std::{
    io,
    path::{Path, PathBuf},
};

use crate::{read_config_file, shell, Gecko, Vcs};

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

    match config.gecko.vcs {
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

pub fn push_to_try() -> io::Result<()> {
    let config = read_config_file(&None)?;

    shell(&config.gecko.path, "./mach", &["try", "--preset", "webgpu"])?;

    Ok(())
}

/// Search for a `mozilla-central` checkout either via Mercurial or Git, iterating from the CWD to its parent directories.
///
/// This function reports to `log` automatically, so no meaningful [`Err`] value is returned.
pub(crate) fn search_for_moz_central_ckt() -> Result<Gecko, ()> {
    use lets_find_up::{find_up_with, FindUpKind, FindUpOptions};

    let find_up_opts = || FindUpOptions {
        cwd: Path::new("."),
        kind: FindUpKind::Dir,
    };
    let find_up = |vcs, root_dir_name| {
        log::debug!("searching for {vcs:?} checkout of `mozilla-central`…");
        let err = || {
            format!(
                "failed to find a {:?} repository ({:?}) in {}",
                vcs, root_dir_name, "any of current working directory and its parent directories",
            )
        };
        find_up_with(root_dir_name, find_up_opts())
            .with_context(err)
            .and_then(|loc_opt| loc_opt.with_context(err))
            .map(|mut dir| {
                dir.pop();
                Gecko { path: dir, vcs }
            })
    };
    let gecko = find_up(Vcs::Mercurial, ".hg").or_else(|e| match find_up(Vcs::Git, ".git") {
        Ok(path) => {
            log::debug!("{e:?}");
            Ok(path)
        }
        Err(e2) => {
            log::warn!("{e:?}");
            log::warn!("{e2:?}");
            log::error!("failed to find a Gecko repository root");
            Err(())
        }
    })?;

    log::info!("detected Gecko repository root at {}", gecko.path.display());

    Ok(gecko)
}
