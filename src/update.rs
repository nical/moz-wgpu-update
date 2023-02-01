use std::{path::PathBuf, fs::File, io::{self, BufWriter}};
use clap::Parser;
use crate::{Directories, Vcs, read_config_file, cargo_toml, cargo_lock, moz_yaml, shell};

// The order of the 3 gecko commits.
const COMMIT_UPADTE: Option<usize> = Some(0);
const COMMIT_AUDIT: Option<usize> = Some(1);
const COMMIT_VENDOR: Option<usize> = Some(2);

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct UpdateArgs {
    /// The new wgpu revision (git hash) to update to.
    #[arg(short, long)]
    wgpu_rev: String,

    /// The bug number.
    #[arg(short, long)]
    bug: Option<String>,

    /// Config file to use.
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Whether to run start a build at the end.
    #[arg(long)]
    build: bool,

    /// Comma separated string of the 3 phabricator revisions (to re-generated already submitted patches).
    #[arg(long)]
    phab_revisions: Option<String>,

    /// Skip the optional steps that ensure mozilla-central is in an expected state.
    #[arg(long)]
    skip_preamble: bool
}

// For convenience, merge Config and Args into a single Param
// struct with default values applied.
pub struct Parameters {
    wgpu_rev: String,
    bug: Option<String>,
    dir: Directories,
    vcs: Vcs,
    phab_revisions: Option<[String; 3]>,
    preamble: bool,
    build: bool,
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

fn get_parameters(args: &UpdateArgs) -> io::Result<Parameters> {
    let config = read_config_file(&args.config)?;

    let phab_revisions = args.phab_revisions.as_ref().map(|s| {
        let mut revs = s.split(',');
        [
            revs.next().unwrap().to_string(),
            revs.next().unwrap().to_string(),
            revs.next().unwrap().to_string(),
        ]
    });

    Ok(Parameters {
        wgpu_rev: args.wgpu_rev.clone(),
        bug: args.bug.clone(),
        dir: config.directories,
        vcs: Vcs::new(&config.vcs),
        phab_revisions,
        build: args.build,
        preamble: !args.skip_preamble,
    })
}

pub fn update_command(args: &UpdateArgs) -> io::Result<()> {
    let params = get_parameters(args)?;

    if params.preamble {
        preamble(&params)?;
    }

    let deltas = update_wgpu(&params)?;

    vet_changes(&params, &deltas)?;

    vendor_wgpu_update(&params)?;

    if params.build {
        build(&params);
    }

    println!("\n\nAll done!");
    if !params.build {
        println!("Now is a good time to do a build in case there were breaking changes in wgpu-core's API.");
    }

    println!("It would also be a good idea to do a try run including the following tests:");
    println!(" - source-test-mozlint-updatebot");
    println!(" - source-test-vendor-rust");

    Ok(())
}

/// Do a few things to make sure we start in a good state.
fn preamble(params: &Parameters) -> io::Result<()> {
    let gecko_path = &params.dir.mozilla_central;

    shell(gecko_path, "hg", &["diff"]);
    commit(params, "(Don't land) Uncommited changes before the wgpu update.", None);

    shell(gecko_path, "./mach", &["vendor", "rust"]);
    commit(params, "(Don't land) Stray unvendored 3rd parties before the wgpu update.", None);

    Ok(())
}

fn update_wgpu(params: &Parameters) -> io::Result<Vec<Delta>> {

    let gecko_path = &params.dir.mozilla_central;
    let wgpu_rev = &params.wgpu_rev;

    let mut bindings_path = gecko_path.clone();
    bindings_path.push("gfx/wgpu_bindings/");

    let mut cargo_toml_path = bindings_path.clone();
    let mut tmp_cargo_toml_path = bindings_path.clone();
    cargo_toml_path.push("Cargo.toml");
    tmp_cargo_toml_path.push("tmp.Cargo.toml");

    let wgpu_url = "https://github.com/gfx-rs/wgpu";

    let mut deltas = vec![
        Delta::new("wgpu-core"),
        Delta::new("wgpu-hal"),
        Delta::new("wgpu-types"),
        Delta::new("naga"),
        Delta::new("d3d12"),
        Delta::new("ash"),
    ];

    println!("Parsing previous crate versions from Cargo.lock");
    for delta in &mut deltas[..] {
        delta.prev = cargo_lock::find_version(&delta.name, &params.dir.mozilla_central)?;
    }

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

    println!("Refresh Cargo.lock");
    // Run a cargo command that will cause it to pick up the new version of the crates that we
    // updated in wgpu_bindings/Cagro.toml (and their depdendencies) and write them in Cargo.lock
    // without trying to update unrelated crates. There may be other ways but this one appears to
    // do what we want.
    shell(gecko_path, "cargo", &["update", "--package", "wgpu-core", "--precise", &params.wgpu_rev]);

    commit(params, &format!("Update wgpu to revision {wgpu_rev}. r=#webgpu-reviewers"), COMMIT_UPADTE);

    println!("Parsing new crate versions from Cargo.lock");
    // Parse Cargo.lock again to get the new version of the crates we are interested in (including
    // the new versions of things we didn´t specify but wgpu depends on).
    for delta in &mut deltas[..] {
        delta.next = cargo_lock::find_version(&delta.name, &params.dir.mozilla_central)?;
    }

    Ok(deltas)
}

fn vendor_wgpu_update(params: &Parameters) -> io::Result<()> {
    let gecko_path = &params.dir.mozilla_central;

    shell(gecko_path, "./mach", &["vendor", "rust"]);

    commit(params, "Vendor wgpu changes. r=#webgpu-reviewers", COMMIT_VENDOR);

    Ok(())
}

fn vet_changes(params: &Parameters, deltas: &[Delta]) -> io::Result<()> {
    let gecko_path = &params.dir.mozilla_central;

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

    commit(params, "Vet wgpu and naga commits. r=#supply-chain-reviewers", COMMIT_AUDIT);

    // Run cargo vet to see if there are any other new crate versions that were imported
    // besides wgpu ones (typically naga, d3d12).
    // TODO: parse the output and add them to the commit in the common cases.
    shell(gecko_path, "./mach", &["cargo", "vet"]);

    Ok(())
}

fn build(params: &Parameters) {
    shell(&params.dir.mozilla_central, "./mach", &["build"]);
}

fn commit(params: &Parameters, msg: &str, commit_idx: Option<usize>) {
    let mut commit_msg = String::new();
    if let Some(bug) = &params.bug {
        commit_msg.push_str(&format!("Bug {bug} - "));
    }
    commit_msg.push_str(msg);

    if let (Some(revs), Some(idx)) = (&params.phab_revisions, commit_idx) {
        commit_msg.push_str(&format!("\n\nDifferential Revision: https://phabricator.services.mozilla.com/{}", revs[idx]));
    }

    let mc = &params.dir.mozilla_central;
    match params.vcs {
        Vcs::Mercurial => { shell(mc, "hg", &["commit", "-m", &commit_msg]); }
        Vcs::Git => { shell(mc, "git", &["commit", "-am", &commit_msg]); }
    }
}
