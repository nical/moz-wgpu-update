use crate::{
    cargo_lock, cargo_toml, concat_path, moz_yaml, read_config_file, read_shell, shell, Vcs,
    Version, DEFAULT_WGPU_REPOSITORY,
};
use clap::Parser;
use std::{
    fs::File,
    io::{self, BufWriter},
    path::{Path, PathBuf},
    process::ExitStatus,
    str::FromStr,
};

// The order of the 3 gecko commits.
const COMMIT_UPADTE: Option<usize> = Some(0);
const COMMIT_AUDIT: Option<usize> = Some(1);
const COMMIT_VENDOR: Option<usize> = Some(2);

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// The new `wgpu` revision to update the `gecko` directory to.
    #[arg(short, long)]
    git_hash: Option<String>,

    /// The bug number.
    #[arg(short, long)]
    bug: Option<String>,

    /// Detect the latest version of `wgpu` from local checkout.
    #[arg(short, long)]
    auto: bool,

    /// Vet from the base revision to the latest commit instead of from commit to commit.
    #[arg(short, long)]
    vet_from_base_revision: bool,

    /// Config file to use.
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Whether to run start a build at the end.
    #[arg(long)]
    build: bool,

    /// Comma separated string of the 3 Phabricator revisions (to re-generate already submitted patches).
    #[arg(long)]
    phab_revisions: Option<String>,

    /// Skip the optional steps that ensure that the `gecko` directory is in an expected state.
    #[arg(long)]
    skip_preamble: bool,
}

// For convenience, merge Config and Args into a single Param
// struct with default values applied.
pub struct Parameters {
    wgpu_rev: String,
    bug: Option<String>,
    gecko_path: PathBuf,
    vcs: Vcs,
    phab_revisions: Option<[String; 3]>,
    repository: String,
    preamble: bool,
    build: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Delta {
    name: String,
    prev: Version,
    next: Version,
}

impl Delta {
    fn new(name: &str) -> Self {
        Delta {
            name: name.to_string(),
            prev: Version {
                semver: String::new(),
                git_hash: String::new(),
            },
            next: Version {
                semver: String::new(),
                git_hash: String::new(),
            },
        }
    }
}

fn get_parameters(args: &Args) -> io::Result<Parameters> {
    let config = read_config_file(&args.config)?;

    let phab_revisions = args.phab_revisions.as_ref().map(|s| {
        let mut revs = s.split(',');
        [
            revs.next().unwrap().to_string(),
            revs.next().unwrap().to_string(),
            revs.next().unwrap().to_string(),
        ]
    });

    let wgpu_rev = if args.auto {
        let wgpu = Version::from_git_checkout(&config.wgpu, true)?;

        wgpu.git_hash
    } else {
        args.git_hash
            .clone()
            .expect("Need a `wgpu` revision revision")
    };

    let repository = config
        .wgpu
        .repository
        .clone()
        .unwrap_or_else(|| DEFAULT_WGPU_REPOSITORY.into());

    Ok(Parameters {
        wgpu_rev,
        bug: args.bug.clone(),
        gecko_path: config.gecko.path.clone(),
        vcs: config
            .gecko
            .vcs
            .as_deref()
            .map(Vcs::from_str)
            .unwrap()
            .unwrap_or_default(),
        phab_revisions,
        repository,
        build: args.build,
        preamble: !args.skip_preamble,
    })
}

pub fn update_command(args: &Args) -> io::Result<()> {
    let params = get_parameters(args)?;

    if params.preamble {
        preamble(&params)?;
    }

    // TODO: Could add a --on-central argument to automatically pull and checkout central.

    let deltas = update_wgpu(&params)?;

    if args.vet_from_base_revision {
        vet_from_base_revision(&params, &deltas)?;
    } else {
        vet_delta(&params, &deltas)?;
    }

    vendor_wgpu_update(&params)?;

    if params.build {
        build(&params)?;
    }

    println!("\n\nAll done!");
    if !params.build {
        println!(
            "Now is a good time to do a `./mach build` in case there were breaking changes in \
            `wgpu-core`'s API."
        );
    }

    println!("It would also be a good idea to do `./mach try --preset webgpu`.");

    Ok(())
}

/// Do a few things to make sure we start in a good state.
fn preamble(params: &Parameters) -> io::Result<()> {
    let vcs = match params.vcs {
        Vcs::Mercurial => "hg",
        Vcs::Git => "git",
    };

    let _ = shell(&params.gecko_path, vcs, &["diff"]);
    let _ = commit(
        params,
        "(Don't land) Uncommited changes before the `wgpu` update.",
        None,
    );

    let _ = shell(&params.gecko_path, "./mach", &["vendor", "rust"]);
    let _ = commit(
        params,
        "(Don't land) Stray unvendored 3rd parties before the `wgpu` update.",
        None,
    );

    Ok(())
}

fn update_wgpu(params: &Parameters) -> io::Result<Vec<Delta>> {
    let wgpu_rev = &params.wgpu_rev;

    let mut bindings_path = params.gecko_path.clone();
    bindings_path.push("gfx/wgpu_bindings/");

    let mut cargo_toml_path = bindings_path.clone();
    let mut tmp_cargo_toml_path = bindings_path.clone();
    cargo_toml_path.push("Cargo.toml");
    tmp_cargo_toml_path.push("tmp.Cargo.toml");

    let wgpu_url = &params.repository;

    let mut deltas = vec![
        Delta::new("wgpu-core"),
        Delta::new("wgpu-hal"),
        Delta::new("wgpu-types"),
        Delta::new("naga"),
        Delta::new("ash"),
    ];

    println!("Parsing previous crate versions from `Cargo.lock`");
    for delta in &mut deltas[..] {
        delta.prev = cargo_lock::find_version(&delta.name, &params.gecko_path)?;
    }

    println!("Parsing {cargo_toml_path:?}");
    cargo_toml::update_cargo_toml(
        io::BufReader::new(File::open(cargo_toml_path.clone())?),
        BufWriter::new(File::create(tmp_cargo_toml_path.clone())?),
        &[(
            DEFAULT_WGPU_REPOSITORY,
            &Version {
                semver: String::new(),
                git_hash: wgpu_rev.to_string(),
            },
        )],
        wgpu_url,
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

    refresh_cargo_lock(&params.gecko_path, &params.wgpu_rev);

    let commit = commit(
        params,
        &format!("Update `wgpu` to revision {wgpu_rev}. r=#webgpu-reviewers"),
        COMMIT_UPADTE,
    )?;
    assert!(commit.success());

    // println!("Parsing new crate versions from `Cargo.lock`");
    // // Parse Cargo.lock again to get the new version of the crates we are interested in (including
    // // the new versions of things we didn´t specify but wgpu depends on).
    // for delta in &mut deltas[..] {
    //     delta.next = cargo_lock::find_version(&delta.name, &params.gecko_path)?;
    // }

    find_deltas(&params.gecko_path, &mut deltas);

    Ok(deltas)
}

fn find_deltas(gecko_path: &Path, deltas: &mut [Delta]) {
    let output = read_shell(gecko_path, "./mach", &["cargo", "vet"]);

    for line in output.stdout.lines() {
        if line.contains("missing [\"safe-to-deploy\"]") {
            if let Some((name, version)) = parse_crate_and_version(line) {
                for delta in deltas.iter_mut() {
                    if delta.name == name {
                        delta.next = version;
                        break;
                    }
                }
            }
        }
    }

    for delta in deltas.iter_mut() {
        if delta.next.semver.is_empty() {
            delta.next = delta.prev.clone();
        }
    }
}

// Parsing something that looks like:  {crate}:{semver}@git:{hash} missing ["safe-to-deploy"]
fn parse_crate_and_version(src: &str) -> Option<(String, Version)> {
    let mut crate_version_hash = src.split_whitespace().next()?.split("@git:");

    let crate_version = crate_version_hash.next().unwrap();
    let git_hash = crate_version_hash
        .next()
        .map(|s| s.to_string())
        .unwrap_or_default();

    let mut semver = String::new();
    let colon = crate_version.find(':').unwrap_or(crate_version.len());
    let name = crate_version[..colon].to_string();
    if colon + 1 < crate_version.len() {
        semver = crate_version[(colon + 1)..].to_string();
    }

    Some((name, Version { semver, git_hash }))
}

fn refresh_cargo_lock(gecko_path: &Path, wgpu_rev: &str) {
    println!("Refresh `Cargo.lock`");
    // Run a `cargo` command that will cause it to pick up the new version of the crates that we
    // updated in `wgpu_bindings/Cargo.toml` (and their depdendencies) and write them in
    // `Cargo.lock` without trying to update unrelated crates. There may be other ways but this one
    // appears to do what we want.
    let output = read_shell(
        gecko_path,
        "cargo",
        &["update", "--package", "wgpu-core", "--precise", wgpu_rev],
    );

    if output.stderr.contains("object not found - no match for id") {
        println!("Uh oh, `cargo` is acting up:");
        println!("{}", output.stderr);
        println!(
            "I've experienced this error intermittently.\n Working around with another command...",
        );

        let _ = read_shell(
            &concat_path(gecko_path, "gfx/wgpu_bindings/"),
            "cargo",
            &["check"],
        );

        println!("...done.")
    }
}

fn vendor_wgpu_update(params: &Parameters) -> io::Result<()> {
    let vendor = shell(&params.gecko_path, "./mach", &["vendor", "rust"])?;
    assert!(vendor.success());

    let commit = commit(
        params,
        "Vendor `wgpu` changes. r=#webgpu-reviewers",
        COMMIT_VENDOR,
    )?;
    assert!(commit.success());

    Ok(())
}

fn vet_delta(params: &Parameters, deltas: &[Delta]) -> io::Result<()> {
    for delta in deltas {
        let crate_name = &delta.name;
        let prev = delta.prev.display_cargo_vet().to_string();
        let next = delta.next.display_cargo_vet().to_string();
        if prev == next {
            println!("{crate_name} version has not changed ({prev}).");
            continue;
        }

        let vet = shell(
            &params.gecko_path,
            "./mach",
            &[
                "cargo",
                "vet",
                "certify",
                crate_name,
                &prev,
                &next,
                "--criteria",
                "safe-to-deploy",
                "--accept-all",
            ],
        )?;
        assert!(vet.success());
    }

    let commit = commit(
        params,
        "Vet `wgpu` and `naga` commits. r=#supply-chain-reviewers",
        COMMIT_AUDIT,
    )?;
    assert!(commit.success());

    let _ = shell(&params.gecko_path, "./mach", &["cargo", "vet"]);

    Ok(())
}

fn vet_from_base_revision(params: &Parameters, deltas: &[Delta]) -> io::Result<()> {
    for delta in deltas {
        let crate_name = &delta.name;
        if delta.prev == delta.next {
            println!(
                "{crate_name} version has not changed ({}).",
                delta.prev.display_cargo_vet()
            );
            continue;
        }

        let mut prev = delta.prev.semver.clone();
        if delta.prev.semver != delta.next.semver {
            let vet = shell(
                &params.gecko_path,
                "./mach",
                &[
                    "cargo",
                    "vet",
                    "certify",
                    crate_name,
                    &delta.prev.semver,
                    &delta.next.semver,
                    "--criteria",
                    "safe-to-deploy",
                    "--accept-all",
                ],
            )?;
            assert!(vet.success());
            prev = delta.next.semver.clone();
        }

        let next = delta.next.display_cargo_vet().to_string();
        let vet = shell(
            &params.gecko_path,
            "./mach",
            &[
                "cargo",
                "vet",
                "certify",
                crate_name,
                &prev,
                &next,
                "--criteria",
                "safe-to-deploy",
                "--accept-all",
            ],
        )?;
        assert!(vet.success());
    }

    let commit = commit(
        params,
        "Vet `wgpu` and `naga` commits. r=#supply-chain-reviewers",
        COMMIT_AUDIT,
    )?;
    assert!(commit.success());

    // Run cargo vet to see if there are any other new crate versions that were imported
    // besides wgpu ones (typically naga).
    // TODO: parse the output and add them to the commit in the common cases.
    let _ = shell(&params.gecko_path, "./mach", &["cargo", "vet"]);

    Ok(())
}

fn build(params: &Parameters) -> io::Result<ExitStatus> {
    shell(&params.gecko_path, "./mach", &["build"])
}

fn commit(params: &Parameters, msg: &str, commit_idx: Option<usize>) -> io::Result<ExitStatus> {
    let mut commit_msg = String::new();
    if let Some(bug) = &params.bug {
        commit_msg.push_str(&format!("Bug {bug} - "));
    }
    commit_msg.push_str(msg);

    if let (Some(revs), Some(idx)) = (&params.phab_revisions, commit_idx) {
        commit_msg.push_str(&format!(
            "\n\nDifferential Revision: https://phabricator.services.mozilla.com/{}",
            revs[idx]
        ));
    }

    let mc = &params.gecko_path;
    match params.vcs {
        Vcs::Mercurial => shell(mc, "hg", &["commit", "-m", &commit_msg]),
        Vcs::Git => shell(mc, "git", &["commit", "-am", &commit_msg]),
    }
}
