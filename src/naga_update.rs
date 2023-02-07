use std::{path::PathBuf, fs::File, io::{self, BufWriter}};
use clap::Parser;
use crate::{read_config_file, cargo_toml, shell, read_shell, concat_path, Version, crate_version_from_checkout};

#[derive(Parser, Debug)]
pub struct Args {
    /// The new naga revision (git hash) to update to.
    #[arg(short, long)]
    git_hash: Option<String>,

    /// The new naga version (semver) to update to.
    #[arg(short, long)]
    semver: Option<String>,

    /// Automatically determine naga's version from a local checkout.
    #[arg(short, long)]
    auto: bool,

    /// The branch name in wgpu (naga-update by default).
    #[arg(short, long)]
    branch: Option<String>,

    #[arg(long)]
    on_master: bool,

    /// Config file to use.
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Whether to run tests at the end.
    #[arg(long)]
    test: bool,
}

pub fn update_command(args: &Args) -> io::Result<()> {
    let config = read_config_file(&args.config)?;

    let wgpu_upstream = config.wgpu.updatream_remote.unwrap_or_else(|| "upstream".to_string());
    let naga_upstream = config.naga.updatream_remote.unwrap_or_else(|| "upstream".to_string());

    let version = if args.auto {
        println!("Detecting naga version from local checkout.");
        crate_version_from_checkout(&config.naga.path, &naga_upstream, true)?
    } else {
        Version {
            semver: args.semver.clone().unwrap_or(String::new()),
            git_hash: args.git_hash.clone().unwrap_or(String::new()),
        }
    };

    println!("Will update wgpu's naga dependency to {}", version.to_string());

    let branch_name = args.branch.clone().unwrap_or("naga-update".to_string());

    // Commit any potential uncommitted changes before doing the update.
    // Run cargo check to make sure the lock file is up to date.
    shell(&config.wgpu.path, "cargo", &["check"])?;
    shell(&config.wgpu.path, "git", &["commit", "-am", "Uncommitted changes before naga update."])?;

    // If we are already in the destination branch, switch to master so that we
    // can re-create it.
    let current_branch = read_shell(&config.wgpu.path, "git", &["rev-parse", "--abbrev-ref", "HEAD"]);
    if args.on_master {
        shell(&config.wgpu.path, "git", &["checkout", "master"])?;
        shell(&config.wgpu.path, "git", &["pull", &wgpu_upstream, "master"])?;
    }
    if current_branch.trim() == branch_name {
        println!("Temporarily swicthing to master");
        shell(&config.wgpu.path, "git", &["checkout", "master"])?;
    }

    // Delete, recreate the branch and switch to it
    shell(&config.wgpu.path, "git", &["branch", "-D", &branch_name])?;
    shell(&config.wgpu.path, "git", &["checkout", "-b", &branch_name])?;

    let folders = &["", "wgpu-core/", "wgpu-hal/", "wgpu-types/"];

    // Apply changes in temporary files.
    println!("Updating Cargo.toml files.");
    for relative_path in folders {
        let folder = concat_path(&config.wgpu.path, relative_path);
        let cargo_toml_path = concat_path(&folder, "Cargo.toml");
        let tmp_cargo_toml_path = concat_path(&folder, "tmp.Cargo.toml");
        println!(" - {cargo_toml_path:?}");

        cargo_toml::update_cargo_toml(
            io::BufReader::new(File::open(cargo_toml_path.clone())?),
            BufWriter::new(File::create(tmp_cargo_toml_path.clone())?),
            &[("naga", &version)],
        )?;
    }

    println!("Applying changes.");
    for relative_path in folders {
        let folder = concat_path(&config.wgpu.path, relative_path);
        let cargo_toml_path = concat_path(&folder, "Cargo.toml");
        let tmp_cargo_toml_path = concat_path(&folder, "tmp.Cargo.toml");

        std::fs::rename(&tmp_cargo_toml_path, &cargo_toml_path)?;
    }

    shell(&config.wgpu.path, "cargo", &["check"])?;

    shell(&config.wgpu.path, "git", &["diff"])?;

    let commit_msg = format!("Update naga to {}", version.to_string());
    shell(&config.wgpu.path, "git", &["commit", "-am", &commit_msg])?;

    if args.test {
        shell(&config.wgpu.path, "cargo", &["nextest", "run"])?;
    }

    Ok(())
}
