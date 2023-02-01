#![allow(clippy::inherent_to_string)]

mod cargo_toml;
mod moz_yaml;
mod cargo_lock;
mod update;
mod helpers;

use std::{path::PathBuf, fs::File, io::{self, Read}};
use std::process::Command;
use clap::Parser;
use serde_derive::{Serialize, Deserialize};
use update::{UpdateArgs};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub enum Args {
    /// Update wgpu in mozilla-central.
    Update(UpdateArgs),
    /// File a bug for the update.
    Bugzilla(helpers::BugzillaArgs),
    /// Run a mach command in the mozilla-central directory.
    Mach(helpers::MachArgs),
    /// Run `hg histedit` in mozilla-central.
    Histedit,
    /// Push a try run to Firefox's CI.
    Try,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Directories {
    pub mozilla_central: PathBuf,
    pub wgpu: PathBuf,
    pub naga: PathBuf,
}

#[derive(Serialize, Deserialize, Debug)]
struct Config {
    pub directories: Directories,
    pub vcs: Option<String>,
}

#[derive(Copy, Clone)]
pub enum Vcs {
    Mercurial,
    Git,
}

impl Vcs {
    pub fn new(string: &Option<String>) -> Self {
        let vcs_str = string.as_ref().map(String::as_str).unwrap_or("hg").to_lowercase();
        match vcs_str.as_str() {
            "hg" | "mercurial" => Vcs::Mercurial,
            "git" => Vcs::Git,
            _ => panic!("Unsupported version control system {vcs_str:?}")
        }
    }
}

fn read_config_file(path: &Option<PathBuf>) -> io::Result<Config> {
    let cfg_path = path.clone().unwrap_or_else(|| "./wgpu_update.toml".into());
    let mut config_file = File::open(cfg_path)?;

    let mut buf = String::new();
    config_file.read_to_string(&mut buf)?;
    let config: Config = toml::from_str(&buf).unwrap();

    Ok(config)
}

fn shell(directory: &PathBuf, cmd: &str, args: &[&str]) {
    let mut cmd_str = format!("{cmd} ");
    for arg in args {
        cmd_str.push_str(arg);
        cmd_str.push(' ');
    }
    println!("Running {cmd_str:?}");

    Command::new(cmd)
        .args(args)
        .current_dir(directory)
        .spawn()
        .unwrap()
        .wait()
        .unwrap();
}

fn main() -> io::Result<()> {
    match &Args::parse() {
        Args::Update(args) => update::update_command(args),
        Args::Bugzilla(args) => helpers::file_bug(args),
        Args::Mach(args) => helpers::run_mach_command(args),
        Args::Try => helpers::push_to_try(),
        Args::Histedit => helpers::hg_histedit(),
    }
}
