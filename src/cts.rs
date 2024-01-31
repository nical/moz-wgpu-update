use clap::Parser;
use std::env::current_dir;
use std::io;
use std::path::PathBuf;
use std::str::FromStr;

use crate::Config;

use crate::read_config_file;
use crate::shell;
use crate::Vcs;

#[derive(Parser, Debug)]
pub struct Args {
    #[clap(subcommand)]
    sub_command: SubCommand,

    /// Config file to use.
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,
}

#[derive(Parser, Debug)]
pub enum SubCommand {
    /// Fetch the provided try revision into a temporary directory.
    Fetch {
        /// The try revision to fetch results from.
        rev: String,
    },
    /// Delete the temporary directory.
    Cleanup,
    /// Process test results in the temporary directory and update test
    /// expectations in gecko.
    Expectations {
        /// Try revision to pull results from.
        ///
        /// If not specified, read from the temporary directory
        #[arg(long)]
        fetch: Option<String>,

        /// Remove the data after running the command
        #[arg(long)]
        cleanup: bool,
    },
    /// Run the CTS locally
    Run {
        /// The test query
        ///
        /// For example "webgpu:api,validation,buffer,destroy:*"
        query: String,
    }
}

fn temp_cts_result_dir(config: &Config) -> PathBuf {
    let mut path = config.gecko.path.parent().unwrap().to_owned();
    path.push("tmp-cts");

    path
}

fn fetch_cts_results_from_try(config: &Config, rev: &str) -> io::Result<()> {
    let path = temp_cts_result_dir(config);
    let gecko_dir_name = config.gecko.path.iter().last().unwrap().to_str().unwrap();

    println!(
        " -- gecko dir: {:?}, parent {:?}",
        config.gecko.path,
        config.gecko.path.parent()
    );
    println!(" -- creating temporary directory at {path:?}");
    std::fs::create_dir_all(&path)?;
    shell(
        &path,
        &format!("../{gecko_dir_name}/mach"),
        &["wpt-fetch-logs", &format!("try:{rev}")],
    )?;

    Ok(())
}

fn update_test_expectations(config: &Config) -> io::Result<()> {
    let path = temp_cts_result_dir(config);
    let mut json_files = Vec::new();

    // This is ridiculously convoluted to do something that probably has
    // a much simpler solution: resolve the path to each jsob file into an
    // array of &str so that we cann invoke the command without the glob "*.json".
    for file in std::fs::read_dir(&path)? {
        let file = file.unwrap();
        if !file.file_type().unwrap().is_file() {
            continue;
        }
        let name = file.file_name().to_str().unwrap().to_string();
        if name.ends_with(".json") {
            let mut file_path = path.clone();
            file_path.push(&name);
            json_files.push(file_path.to_str().unwrap().to_string());
        }
    }

    let mut args: Vec<&str> = vec!["process-reports", "--preset=reset-contradictory"];

    for file in &json_files {
        args.push(&file);
    }

    if !shell(&config.gecko.path, "moz-webgpu-cts", &args)?.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Processing the cts test results failed",
        ));
    }

    Ok(())
}

fn commit(config: &Config, commit_msg: &str) -> io::Result<()> {
    let mc = &config.gecko.path;
    let vcs = config
        .gecko
        .vcs
        .as_deref()
        .map(Vcs::from_str)
        .unwrap()
        .unwrap_or_default();

    match vcs {
        Vcs::Mercurial => shell(mc, "hg", &["commit", "-m", commit_msg]),
        Vcs::Git => shell(mc, "git", &["commit", "-am", commit_msg]),
    }?;

    Ok(())
}

fn cleanup_command(config: &Config) -> io::Result<()> {
    let path = temp_cts_result_dir(&config);
    shell(
        &current_dir().unwrap(),
        "rm",
        &["-rf", path.to_str().unwrap()],
    )?;

    Ok(())
}

fn run_command(config: &Config, query: &str) -> io::Result<()> {
    let mc = &config.gecko.path;

    let mut test_cmd = "_mozilla/webgpu/cts/webgpu/".to_string();

    let query = query.strip_prefix("q=").unwrap_or(query);
    let query = query.strip_prefix("webgpu:").unwrap_or(query);
    let path = query.split(":").next().unwrap_or(query);
    for item in path.split(",") {
        test_cmd.push_str(item);
        test_cmd.push_str("/");
    }
    test_cmd.push_str("cts.https.html?q=webgpu:");
    test_cmd.push_str(query);

    shell(
        mc,
        "./mach",
        &["wpt", &test_cmd]
    )?;

    Ok(())
}

pub fn command(args: &Args) -> io::Result<()> {
    let config = read_config_file(&args.config)?;

    match &args.sub_command {
        SubCommand::Cleanup => cleanup_command(&config),
        SubCommand::Fetch { rev } => fetch_cts_results_from_try(&config, &rev),
        SubCommand::Expectations { fetch, cleanup } => {
            if let Some(rev) = fetch {
                fetch_cts_results_from_try(&config, &rev)?;
            }

            commit(
                &config,
                "(Don't land) uncommitted changes before running the command",
            )?;

            update_test_expectations(&config)?;

            commit(&config, "Update WebGPU CTS test expectations")?;

            if *cleanup {
                cleanup_command(&config)?;
            }

            Ok(())
        }
        SubCommand::Run { query } => {
            run_command(&config, &query)
        }
    }
}
