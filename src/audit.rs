use std::{io::{self, Read}, path::{Path, PathBuf}, fs::File, sync::Arc};
use clap::Parser;
use octocrab::{Octocrab, models::pulls::ReviewState};
use crate::{read_shell, read_config_file};

#[derive(Parser, Debug)]
pub struct AuditArgs {
    project: String,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    from: Option<String>,
    #[arg(long)]
    to: Option<String>,
}

struct Github {
    runtime: tokio::runtime::Runtime,
    api: Arc<Octocrab>,
    org: String,
    project: String,
}

impl Github {
    pub fn new(project: &str) -> io::Result<Self> {
        Ok(Github {
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()?,
            api: octocrab::instance(),
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

pub fn pull_commits_to_audit(args: &AuditArgs) -> io::Result<()> {
    let config = read_config_file(&args.config)?;
    let project: &str = &args.project;

    let repo = match project {
        "wgpu" => config.wgpu.path.clone(),
        "naga" => config.naga.path.clone(),
        other => { panic!("Unknown project {other:?}"); }
    };

    let start_commit = args.from.clone().unwrap_or_else(|| {
        read_latest_audit(&PathBuf::from("./latest-commit.txt")).unwrap()
    });

    let end_commit = args.to.clone().unwrap_or_else(|| "HEAD".to_string());

    let github = Github::new(project)?;

    let rev_list = git_rev_list(&repo, &start_commit, &end_commit)?;

    println!("{rev_list:?}");

    for commit in &rev_list {
        println!("{commit}");
        //let commit_diff = read_shell(&repo, "gh", &["api", &format!("/repos/gfx-rs/{project}/commits/{commit}")]);
        //let pulls = read_shell(&repo, "gh", &["api", &format!("/repos/gfx-rs/{project}/commits/{commit}/pulls")]);
        let pulls = pull_requests_for_commit(&github, &commit);

        println!("pulls: {pulls:?}");
        for pull in &pulls {
            let reviewers = reviewers_for_pull_request(&github, *pull);
            println!("   reviewers: {reviewers:?}");
        }
    }

    unimplemented!();
}

fn pull_requests_for_commit(github: &Github, commit: &str) -> Vec<u64> {
    let request = github.runtime.block_on(
        github.api.repos("gfx-rs", &github.project).list_pulls(commit.to_string()).send()
    );

    let pulls = request
        .unwrap()
        .items
        .iter()
        .map(|pull| pull.id.into_inner())
        .collect();

    pulls
}

fn reviewers_for_pull_request(github: &Github, pr: u64) -> Vec<String> {
    let request = github.runtime.block_on(
        github.api.pulls(&github.org, &github.project).list_reviews(pr)
    );

    let reviews = request
        .map(|reviews|{
            reviews.items
                .iter()
                .filter(|review| review.state == Some(ReviewState::Approved))
                .map(|review| review.user.as_ref().map(|user| user.login.clone()).unwrap_or(String::new()))
                .filter(|user_name| !user_name.is_empty())
                .collect()
        }).unwrap_or_default();

    reviews
}
