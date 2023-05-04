mod audit;
mod cargo_lock;
mod cargo_toml;
mod helpers;
mod moz_yaml;
mod naga_update;
mod wgpu_update;

use anyhow::bail;
use clap::Parser;
use format::lazy_format;
use serde_derive::{Deserialize, Serialize};
use std::{
    fmt::Display,
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    str::FromStr,
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub enum Args {
    /// Update `wgpu` in the `gecko` directory.
    WgpuUpdate(wgpu_update::Args),
    /// Update `naga` in `wgpu`.
    NagaUpdate(naga_update::Args),
    /// File a bug for the update.
    Bugzilla(helpers::BugzillaArgs),
    /// List commits to audit.
    Audit(audit::AuditArgs),
    /// Run a `mach` command in the `gecko` directory.
    Mach(helpers::MachArgs),
    /// Run `hg histedit` in the `gecko` directory.
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
    #[serde(default = "default_branch")]
    main_branch: String,
    #[serde(default = "default_remote")]
    upstream_remote: String,
    trusted_reviewers: Vec<String>,
    latest_commit: Option<PathBuf>,
}

fn default_branch() -> String {
    "main".into()
}

fn default_remote() -> String {
    "upstream".into()
}

#[derive(Default, Copy, Clone)]
pub enum Vcs {
    #[default]
    Mercurial,
    Git,
}

impl FromStr for Vcs {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "hg" | "mercurial" => Ok(Vcs::Mercurial),
            "git" => Ok(Vcs::Git),
            _ => bail!("Unsupported version control system {s:?}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    pub semver: String,
    pub git_hash: String,
}

impl Version {
    /// The semver/git hash pair formatted in the way cargo vet expects, or just the
    /// semver string if there is no git hash.
    pub fn display_cargo_vet(&self) -> impl Display + '_ {
        lazy_format!(|f| {
            if self.git_hash.is_empty() {
                return write!(f, "{}", self.semver);
            }

            write!(f, "{}@git:{}", self.semver, self.git_hash)
        })
    }

    fn from_git_checkout(project: &GithubProject, pull: bool) -> io::Result<Self> {
        println!("Detecting crate version from local checkout.");
        let current_branch =
            read_shell(&project.path, "git", &["rev-parse", "--abbrev-ref", "HEAD"]).stdout;
        let current_branch = current_branch.trim();

        let upstream = &project.upstream_remote;
        let main_branch = &&project.main_branch;

        if pull {
            // Temporarily switch to the main branch.
            shell(
                &project.path,
                "git",
                &["commit", "-am", "Uncommitted changes before update."],
            )?;
            shell(&project.path, "git", &["checkout", main_branch])?;
            shell(
                &project.path,
                "git",
                &["pull", &project.upstream_remote, main_branch],
            )?;
        }

        let git_hash = read_shell(
            &project.path,
            "git",
            &["rev-parse", &format!("{upstream}/{main_branch}")],
        )
        .stdout
        .trim()
        .to_string();

        let cargo_toml_path = concat_path(&project.path, "Cargo.toml");
        let reader = io::BufReader::new(File::open(cargo_toml_path)?);
        let semver = cargo_toml::get_package_attribute(reader, "version")?.unwrap();

        if pull {
            // Switch back to the previous branch.
            shell(&project.path, "git", &["checkout", current_branch])?;
        }

        Ok(Self { semver, git_hash })
    }
}

fn read_config_file(path: &Option<PathBuf>) -> io::Result<Config> {
    let in_current_dir = PathBuf::from("./.moz-wgpu.toml");
    let in_home = dirs::home_dir().map(|mut path| {
        path.push(".moz-wgpu.toml");
        path
    });

    let mut config_file = if let Some(path) = path {
        File::open(path)?
    } else if let Ok(file) = File::open(&in_current_dir) {
        file
    } else if let Some(in_home) = in_home {
        File::open(&in_home).ok().unwrap_or_else(|| {
            panic!(
                "Could not find config file. Searched locations \n{in_current_dir:?}\n{in_home:?}"
            );
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

pub struct ShellOutput {
    pub stdout: String,
    pub stderr: String,
}

/// Execute a command and read stdout into a string.
///
/// Note that the resulting string will likely have a \n at the end, even
/// if only one line was written.
fn read_shell(directory: &Path, cmd: &str, args: &[&str]) -> ShellOutput {
    let mut cmd_str = format!("{cmd} ");
    for arg in args {
        cmd_str.push_str(arg);
        cmd_str.push(' ');
    }
    println!(" -- Running {cmd_str:?}");

    let output = Command::new(cmd)
        .args(args)
        .current_dir(directory)
        .output()
        .unwrap();

    ShellOutput {
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

pub fn concat_path(a: &Path, b: &str) -> PathBuf {
    let mut path = a.to_path_buf();
    if !b.is_empty() {
        path.push(&PathBuf::from(b));
    }

    path
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
