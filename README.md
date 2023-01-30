# moz-wgpu-update
Scripts to automate the process of updating wgpu in mozilla-central

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
cargo run -- --wgpu-rev 98ea3500fd2cfb4b51d5454c662d8eefd940156a --bug 1813547
```

`cargo vet` will prompt you to acknowledge that you have properly vetted the changes for each new crate version along the way.

This creates 3 commits:

- Bug 1813547 - Update wgpu to revision 98ea3500fd2cfb4b51d5454c662d8eefd940156a. r=#webgpu-reviewers
- Bug 1813547 - Vendor wgpu changes. r=#webgpu-reviewers
- Bug 1813547 - Vet wgpu and naga commits. r=#supply-chain-reviewers

Note: The script uses mozilla-central's `Cargo.toml` file to find the version before and after updating dependencies, so you want it to be up to date before running the script (just do a build beforehand).
