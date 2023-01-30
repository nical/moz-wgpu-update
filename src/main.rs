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

    /// Who to ask the review to.
    #[arg(short, long)]
    reviewers: Option<String>,

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
    // TODO: a setting for using git instead of hg in mozilla-central.
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

    for delta in &mut deltas {
        delta.prev = cargo_lock::find_version(&delta.name, &config)?;
    }

    update_wgpu(&config, &args)?;

    vendor_wgpu_update(&config)?;

    // Vendoring coveniently updated the Cargo.lock file so go find the versions and hashes
    // again to figure out the delta to change.
    for delta in &mut deltas {
        delta.next = cargo_lock::find_version(&delta.name, &config)?;
    }

    vet_changes(&config, &deltas)?;

    if args.build {
        build(&config);
    }

    Ok(())
}

fn update_wgpu(config: &Config, args: &Args) -> io::Result<()> {

    let reviewers = args.reviewers
        .clone()
        .unwrap_or_else(|| "#webgpu-reviewers".into());

    let gecko_path = &config.directories.mozilla_central;
    let wgpu_rev = &args.wgpu_rev;

    let mut bindings_path = gecko_path.clone();
    bindings_path.push("gfx/wgpu_bindings/");

    let mut cargo_toml_path = bindings_path.clone();
    let mut tmp_cargo_toml_path = bindings_path.clone();
    cargo_toml_path.push("Cargo.toml");
    tmp_cargo_toml_path.push("tmp.Cargo.toml");

    let wgpu_url = "https://github.com/gfx-rs/wgpu";

    println!("Parsing {:?}", cargo_toml_path);
    cargo_toml::update_cargo_toml(
        io::BufReader::new(File::open(cargo_toml_path.clone())?),
        BufWriter::new(File::create(tmp_cargo_toml_path.clone())?),
        &[(wgpu_url, wgpu_rev)],
    )?;

    let mut moz_yaml_path = bindings_path.clone();
    let mut tmp_moz_yaml_path = bindings_path.clone();
    moz_yaml_path.push("moz.yaml");
    tmp_moz_yaml_path.push("tmp.moz.yaml");

    println!("Parsing {:?}", moz_yaml_path);
    moz_yaml::update_moz_yaml(
        io::BufReader::new(File::open(moz_yaml_path.clone())?),
        BufWriter::new(File::create(tmp_moz_yaml_path.clone())?),
        &[(wgpu_url, wgpu_rev)],
    )?;

    println!("Applying updates");
    std::fs::rename(&tmp_cargo_toml_path, &cargo_toml_path)?;
    std::fs::rename(&tmp_moz_yaml_path, &moz_yaml_path)?;

    let mut commit_msg = String::new();
    if let Some(bug) = &args.bug {
        commit_msg.push_str("Bug ");
        commit_msg.push_str(&bug);
        commit_msg.push_str(" - ");
    }
    commit_msg.push_str("Update wgpu to revision ");
    commit_msg.push_str(&wgpu_rev);
    commit_msg.push_str(". r=");
    commit_msg.push_str(&reviewers);

    println!("Committing {commit_msg:?}");

    //Command::new("hg")
    //    .arg(&"diff")
    //    .current_dir(&gecko_path)
    //    .spawn()
    //    .unwrap()
    //    .wait()
    //    .unwrap();

    Command::new("hg")
        .args(&["commit", "-m", &commit_msg])
        .current_dir(&gecko_path)
        .spawn()
        .unwrap()
        .wait()
        .unwrap();

    println!("Done.");

    Ok(())
}

fn vendor_wgpu_update(config: &Config) -> io::Result<()> {
    let gecko_path = &config.directories.mozilla_central;
    println!("Running mach vendor rust");
    Command::new("./mach")
        .args(&["vendor", "rust"])
        .current_dir(&gecko_path)
        .spawn()
        .unwrap()
        .wait()
        .unwrap();

    Command::new("hg")
        .args(&["commit", "-m", &"Vendor wgpu changes"])
        .current_dir(&gecko_path)
        .spawn()
        .unwrap()
        .wait()
        .unwrap();

    Ok(())
}

fn vet_changes(config: &Config, deltas: &[Delta]) -> io::Result<()> {
    let gecko_path = &config.directories.mozilla_central;

    for delta in deltas {
        let crate_name = &delta.name;
        let prev = delta.prev.to_string();
        let next = delta.next.to_string();
        if prev == next {
            println!("{crate_name} version has not changed ({prev}).");
            continue;
        }
        let cmd = format!("cargo vet certify {crate_name} {prev} {next} --criteria safe-to-deploy");
        let args: Vec<&str> = cmd.split(' ').collect();
        println!("Running {cmd:?}");
        Command::new("./mach")
            .args(&args)
            .current_dir(&gecko_path)
            .spawn()
            .unwrap()
            .wait()
            .unwrap();
    }

    Command::new("hg")
        .args(&["commit", "-m", &"Vet wgpu and naga commits. r=#supply-chain-reviewers"])
        .current_dir(&gecko_path)
        .spawn()
        .unwrap()
        .wait()
        .unwrap();

    // Run cargo vet to see if there are any other new crate versions that were imported
    // besides wgpu ones (typically naga, d3d12).
    // TODO: parse the output and add them to the commit in the common cases.
    Command::new("./mach")
        .args(&["cargo", "vet"])
        .current_dir(&gecko_path)
        .spawn()
        .unwrap()
        .wait()
        .unwrap();

    Ok(())
}

fn build(config: &Config) {
    Command::new("./mach")
        .args(&["build"])
        .current_dir(&config.directories.mozilla_central)
        .spawn()
        .unwrap()
        .wait()
        .unwrap();
}