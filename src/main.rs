mod cargo_toml;
mod moz_yaml;

use std::{path::PathBuf, fs::File, io::{self, Read, BufWriter}};
use std::process::Command;

use clap::Parser;
use serde_derive::{Serialize, Deserialize};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// The wgpu revision to update to.
    #[arg(short, long)]
    wgpu_rev: String,

    /// The bug number.
    #[arg(short, long)]
    bug: Option<String>,

    /// Who to ask the review to.
    #[arg(short, long)]
    reviewers: Option<String>,

    /// Config file to use.
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,
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
}

fn main() -> io::Result<()> {
    let args = Args::parse();

    let cfg_path = args.config.unwrap_or_else(|| "./wgpu_update.toml".into());
    let mut config_file = File::open(cfg_path)?;

    let mut buf = String::new();
    config_file.read_to_string(&mut buf)?;
    let config: Config = toml::from_str(&buf).unwrap();

    let reviewers = args.reviewers.unwrap_or_else(|| "#webgpu-reviewers".into());

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
    if let Some(bug) = args.bug {
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
