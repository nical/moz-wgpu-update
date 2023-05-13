use crate::{read_config_file, read_shell, shell};
use clap::Parser;
use core::panic;
use octocrab::{
    models::{
        pulls::{PullRequest, ReviewState},
        IssueState,
    },
    Octocrab,
};
use std::{
    fs::File,
    io::{self, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Parser, Debug)]
pub struct AuditArgs {
    project: String,
    #[arg(long)]
    config: Option<PathBuf>,
    /// Start of the commit range.
    ///
    /// If not specified, this script will look into ./latest-commit.txt
    #[arg(long)]
    from: Option<String>,
    /// End of the commit range (defaults to HEAD).
    #[arg(long)]
    to: Option<String>,
    /// Optionally write the resulting csv into a file (defaults to stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Whether to pull changes and checkout the main branch.
    #[arg(long)]
    pull: bool,
}

struct Github {
    runtime: tokio::runtime::Runtime,
    api: Arc<Octocrab>,
    org: String,
    project: String,
}

impl Github {
    pub fn new(project: &str, api_token: Option<String>) -> io::Result<Self> {
        let mut api = octocrab::OctocrabBuilder::new();

        if let Some(api_token) = api_token {
            // The config file contains either the token token itself or the string "gh" which signifies
            // use the gh command-line app to get the token.
            let token = match &api_token[..] {
                "gh" => read_shell(&PathBuf::from("."), "gh", &["auth", "token"])
                    .stdout
                    .trim()
                    .to_string(),
                token => token.to_string(),
            };

            api = api.personal_token(token);
        }

        Ok(Github {
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()?,
            api: Arc::new(api.build().unwrap()),
            org: "gfx-rs".to_string(),
            project: project.to_string(),
        })
    }
}

fn git_rev_list(path: &Path, from: &str, to: &str) -> io::Result<Vec<String>> {
    if from == to {
        return Ok(Vec::new());
    }

    let text = read_shell(path, "git", &["rev-list", &format!("{from}..{to}")]).stdout;

    let mut result = Vec::new();
    for line in text.split('\n') {
        if !line.is_empty() {
            result.push(line.to_string());
        }
    }

    Ok(result)
}

pub fn read_latest_audit(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut result = String::new();

    file.read_to_string(&mut result)?;

    Ok(result.trim().to_string())
}

struct Commit {
    pull_request: Option<u64>,
    hash: String,
    author: String,
    reviewers: Vec<String>,
    merger: Option<String>,
    vetted_by: Vec<String>,
}

pub fn find_commits_to_audit(args: &AuditArgs) -> io::Result<()> {
    let config = read_config_file(&args.config)?;

    let project = match args.project.as_str() {
        "wgpu" => &config.wgpu,
        "naga" => &config.naga,
        other => {
            panic!("Unknown project {other:?}");
        }
    };

    let latest_commit_path = project
        .latest_commit
        .clone()
        .unwrap_or_else(|| PathBuf::from("./latest-commit.txt"));

    let start_commit = args
        .from
        .clone()
        .unwrap_or_else(|| read_latest_audit(&latest_commit_path).unwrap());

    let end_commit = args.to.clone().unwrap_or_else(|| "HEAD".to_string());

    let github = Github::new(&args.project, config.github_api_token.clone())?;

    if args.pull {
        let upstream = &project.upstream_remote;
        shell(
            &project.path,
            "git",
            &[
                "commit",
                "-am",
                "Uncommitted changes before running `moz-wgpu audit`",
            ],
        )?;
        shell(&project.path, "git", &["checkout", &project.main_branch])?;
        shell(
            &project.path,
            "git",
            &["pull", upstream, &project.main_branch],
        )?;
    }

    let rev_list = git_rev_list(&project.path, &start_commit, &end_commit)?;

    if rev_list.is_empty() {
        println!("No new commits since {start_commit}, nothing to do.");
        return Ok(());
    }

    let mut commits = Vec::new();
    let mut found_at_least_one_pr = false;

    for commit_hash in rev_list.iter().rev() {
        println!("{commit_hash}");

        let pulls = pull_requests_for_commit(&github, commit_hash);

        if pulls.is_empty() {
            println!("Found no pull request for this commit");
            // This is less common but it can happen that commits are made without pull a request.
            commits.push(Commit {
                pull_request: None,
                author: String::new(),
                reviewers: Vec::new(),
                merger: None,
                hash: commit_hash.clone(),
                vetted_by: Vec::new(),
            });
        }

        for pull in pulls {
            found_at_least_one_pr = true;
            let author = pull.user.clone().map(|user| user.login).unwrap_or_default();

            println!(
                "Pull #{} by {author} - {:?}",
                pull.number,
                pull.title.clone().unwrap_or_default()
            );

            let mut commit = Commit {
                pull_request: Some(pull.number),
                author,
                hash: commit_hash.clone(),
                reviewers: reviewers_for_pull_request(&github, pull.number),
                merger: merger_for_pull_request(&github, pull.number),
                vetted_by: Vec::new(),
            };

            fn maybe_add_vetter(vetted_by: &mut Vec<String>, trusted: &[String], name: &String) {
                if trusted.contains(name) && !vetted_by.contains(name) {
                    vetted_by.push(name.to_string());
                }
            }

            maybe_add_vetter(
                &mut commit.vetted_by,
                &project.trusted_reviewers,
                &commit.author,
            );
            for reviewer in &commit.reviewers {
                maybe_add_vetter(&mut commit.vetted_by, &project.trusted_reviewers, reviewer);
            }
            if let Some(merger) = &commit.merger {
                maybe_add_vetter(&mut commit.vetted_by, &project.trusted_reviewers, merger);
            }

            commits.push(commit);
        }
    }

    if !found_at_least_one_pr {
        println!();
        println!("Now that's odd. We found commits locally via git rev-list but we couldn't get pull requests from the web API.");
        println!("This could mean:");
        println!(" - That commits have been merged without pull requests.");
        println!(" - Or your github authentication token has expired.");
    }

    write_csv_output(&commits, &args.output)?;

    if let Some(commit) = rev_list.first() {
        if let Some(path) = &project.latest_commit {
            println!("\nSaving latest commit {commit:?} to {path:?}");
            write!(io::BufWriter::new(File::create(path)?), "{commit}")?;
        }
    }

    Ok(())
}

fn pull_requests_for_commit(github: &Github, commit: &str) -> Vec<PullRequest> {
    let request = github.runtime.block_on(
        github
            .api
            .repos("gfx-rs", &github.project)
            .list_pulls(commit.to_string())
            .send(),
    );

    request
        .map(|pulls| {
            pulls
                .items
                .into_iter()
                .filter(|pull| pull.state == Some(IssueState::Closed))
                .collect()
        })
        .unwrap_or_default()
}

fn reviewers_for_pull_request(github: &Github, pr: u64) -> Vec<String> {
    let request = github.runtime.block_on(
        github
            .api
            .pulls(&github.org, &github.project)
            .list_reviews(pr),
    );

    let mut reviewers = Vec::new();
    if let Ok(reviews) = request {
        for reviewer in reviews
            .items
            .iter()
            .filter(|review| review.state == Some(ReviewState::Approved))
            .map(|review| {
                review
                    .user
                    .as_ref()
                    .map(|user| user.login.clone())
                    .unwrap_or(String::new())
            })
            .filter(|user_name| !user_name.is_empty())
        {
            reviewers.push(reviewer)
        }
    }

    reviewers
}

fn merger_for_pull_request(github: &Github, pr_idx: u64) -> Option<String> {
    let project = &github.project;
    let org = &github.org;

    // Could not find how to get a PR's merger from octocrab, so we'query it directly
    // via graphql.
    let query = format!(
        "
        query {{
            repository(owner:{org:?}, name:{project:?}) {{
                pullRequest(number:{pr_idx}) {{ mergedBy {{ login }} }}
            }}
        }}"
    );

    // Remove the whitespaces so it's a bit nicer to read in the terminal.
    let query: String = query.chars().filter(|c| *c != ' ' && *c != '\n').collect();
    //println!("graphql query: \"{query}\"");

    let response: serde_json::Value = github.runtime.block_on(github.api.graphql(&query)).unwrap();

    let merger = response
        .get("data")?
        .get("repository")?
        .get("pullRequest")?
        .get("mergedBy")?
        .get("login")?
        .to_string()
        .trim_matches('"')
        .to_string();

    Some(merger)
}

fn comma_separated_string(items: &[String]) -> String {
    let mut result = String::new();
    for (idx, item) in items.iter().enumerate() {
        assert!(!item.is_empty());
        result.push_str(item);
        if idx + 1 != items.len() {
            result.push(',');
        }
    }

    result
}

fn write_csv_output(items: &[Commit], output: &Option<PathBuf>) -> io::Result<()> {
    let mut stdout = std::io::stdout();
    let mut output_file = output
        .as_ref()
        .map(|path| BufWriter::new(File::create(path).unwrap()));

    let writer = if let Some(file) = &mut output_file {
        file as &mut dyn Write
    } else {
        println!("\n\n\n");
        &mut stdout as &mut dyn Write
    };

    for item in items {
        let pull_request = item
            .pull_request
            .map(|num| format!("{num}"))
            .unwrap_or_default();
        writeln!(
            writer,
            "{}\t{}\t{}\t{}\t{}\t{}",
            pull_request,
            item.hash,
            item.author,
            comma_separated_string(&item.reviewers),
            item.merger.clone().unwrap_or_default(),
            comma_separated_string(&item.vetted_by),
        )?;
    }

    Ok(())
}
