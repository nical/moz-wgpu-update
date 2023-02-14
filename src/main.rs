#![allow(clippy::inherent_to_string)]

mod cargo_toml;
mod moz_yaml;
mod cargo_lock;
mod wgpu_update;
mod naga_update;
mod helpers;
mod audit;

use std::{path::{Path, PathBuf}, fs::File, io::{self, Read}, process::ExitStatus};
use std::process::Command;
use clap::Parser;
use serde_derive::{Serialize, Deserialize};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub enum Args {
    /// Update wgpu in mozilla-central.
    WgpuUpdate(wgpu_update::Args),
    /// Update naga in wgpu.
    NagaUpdate(naga_update::Args),
    /// File a bug for the update.
    Bugzilla(helpers::BugzillaArgs),
    /// List commits to audit.
    Audit(audit::AuditArgs),
    /// Run a mach command in the mozilla-central directory.
    Mach(helpers::MachArgs),
    /// Run `hg histedit` in mozilla-central.
    Histedit,
    /// Push a try run to Firefox's CI.
    Try,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
struct Config {
    gecko: Gecko,
    wgpu: GithubProject,
    naga: GithubProject,
    github_api_token: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Gecko {
    path: PathBuf,
    vcs: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct GithubProject {
    path: PathBuf,
    upstream_remote: Option<String>,
    trusted_reviewers: Vec<String>,
    latest_commit: Option<PathBuf>,
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

#[derive(Clone, Debug)]
pub struct Version {
    pub semver: String,
    pub git_hash: String,
}

impl Version {
    /// The semver/git hash pair formatted in the way cargo vet expects, or just the
    /// semver string if there is no git hash.
    pub fn to_string(&self) -> String {
        if self.git_hash.is_empty() {
            return self.semver.clone()
        }

        format!("{}@git:{}", self.semver, self.git_hash)
    }
}

fn read_config_file(path: &Option<PathBuf>) -> io::Result<Config> {
    let in_current_dir = PathBuf::from("./.moz-wgpu.toml");
    let in_home = dirs::home_dir().map(|mut path| { path.push(".moz-wgpu.toml"); path });

    let mut config_file = if let Some(path) = path {
        File::open(path)?
    } else if let Ok(file) = File::open(&in_current_dir) {
        file
    } else if let Some(in_home) = in_home {
        File::open(&in_home).ok().unwrap_or_else(|| {
            panic!("Could not find config file. Seached locations \n{in_current_dir:?}\n{in_home:?}");
        })
    } else {
        panic!();
    };

    let mut buf = String::new();
    config_file.read_to_string(&mut buf)?;
    let config: Config = toml::from_str(&buf).unwrap();

    Ok(config)
}

fn shell(directory: &Path, cmd: &str, args: &[&str]) -> io::Result<ExitStatus> {
    let mut cmd_str = format!("{cmd} ");
    for arg in args {
        cmd_str.push_str(arg);
        cmd_str.push(' ');
    }
    println!(" -- Running {cmd_str:?}");

    Command::new(cmd).args(args).current_dir(directory).status()
}

/// Execute a command and read stdout into a string.
///
/// Note that the resulting string will likely have a \n at the end, even
/// if only one line was written.
fn read_shell(directory: &Path, cmd: &str, args: &[&str]) -> String {
    let mut cmd_str = format!("{cmd} ");
    for arg in args {
        cmd_str.push_str(arg);
        cmd_str.push(' ');
    }
    println!(" -- Running {cmd_str:?}");

    let bytes = Command::new(cmd)
        .args(args)
        .current_dir(directory)
        .output()
        .unwrap()
        .stdout;

    String::from_utf8(bytes).unwrap()
}

pub fn concat_path(a: &Path, b: &str) -> PathBuf {
    let mut path = a.to_path_buf();
    if !b.is_empty() {
        path.push(&PathBuf::from(b));
    }

    path
}

fn crate_version_from_checkout(path: &Path, upstream: &str, pull: bool) -> io::Result<Version> {
    println!("Detecting crate version from local checkout.");
    let current_branch = read_shell(path, "git", &["rev-parse", "--abbrev-ref", "HEAD"]);
    let current_branch = current_branch.trim();

    if pull {
        // Temporarily switch to the master branch.
        shell(path, "git", &["commit", "-am", "Uncommitted changes before update."])?;
        shell(path, "git", &["checkout", "master"])?;
        shell(path, "git", &["pull", upstream, "master"])?;
    }

    let git_hash = read_shell(path, "git", &["rev-parse", &format!("{upstream}/master")]).trim().to_string();

    let cargo_toml_path = concat_path(path, "Cargo.toml");
    let reader = io::BufReader::new(File::open(cargo_toml_path)?);
    let semver = cargo_toml::get_package_attribute(reader, "version")?.unwrap();

    if pull {
        // Switch back to the previous branch.
        shell(path, "git", &["checkout", current_branch])?;
    }

    Ok(Version { semver, git_hash })
}

fn main() -> io::Result<()> {
    match &Args::parse() {
        Args::WgpuUpdate(args) => wgpu_update::update_command(args),
        Args::NagaUpdate(args) => naga_update::update_command(args),
        Args::Bugzilla(args) => helpers::file_bug(args),
        Args::Audit(args) => audit::find_commits_to_audit(args),
        Args::Mach(args) => helpers::run_mach_command(args),
        Args::Try => helpers::push_to_try(),
        Args::Histedit => helpers::hg_histedit(),
    }
}
