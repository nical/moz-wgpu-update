# moz-wgpu-update

Scripts to automate the process of updating wgpu in mozilla-central.

# Example usage

Create a `wgpu_update.toml` file with information about where the various repositories are on disk, for example:

```toml
[directories]
wgpu = "/home/nical/dev/rust/wgpu"
naga = "/home/nical/dev/rust/naga"
mozilla_central = "/home/nical/dev/mozilla/mozilla-unified"
```

Then run the script, for example:

```bash
cargo run -- update --wgpu-rev 98ea3500fd2cfb4b51d5454c662d8eefd940156a --bug 1813547
```

`cargo vet` will prompt you to acknowledge that you have properly vetted the changes for each new crate version along the way.

This creates 3 commits:

- Bug 1813547 - Update wgpu to revision 98ea3500fd2cfb4b51d5454c662d8eefd940156a. r=#webgpu-reviewers
- Bug 1813547 - Vendor wgpu changes. r=#webgpu-reviewers
- Bug 1813547 - Vet wgpu and naga commits. r=#supply-chain-reviewers

In practice there is often going to be fixes to make along the way, causing you to re-generate the commits multiple time.

If so, you may want to pass `--skip-pramble` on subsequent runs. The preamble commits any uncommitted changes in mozilla-central runs `cargo vendor rust` to make sure there is no unrelated crates that will be picked up later when the script vendors the wgpu changes. That takes time and there is no need to run it again on first run the script did not produce commit messages that start with "(Dont' land)".

If you have already submitted the commits to phabricator and want to re-generate them, you'll want to make sure the new commits update the corresponding phabricator revisions. It is tedious to manually edit each commit message to add the revision marker every time they are re-generated. The script can do that for you if you pass a comma separated list of the three phabricator revision ids in their order of creation using `--phab_revisions`, for example:

```bash
cargo run -- update --wgpu-rev 98ea3500fd2cfb4b51d5454c662d8eefd940156a --bug 1813547 --skip-preamble --phab-revisions "D168302,D168303,D168304"
```

# The Full auditting and update process

## Run Jim's scripts

https://github.com/jimblandy/vet-wgpu

To remove some manual steps I call them from two alsmot identical shell scripts placed in `wgpu` and `naga` folders in a clone of Jim's vet-wgpu repository

The script for wgpu is:

```bash
cat ./repo.sh
echo "--"
export LAST_COMMIT=$(cat ./last-commit) &&
echo "Starting at commit $LAST_COMMIT" &&
cd /home/nical/dev/rust/wgpu &&
git checkout master &&
git pull upstream master &&
git rev-list $LAST_COMMIT..HEAD > /home/nical/dev/mozilla/vet-wgpu/wgpu/commit-list &&
cat /home/nical/dev/mozilla/vet-wgpu/wgpu/commit-list &&
echo "--" &&
cd - &&
echo "Running fetch-commits.sh..." &&
sh ../fetch-commits.sh &&
echo "Running make-commit-pulls.sh..." &&
sh ../make-commit-pulls.sh &&
echo "Running fetch-pulls.sh..." &&
sh ../fetch-pulls.sh &&
echo "Running mergers-and-approvers.sh..." &&
sh ../mergers-and-approvers.sh &&
echo "--" &&
cat mergers-and-approvers.tsv &&
# the last commit appears first in commit-list
head -n 1 ./commit-list > last-commit &&
echo "Last commit is now $(cat ./last-commit)"
```

I'm hoping to re-write all of it in some rust code that I'll have an easier time understanding and that could be used by anyone.

The steps look like this:

```bash
$ cd path/to/vet-wgpu/wgpu
$ ./run.sh
<lots of stuff in stdout>
<A csv-formatted list of PR, commits, reviewers, etc.>
Last commit is now 98ea3500fd2cfb4b51d5454c662d8eefd940156a
```

## Audit commits

The script printed to stdout a csv-formatted list of commits that have to be appended to the wgpu-vet shared spreadsheet.

The spreadheet contains a "vetted by" column, any commit that does not have a name in there must be auditted. The spreadsheet generates links to the pull requests, now is a good time to follow the links of whatever needs auditting, do the audit and add your name in the corresponding cells of the "vetted by" column.

## Repeat the previous steps for naga

Run jim's scripts with naga instead of wgpu and update the second tab of the vet-wgpu spreadhseet.

## Prep mozilla-central

Nothing surprising here, we just don't want to accidentally pick up uncommitted changes, although if you forget, the tool will detect that and put it in anoter commit.

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
cargo run -- bugzilla "wgpu update (Early February 2023)"
```

The command above produces an url with pre-filled bugzilla entries.

Adding `--open` directly opens the url with firefox.

## Run this tool

Copy the hash that was printed to stdout at the end of the previous step with jim's scripts (in the previous example it was `98ea3500fd2cfb4b51d5454c662d8eefd940156a`) as well as the bug number (example `1813547`) and use it as input for this tool.

```bash
$ cd path/to/this/repository
$ cargo run -- update --wgpu-rev 98ea3500fd2cfb4b51d5454c662d8eefd940156a --bug 1813547
```

The bug number if optional. If absent, it just won't be in the commit messages.

If everything went well, you have 3 new commits in mozilla central:

- Bug 1813547 - Update wgpu to revision 98ea3500fd2cfb4b51d5454c662d8eefd940156a. r=#webgpu-reviewers
- Bug 1813547 - Vendor wgpu changes. r=#webgpu-reviewers
- Bug 1813547 - Vet wgpu and naga commits. r=#supply-chain-reviewers

At the end, the tool printed a few instructions, typically the two tests to not forget to put in a try run.

```bash
$ cd /path/to/mozilla-central
$ hg wip # check that the commits are there
```

## Build firefox

If you didn't pass `--build` to the tool.

```bash
$ cd /path/to/mozilla-central
$ ./mach build
```

If there are build errors, it might be that `wgpu-core`'s API has changed. Fix the issue (hopefully all of the changes can be done in mozilla-central), and create a new commit.

You could fold these fixes into the commit `Bug 1813547 - Update wgpu to revision 98ea3500fd2cfb4b51d5454c662d8eefd940156a. r=#webgpu-reviewers`, only do that if you are certain you won't need to re-generate the commits.

## Submit the changes for review, wait for the review and land them

The usual patch landing process (typically takes a day to get the reviews if you ask for it in the team's matrix channel).
