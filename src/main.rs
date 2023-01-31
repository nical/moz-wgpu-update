#![allow(clippy::inherent_to_string)]

mod cargo_toml;
mod moz_yaml;
mod cargo_lock;

use std::{path::PathBuf, fs::File, io::{self, Read, BufWriter}};
use std::process::Command;

use clap::Parser;
use serde_derive::{Serialize, Deserialize};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// The new wgpu revision (git hash) to update to.
    #[arg(short, long)]
    wgpu_rev: String,

    /// The previous wgpu revision (git hash) (if not specified, it is detected automatically from mozilla-central's Cargo.lock)
    #[arg(short, long)]
    previous_wgpu_rev: Option<String>,

    /// The bug number.
    #[arg(short, long)]
    bug: Option<String>,

    /// Config file to use.
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Whether to run start a build at the end.
    #[arg(long)]
    build: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Directories {
    pub mozilla_central: PathBuf,
    pub wgpu: PathBuf,
    pub naga: PathBuf,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub directories: Directories,
    pub vcs: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Delta {
    name: String,
    prev: Version,
    next: Version,
}

impl Delta {
    fn new(name: &str) -> Self {
        Delta {
            name: name.to_string(),
            prev: Version { semver: String::new(), git_hash: String::new() },
            next: Version { semver: String::new(), git_hash: String::new() },
        }
    }
}

#[derive(Clone, Debug)]
pub struct Version {
    pub semver: String,
    pub git_hash: String,
}

impl Version {
    pub fn to_string(&self) -> String {
        if self.git_hash.is_empty() {
            return self.semver.clone()
        }

        format!("{}@git:{}", self.semver, self.git_hash)
    }
}

fn main() -> io::Result<()> {
    let args = Args::parse();

    let cfg_path = args.config.clone().unwrap_or_else(|| "./wgpu_update.toml".into());
    let mut config_file = File::open(cfg_path)?;

    let mut buf = String::new();
    config_file.read_to_string(&mut buf)?;
    let config: Config = toml::from_str(&buf).unwrap();

    let mut deltas = [
        Delta::new("wgpu-core"),
        Delta::new("wgpu-hal"),
        Delta::new("wgpu-types"),
        Delta::new("naga"),
        Delta::new("d3d12"),
        Delta::new("ash"),
    ];

    update_wgpu(&config, &args, &mut deltas)?;

    vet_changes(&config, &args, &deltas)?;

    vendor_wgpu_update(&config, &args)?;

    if args.build {
        build(&config);
    }

    println!("\n\nAll done!");
    if !args.build {
        println!("Now is a good time to do a build in case there were breaking changes in wgpu-core's API.");
    }

    println!("It would also be a good idea to do a try run including the following tests:");
    println!(" - source-test-mozlint-updatebot");
    println!(" - source-test-vendor-rust");

    Ok(())
}

fn update_wgpu(config: &Config, args: &Args, deltas: &mut [Delta]) -> io::Result<()> {

    let gecko_path = &config.directories.mozilla_central;
    let wgpu_rev = &args.wgpu_rev;

    let mut bindings_path = gecko_path.clone();
    bindings_path.push("gfx/wgpu_bindings/");

    let mut cargo_toml_path = bindings_path.clone();
    let mut tmp_cargo_toml_path = bindings_path.clone();
    cargo_toml_path.push("Cargo.toml");
    tmp_cargo_toml_path.push("tmp.Cargo.toml");

    let wgpu_url = "https://github.com/gfx-rs/wgpu";

    println!("Parsing {cargo_toml_path:?}");
    cargo_toml::update_cargo_toml(
        io::BufReader::new(File::open(cargo_toml_path.clone())?),
        BufWriter::new(File::create(tmp_cargo_toml_path.clone())?),
        &[(wgpu_url, wgpu_rev)],
    )?;

    let mut moz_yaml_path = bindings_path.clone();
    let mut tmp_moz_yaml_path = bindings_path.clone();
    moz_yaml_path.push("moz.yaml");
    tmp_moz_yaml_path.push("tmp.moz.yaml");

    println!("Parsing {moz_yaml_path:?}");
    moz_yaml::update_moz_yaml(
        io::BufReader::new(File::open(moz_yaml_path.clone())?),
        BufWriter::new(File::create(tmp_moz_yaml_path.clone())?),
        &[(wgpu_url, wgpu_rev)],
    )?;

    println!("Applying updates");
    std::fs::rename(&tmp_cargo_toml_path, &cargo_toml_path)?;
    std::fs::rename(&tmp_moz_yaml_path, &moz_yaml_path)?;

    refresh_cargo_toml_and_update_deltas(config, args, deltas)?;

    let mut commit_msg = String::new();
    if let Some(bug) = &args.bug {
        commit_msg.push_str(&format!("Bug {bug} - "));
    }
    commit_msg.push_str(&format!("Update wgpu to revision {wgpu_rev}. r=#webgpu-reviewers"));

    commit(config, &commit_msg);

    Ok(())
}

fn refresh_cargo_toml_and_update_deltas(config: &Config, args: &Args, deltas: &mut [Delta]) -> io::Result<()> {
    let gecko_path = &config.directories.mozilla_central;

    println!("Parse previous crate versions from Cargo.lock");
    for delta in &mut deltas[..] {
        delta.prev = cargo_lock::find_version(&delta.name, config)?;
    }

    println!("Refresh Cargo.lock");
    // Run a cargo command that will cause it to pick up the new version of the crates that we
    // updated in wgpu_bindings/Cagro.toml (and their depdendencies) without trying to update
    // unrelated crates. There may be other ways but this one appears to do what we want.
    shell(gecko_path, "cargo", &["update", "--package", "wgpu-core", "--precise", &args.wgpu_rev]);

    println!("Parse new crate versions from Cargo.lock");
    // Parse Cargo.lock again to get the new version of the crates we are interested in (including
    // the new versions of things we didn´t specify but wgpu depends on).
    for delta in &mut deltas[..] {
        delta.next = cargo_lock::find_version(&delta.name, config)?;
    }

    Ok(())
}

fn vendor_wgpu_update(config: &Config, args: &Args) -> io::Result<()> {
    let gecko_path = &config.directories.mozilla_central;

    shell(gecko_path, "./mach", &["vendor", "rust"]);

    let mut commit_msg = String::new();
    if let Some(bug) = &args.bug {
        commit_msg.push_str(&format!("Bug {bug} - "));
    }
    commit_msg.push_str("Vendor wgpu changes. r=#webgpu-reviewers");
    
    commit(config, &commit_msg);

    Ok(())
}

fn vet_changes(config: &Config, args: &Args, deltas: &[Delta]) -> io::Result<()> {
    let gecko_path = &config.directories.mozilla_central;

    for delta in deltas {
        let crate_name = &delta.name;
        let prev = delta.prev.to_string();
        let next = delta.next.to_string();
        if prev == next {
            println!("{crate_name} version has not changed ({prev}).");
            continue;
        }

        shell(gecko_path, "./mach", &["cargo", "vet", "certify", crate_name, &prev, &next, "--criteria", "safe-to-deploy"]);
    }

    let mut commit_msg = String::new();
    if let Some(bug) = &args.bug {
        commit_msg.push_str(&format!("Bug {bug} - "));
    }
    commit_msg.push_str("Vet wgpu and naga commits. r=#supply-chain-reviewers");

    commit(config, &commit_msg);

    // Run cargo vet to see if there are any other new crate versions that were imported
    // besides wgpu ones (typically naga, d3d12).
    // TODO: parse the output and add them to the commit in the common cases.
    shell(gecko_path, "./mach", &["cargo", "vet"]);

    Ok(())
}

fn build(config: &Config) {
    shell(&config.directories.mozilla_central, "./mach", &["build"]);
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

fn commit(config: &Config, commit_msg: &str) {
    let mc = &config.directories.mozilla_central;
    if config.vcs == Some("git".into()) {
        shell(mc, "git", &["commit", "-am", commit_msg]);
    } else {
        shell(mc, "hg", &["commit", "-m", commit_msg]);
    }
}