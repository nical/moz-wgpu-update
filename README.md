# `moz-wgpu-update`

Scripts to automate the process of updating wgpu in mozilla-central.

# Example usage

## Setup

You will need a `.moz-wgpu.toml` file with information about where the various repositories are on disk. For example, mine looks like this:

```toml
github-api-token = "gh"

[gecko]
path = "/home/nical/dev/mozilla/mozilla-unified"
vcs = "hg"

[wgpu]
path = "/home/nical/dev/rust/wgpu"
upstream-remote = "upstream"
trusted-reviewers = ["nical", "teoxoy", "ErichDonGubler", "jimblandy"]
latest-commit = "/home/nical/dev/mozilla/moz-wgpu-update/latest-wgpu-commit.txt"

[naga]
path = "/home/nical/dev/rust/naga"
upstream-remote = "upstream"
trusted-reviewers = ["nical", "teoxoy", "ErichDonGubler", "jimblandy"]
latest-commit = "/home/nical/dev/mozilla/moz-wgpu-update/latest-naga-commit.txt"
```

`upstream-remote` is the name of the remote git will pull from (for example `upstream` in the command `git pull upstream master`) to get the latest changes. If not specified, the default is "upstream".
`main-branch` is the project's main branch. It should be `master` for `naga` and `trunk` for `wgpu`.

`github-api-token` is needed by the `audit` command. It is explained later in this document.

The script will look for the configuration file in the current folder, then in the home folder.

You can install the script like any Rust binary:

```bash
$ cargo install --path path/to/this/repository/
```

Or just run it form this repository's root folder. In this case, replace the beginning of the command `moz-wgpu ` with `cargo run -- ` in all of the examples in this document.


## Updating `wgpu` in mozilla-central

```bash
# Update the `wgpu` dependencies in mozilla-central to revision 98ea3500fd2cfb4b51d5454c662d8eefd940156a
$ moz-wgpu wgpu-update --git-hash 98ea3500fd2cfb4b51d5454c662d8eefd940156a --bug 1813547
```

or

```bash
# Similar, except that `--auto` tells script to detect the latest `wgpu` revision from your local
# checkout's trunk branch. Beware! This will pull changes in `wgpu`'s trunk branch.
$ moz-wgpu wgpu-update --auto --bug 1813547
```

Specifying the bug number is optional.

This creates 3 commits:

- Bug 1813547 - Update wgpu to revision 98ea3500fd2cfb4b51d5454c662d8eefd940156a. r=#webgpu-reviewers
- Bug 1813547 - Vendor wgpu changes. r=#webgpu-reviewers
- Bug 1813547 - Vet wgpu and naga commits. r=#supply-chain-reviewers

In practice there are often going to be fixes to make along the way, causing you to re-generate the commits multiple times.

If so, you may want to pass `--skip-preamble` on subsequent runs. The preamble commits any uncommitted changes in mozilla-central and runs `cargo vendor rust` to make sure there are no unrelated crates that will be picked up later when the script vendors the `wgpu` changes. That takes time and there is no need to run it again as long as, on the first run, the script did not produce commit messages that start with "(Don't land)".

If you have already submitted the commits to phabricator and want to re-generate them, you'll want to make sure the new commits update the corresponding phabricator revisions. It is tedious to manually edit each commit message to add the revision marker every time they are re-generated. The script can do that for you if you pass a comma separated list of the three phabricator revision ids in their order of creation using `--phab-revisions`, for example:

```bash
$ moz-wpgu wgpu-update --git-hash 98ea3500fd2cfb4b51d5454c662d8eefd940156a --bug 1813547 --skip-preamble --phab-revisions "D168302,D168303,D168304"
```

## Updating `naga` in `wgpu`

```bash
$ moz-wpgu naga-update --auto --branch "naga-up" --test
```

`--auto` will automatically detect the changes from your local `naga` checkout's master branch. Note that it will pull changes into your master branch. You can also use `--git-hash <hash>` and `--semver <major.minor.patch>` to update to a specific version.

`--branch` lets you specify the branch to write the update into. This defaults to `naga-update`. Note that the branch will be re-created each time the command is run.

# The full auditing and update process

## The `audit` command

This tool implements a script that summarizes the commits that need to be audited.

Before running the command, you must set up a GitHub API token so that the tool can access github's graphql API. Once you have the api token, you can add `github-api-token = "<token>"` in your config file. If you are using the `gh` command-line tool and the latter is authenticated, you can instead put `github-api-token = "gh"` in the config file and the tool will automatically request the token from `gh`.

Here is an example of using the script to gather information about `wgpu` commits between specific revisions and write the output into `./wgpu-commits.csv`.

```
$ moz-wgpu audit wgpu --from c371e7039dac763b08ada0a35f6c11cd71052010 --to HEAD -o ./wgpu-commits.csv
```

- `--to` defaults to `HEAD` so we don't actually need to pass it.
- `-o`/`--output` is optional. If absent, the result will be printed to stdout.
- If the the config file contains a path for the project's `latest-commit`, `--from` can omitted, and the script will use the latest commit hash written into a text file at the given path instead. The script will also update that file at the end.

So if you use this tool frequently, the command invocation will probably something like:

```bash
# To gather wgpu commits to audit:
$ moz-wgpu audit wgpu

# To gather `naga` commits to audit:
$ moz-wgpu audit naga
```

The output looks like this:

```csv
3435	1e27fd4afb6c9e203fa3bc096c000e3aa385de6d	Elabajaba	ErichDonGubler	nical	ErichDonGubler,nical
3338	2562f323bb4597da814d009459344e4133bd1d2c	AdrianEddy	cwfitzgerald	cwfitzgerald	
3401	c5e2f5a7b99f46b3d70fa6d05ff7d75de01a1235	Elabajaba	cwfitzgerald	cwfitzgerald	
3434	7826092d866ed624d906cebf6988be43882edaf3	Elabajaba	cwfitzgerald	cwfitzgerald	
3444	4ea31598a018cbd24b75bc10a2100b1e522fd613	cwfitzgerald		nical	nical
3446	e36c080ef8c117278533ea43f84c90f9bed7f882	crowlKats		teoxoy	teoxoy
3447	42b48ecb9ff6287ceef0c4203ffe672dffec4f2c	Elabajaba	cwfitzgerald	cwfitzgerald	
3451	6399dd486608986ca65303a26928d5ba210c4855	nical		teoxoy	teoxoy
3445	41de797c745d317e93b9cf50e7446faff7f65954	teoxoy		nical	nical
```

It is a csv formatted table using tabs as separator, with columns `pull request`, `commit`, `author`, `reviewers`, `merger`, `vetted by`.

This has to be appended to the `wgpu-vet` shared spreadsheet.

## Audit commits

The script printed to stdout a csv-formatted list of commits that have to be appended to the `wgpu-vet` shared spreadsheet.

The spreadheet contains a "vetted by" column, and any commit that does not have a name in there must be audited. The spreadsheet generates links to the pull requests. Now is a good time to follow the links of whatever needs auditing, do the audit and add your name in the corresponding cell of the "vetted by" column.

## Repeat the previous steps for `naga`

The audit command works the same way for `wgpu` and `naga`.

## Prep mozilla-central

Nothing surprising here, we just don't want to accidentally pick up uncommitted changes, although if you forget, the tool will detect that and put it in another commit.

```bash
$ cd /path/to/mozilla-central
$ hg diff # Just checking for uncommitted changes, commit them if need be.
$ hg pull
$ hg checkout central # if you want to apply on top of a fesh revision of central.
```

## File a bug for the update

Go to bugzilla, file a bug, write down the bug number (in our example, it's going to be `1813547`).

This tool can make that a bit easier with the following command:

```bash
$ moz-wpgu bugzilla "`wgpu` update (Early February 2023)"
```

The command above produces an url with pre-filled bugzilla entries.

Adding `--open` directly opens the url with firefox.

## Run this tool

Copy the hash that was printed to stdout at the end of the previous step with Jim's scripts (in the previous example it was `41de797c745d317e93b9cf50e7446faff7f65954`) as well as the bug number (example `1813547`) and use it as input for this tool.

```bash
$ cd path/to/this/repository
$ moz-wpgu wgpu-update --git-hash 41de797c745d317e93b9cf50e7446faff7f65954 --bug 1813547
```

The bug number if optional. If absent, it just won't be in the commit messages.

If everything went well, you have 3 new commits in mozilla central:

- Bug 1813547 - Update wgpu to revision 41de797c745d317e93b9cf50e7446faff7f65954. r=#webgpu-reviewers
- Bug 1813547 - Vendor wgpu changes. r=#webgpu-reviewers
- Bug 1813547 - Vet wgpu and naga commits. r=#supply-chain-reviewers

At the end, the tool printed a few instructions, typically the two tests to not forget to put in a try run.

```bash
$ cd /path/to/mozilla-central
$ hg wip # check that the commits are there
```

## Prune audits

The above process will add entries to `supply-chain/audits.toml` that may be
redundant. Until `cargo vet` is adjusted, they should be manually removed.

For example, suppose `audits.toml` contains the following entries for the `naga`
crate:

```
[[audits.naga]]
criteria = "safe-to-deploy"
version = "0.8.0"

[[audits.naga]]
criteria = "safe-to-deploy"
delta = "0.8.0 -> 0.9.0"

[[audits.naga]]
criteria = "safe-to-deploy"
delta = "0.9.0 -> 0.10.0"

[[audits.naga]]
criteria = "safe-to-deploy"
delta = "0.10.0 -> 0.10.0@git:e98bd9264c3a6b04dff15a6b1213c0c80201740a"

[[audits.naga]]
criteria = "safe-to-deploy"
delta = "0.10.0@git:1be8024bda3594987b417bead5024b98be9ab521 -> 0.11.0@git:f0edae8ce9e55eeef489fc53b10dc95fb79561cc"

[[audits.naga]]
criteria = "safe-to-deploy"
delta = "0.10.0@git:e98bd9264c3a6b04dff15a6b1213c0c80201740a -> 0.10.0@git:1be8024bda3594987b417bead5024b98be9ab521"
```

These entries are not all in chronological order, but if you look at the commit
hashes, you can follow the chain from the first audit of `0.8.0` to
`0.11.0@git:f0edae8c`.

However, audits of unreleased commits are unlikely to be valuable to anyone
outside of Mozilla. And because we update the version of `wgpu` and its related
crates so frequently, adding an audit entry for each import will clutter
`audits.toml` with useless information.

To avoid this, we adopt the rule that, while delta audits from one release to
another should always be retained, `audits.toml` should have at most one delta
entry from a released version of a given crate to a Git commit. This means that
the above should be reduced to:

```
[[audits.naga]]
criteria = "safe-to-deploy"
version = "0.8.0"

[[audits.naga]]
criteria = "safe-to-deploy"
delta = "0.8.0 -> 0.9.0"

[[audits.naga]]
criteria = "safe-to-deploy"
delta = "0.9.0 -> 0.10.0"

[[audits.naga]]
criteria = "safe-to-deploy"
delta = "0.10.0 -> 0.11.0@git:f0edae8ce9e55eeef489fc53b10dc95fb79561cc"
```

## Build firefox

If you didn't pass `--build` to the tool.

```bash
# The mach command forwards its parameters to mach and runs it in your gecko directoty for convenience.
$ moz-wgpu mach build
# It is equivalent to:
$ cd /path/to/mozilla-central
$ ./mach build
```

If there are build errors, it might be that `wgpu-core`'s API has changed. Fix the issue (hopefully all of the changes can be done in mozilla-central), and create a new commit.

You could fold these fixes into the commit `Bug 1813547 - Update wgpu to revision 41de797c745d317e93b9cf50e7446faff7f65954. r=#webgpu-reviewers`, only do that if you are certain you won't need to re-generate the commits.

## Submit the changes for review, wait for the review and land them

The usual patch landing process (typically takes a day to get the reviews if you ask for it in the team's matrix channel).

# Testing a branch from a wgpu fork

The script has some limited support for letting the wgpu-update commands point to a fork of the wgpu repository for testing purposes.
Here is an example showing how to get gecko to point to a specific commit from the `master` branch of `https://github.com/gents83/wgpu`, which at the time of writing contains the very anticipated wgpu arcanization work.

First clone the fork soemwhere (let's say at `/path/to/local/arcanization/`).

First make a copy of your config file.

```bash
cp ~/.moz-wgpu.toml ./arcanization.toml
```

Then make a few changes to wgpu entry in the new `arcanization.toml` configuration file:

```toml
[wgpu]
path = "/path/to/local/arcanization/"
repository = "https://github.com/gents83/wgpu"
main-branch = "master"
```

Leave all other entries of the configuration file untouched.

Then run the script, instructing it to use the new config file instead of the default one:

```bash
moz-wgpu wgpu-update --config ./arcanization.toml -g d1fe60c955111cc1dd637339a8fb88b1838fc423 --skip-preamble
```
