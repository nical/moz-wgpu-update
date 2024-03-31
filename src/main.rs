mod audit;
mod cargo_lock;
mod cargo_toml;
mod cts;
mod helpers;
mod moz_yaml;
mod wgpu_update;

use anyhow::bail;
use clap::Parser;
use format::lazy_format;
use serde_derive::{Deserialize, Serialize};
use std::{
    env::{current_dir, set_current_dir},
    fmt::Display,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    str::FromStr,
};

const DEFAULT_WGPU_REPOSITORY: &'static str = "https://github.com/gfx-rs/wgpu";

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub enum Args {
    /// Update `wgpu` in the `gecko` directory.
    WgpuUpdate(wgpu_update::Args),
    /// File a bug for the update.
    Bugzilla(helpers::BugzillaArgs),
    /// List commits to audit.
    Audit(audit::AuditArgs),
    /// Run a `mach` command in the `gecko` directory.
    Mach(helpers::MachArgs),
    /// Run `hg histedit` in the `gecko` directory.
    Histedit,
    /// Push a try run to Firefox's CI.
    Try {
        /// Request that all jobs be re-run <REBUILD> times.
        #[arg(long)]
        rebuild: Option<u8>,
    },
    /// Update this tool to its latest version using cargo.
    SelfUpdate,
    /// CTS related commands.
    Cts(cts::Args),
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
struct Config {
    gecko: Gecko,
    wgpu: GithubProject,
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
    // This parameter allows the wgpu-update command to override the wgpu repository url and
    // point to a wgpu fork (typically for testing purposes).
    // For regular use cases it is fine to let it unset by default.
    repository: Option<String>,
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
        let cargo_toml_str = fs::read_to_string(cargo_toml_path)?;
        let semver = ::cargo_toml::Manifest::from_str(&cargo_toml_str)
            .unwrap()
            .package()
            .version()
            .to_owned();

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
                "Could not find config file. Searched locations {:#?}",
                [&in_current_dir, &in_home],
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
    let old_cwd = current_dir().unwrap();
    set_current_dir(directory).unwrap();
    let mut cmd_str = format!("{cmd} ");
    for arg in args {
        cmd_str.push_str(arg);
        cmd_str.push(' ');
    }
    println!(" -- Running {cmd_str:?}");

    let status = Command::new(cmd)
        .args(args)
        .current_dir(directory)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();
    set_current_dir(old_cwd).unwrap();
    status
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
    let old_cwd = current_dir().unwrap();
    set_current_dir(directory).unwrap();
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
    set_current_dir(old_cwd).unwrap();

    ShellOutput {
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

pub fn concat_path(a: &Path, b: &str) -> PathBuf {
    if b.is_empty() {
        a.to_path_buf()
    } else {
        a.join(&Path::new(b))
    }
}

fn main() -> io::Result<()> {
    match &Args::parse() {
        Args::WgpuUpdate(args) => wgpu_update::update_command(args),
        Args::Bugzilla(args) => helpers::file_bug(args),
        Args::Audit(args) => audit::find_commits_to_audit(args),
        Args::Mach(args) => helpers::run_mach_command(args),
        Args::Try { rebuild } => helpers::push_to_try(*rebuild),
        Args::Histedit => helpers::hg_histedit(),
        Args::SelfUpdate => self_update(),
        Args::Cts(args) => cts::command(args),
    }
}

fn self_update() -> io::Result<()> {
    shell(
        &current_dir().unwrap(),
        "cargo",
        &[
            "install",
            "--git",
            "https://github.com/nical/moz-wgpu-update",
        ],
    )?;
    shell(
        &current_dir().unwrap(),
        "cargo",
        &[
            "install",
            "--git",
            "https://github.com/ErichDonGubler/moz-webgpu-cts",
        ],
    )?;

    Ok(())
}
