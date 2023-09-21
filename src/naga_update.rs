use crate::{cargo_toml, concat_path, read_config_file, read_shell, shell, Version, DEFAULT_NAGA_REPOSITORY};
use clap::Parser;
use std::{
    fs::File,
    io::{self, BufWriter},
    path::PathBuf,
};

#[derive(Parser, Debug)]
pub struct Args {
    /// The new `naga` revision (git hash) to update to.
    #[arg(short, long)]
    git_hash: Option<String>,

    /// The new `naga` version (semver) to update to.
    #[arg(short, long)]
    semver: Option<String>,

    /// Automatically determine `naga`'s version from a local checkout.
    #[arg(short, long)]
    auto: bool,

    /// The branch name in `wgpu` (`naga`-update by default).
    #[arg(short, long)]
    branch: Option<String>,

    /// Checkout and pull `wgpu`'s trunk branch before creating the update branch.
    #[arg(long)]
    on_trunk: bool,

    /// Config file to use.
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Whether to run tests at the end.
    #[arg(long)]
    test: bool,
}

pub fn update_command(args: &Args) -> io::Result<()> {
    let config = read_config_file(&args.config)?;

    let version = if args.auto {
        println!("Detecting `naga` version from local checkout.");
        Version::from_git_checkout(&config.naga, true)?
    } else {
        Version {
            semver: args.semver.clone().unwrap_or(String::new()),
            git_hash: args.git_hash.clone().unwrap_or(String::new()),
        }
    };

    println!(
        "Will update `wgpu`'s `naga` dependency to {}",
        version.display_cargo_vet()
    );

    let branch_name = args.branch.clone().unwrap_or("naga-update".to_string());

    // Commit any potential uncommitted changes before doing the update.
    // Run cargo check to make sure the lock file is up to date.
    shell(&config.wgpu.path, "cargo", &["check"])?;
    shell(
        &config.wgpu.path,
        "git",
        &["commit", "-am", "Uncommitted changes before naga update."],
    )?;

    // If we are already in the destination branch, switch to `master` so that we
    // can re-create it.
    let current_branch = read_shell(
        &config.wgpu.path,
        "git",
        &["rev-parse", "--abbrev-ref", "HEAD"],
    )
    .stdout;
    if args.on_trunk {
        shell(&config.wgpu.path, "git", &["checkout", "trunk"])?;
        shell(
            &config.wgpu.path,
            "git",
            &["pull", &config.wgpu.upstream_remote, "trunk"],
        )?;
    }
    if current_branch.trim() == branch_name {
        println!("Temporarily switching to `trunk`");
        shell(&config.wgpu.path, "git", &["checkout", "trunk"])?;
    }

    // Delete, recreate the branch and switch to it
    shell(&config.wgpu.path, "git", &["branch", "-D", &branch_name])?;
    shell(&config.wgpu.path, "git", &["checkout", "-b", &branch_name])?;

    let folders = &["", "wgpu-core/", "wgpu-hal/", "wgpu-types/"];

    // Apply changes in temporary files.
    println!("Updating `Cargo.toml` files.");
    for relative_path in folders {
        let folder = concat_path(&config.wgpu.path, relative_path);
        let cargo_toml_path = concat_path(&folder, "Cargo.toml");
        let tmp_cargo_toml_path = concat_path(&folder, "tmp.Cargo.toml");
        println!(" - {cargo_toml_path:?}");

        cargo_toml::update_cargo_toml(
            io::BufReader::new(File::open(cargo_toml_path.clone())?),
            BufWriter::new(File::create(tmp_cargo_toml_path.clone())?),
            &[("naga", &version)],
            DEFAULT_NAGA_REPOSITORY,
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

    let commit_msg = format!("Update `naga` to {}", version.display_cargo_vet());
    shell(&config.wgpu.path, "git", &["commit", "-am", &commit_msg])?;

    if args.test {
        shell(&config.wgpu.path, "cargo", &["nextest", "run"])?;
    }

    Ok(())
}
