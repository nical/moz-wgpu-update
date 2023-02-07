use std::{io::{self, Read, BufWriter, Write}, path::{Path, PathBuf}, fs::File, sync::Arc};
use clap::Parser;
use octocrab::{Octocrab, models::{pulls::{ReviewState, PullRequest}, IssueState}};
use crate::{read_shell, read_config_file};

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
    output: Option<PathBuf>
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
        if let Some(token) = api_token {
            api = api.personal_token(token);
        }

        Ok(Github {
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()?,
            api: Arc::new(api.build().unwrap()),
            org: "gfx-rs".to_string(),
            project: project.to_string()
        })
    }
}

fn git_rev_list(path: &Path, from: &str, to: &str) -> io::Result<Vec<String>> {
    if from == to {
        return Ok(Vec::new());
    }

    // git rev-list $LAST_COMMIT..HEAD
    let text = read_shell(path, "git", &["rev-list", &format!("{from}..{to}")]);

    let mut result = Vec::new();
    for line in text.split('\n') {
        if !line.is_empty() {
            result.push(line.to_string());
        }
    }

    Ok(result)
}

pub fn read_latest_audit(path: &Path) -> io::Result<String> {
    let mut  file = File::open(path)?;
    let mut result = String::new();

    file.read_to_string(&mut result)?;
    
    Ok(result.trim().to_string())
}

struct Commit {
    index: u64,
    hash: String,
    author: String,
    reviewers: Vec<String>,
    merger: String,
    vetted_by: Vec<String>,
    //merge_date: Option<DateTime<Utc>>
}

pub fn pull_commits_to_audit(args: &AuditArgs) -> io::Result<()> {
    let config = read_config_file(&args.config)?;
    let project: &str = &args.project;

    let (repo, trusted_reviewers) = match project {
        "wgpu" => (config.wgpu.path.clone(), &config.wgpu.trusted_reviewers),
        "naga" => (config.naga.path.clone(), &config.naga.trusted_reviewers),
        other => { panic!("Unknown project {other:?}"); }
    };

    let start_commit = args.from.clone().unwrap_or_else(|| {
        read_latest_audit(&PathBuf::from("./latest-commit.txt")).unwrap()
    });

    let end_commit = args.to.clone().unwrap_or_else(|| "HEAD".to_string());

    let github = Github::new(project, config.github_api_token.clone())?;

    let rev_list = git_rev_list(&repo, &start_commit, &end_commit)?;

    let mut items = Vec::new();

    for commit in &rev_list {
        println!("{commit}");
        //let commit_diff = read_shell(&repo, "gh", &["api", &format!("/repos/gfx-rs/{project}/commits/{commit}")]);
        //let pulls = read_shell(&repo, "gh", &["api", &format!("/repos/gfx-rs/{project}/commits/{commit}/pulls")]);

        for pull in pull_requests_for_commit(&github, &commit) {
            let author = pull.user.clone().map(|user| user.login).unwrap_or_default();
            let index = pull.number;

            let mut item = Commit {
                index,
                author,
                hash: commit.clone(),
                reviewers: reviewers_for_pull_request(&github, index),
                merger: String::new(), // TODO: main missing piece.
                vetted_by: Vec::new(),
            };

            for reviewer in &item.reviewers {
                if trusted_reviewers.contains(&reviewer) {
                    item.vetted_by.push(reviewer.clone());
                }
            }

            items.push(item);
        }
    }

    // TODO: sort items by merge date?

    write_csv_output(&items, &args.output)?;

    // TODO: write into latest-commit.txt

    Ok(())
}

fn pull_requests_for_commit(github: &Github, commit: &str) -> Vec<PullRequest> {
    let request = github.runtime.block_on(
        github.api.repos("gfx-rs", &github.project).list_pulls(commit.to_string()).send()
    );

    let pulls = request.map(|pulls| {
        pulls.items
            .into_iter()
            .filter(|pull| pull.state == Some(IssueState::Closed))
            .collect()
    }).unwrap_or_default();

    pulls
}

fn reviewers_for_pull_request(github: &Github, pr: u64) -> Vec<String> {
    let request = github.runtime.block_on(
        github.api.pulls(&github.org, &github.project).list_reviews(pr)
    );

    let mut reviewers = Vec::new();
    if let Ok(reviews) = request {
        for reviewer in reviews.items.iter()
            .filter(|review| review.state == Some(ReviewState::Approved))
            .map(|review| review.user.as_ref().map(|user| user.login.clone()).unwrap_or(String::new()))
            .filter(|user_name| !user_name.is_empty()) {

            reviewers.push(reviewer)
        }
    }

    reviewers
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
    let mut output_file = output.as_ref().map(|path| BufWriter::new(File::create(path).unwrap()));

    let writer = if let Some(file) = &mut output_file {
        file as &mut dyn Write
    } else {
        println!("\n\n\n");
        &mut stdout as &mut dyn Write
    };

    for item in items {
        writeln!(writer, "{}\t{}\t{}\t{}\t{}\t{}",
            item.index,
            item.hash,
            item.author,
            comma_separated_string(&item.reviewers),
            item.merger,
            comma_separated_string(&item.vetted_by),
        )?;
    }

    Ok(())
}
