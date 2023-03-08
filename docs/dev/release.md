# Release Process

The release process of wasmer is mostly automated, including generating the CHANGELOG,
tagging the release and starting the `build.yaml` action 

In the `/scripts` folder, you will see three files:

- `update-version.py`: iterates through all `Cargo.toml` files and bumps the version number
according to `PREVIOUS_VERSION` and `NEXT_VERSION`
- `publish.py`: resolves the dependency order and publishes all crates to crates.io
- `make-release.py`: creates a new pull request from the current master branch, generates the
CHANGELOG, waits until all checks have passed and the release PR is merged, then starts the 
GitHub action to trigger the actual release on GitHub.

In theory, all you need to do to create a new release is to look that master is green, then
run `python3 scripts/make-release.py 3.2.0` - in practice the script can sometimes lock up or
break due to unexpected delays for example it takes a couple of seconds between a pull request
being merged and master being updated. Therefore it's best to run each step individually and
make sure that every step finishes properly.

```sh
# required before starting
gh login

# Script will create a release PR (release-3.2.0) and loop until the 
# release PR is merged into master (after checks are green)
python3 scripts/make-release.py 3.2.0

# After the release PR is merged, the build.yml workflow should be
# triggered automatically - if it isn't, trigger it manually
git checkout master
git tag v3.2.0 && git push origin v3.2.0
gh workflow run build.yml --ref v3.2.0 --field release=v3.2.0

# After the release is done on GitHub, run the script again 
# to update the release notes
python3 scripts/make-release.py 3.2.0

# Once the release on GitHub is properly done and verified that all
# artifacts exist, checkout the tag and run the publishing to crates.io
git checkout v3.2.0
python3 scripts/publish.py publish
``` 

After the GitHub release (first command), the crates need to be 
published to crates.io - the order is important because if anything 
goes wrong in the first command or a release needs to be amended
because of last-minute fixes, we can still revert the GitHub release, 
but publishing on crates.io is final because we can't yank crates 
(this has caused a lot of version-renumbering issues in the past).

## Issues to watch out for

There are a couple of problems with the scripts that you should watch out for:

- On the release pull request, the CHANGELOG might be generated incorrectly or with wrong line endings
- If the script fails, there should be an audible message (implemented using the `say` command), so that you
  can leave the script running in the background and get notified if anything goes wrong.
- The script might not trigger the `build.yml` action, in some cases it has to be run manually
- Publishing to crates.io might fail because of new crates that have to be published manually.
    - It is important to adjust the `SETTINGS` in the `publish.py` script if some crates need default-features
      to be enabled when publishing
    - crates that were never published before need to usually be published for the first time 
      by `cd lib/crate && cargo publish`
- After publishing new crates, check that the crate ownership is set to `github:wasmerio:wasmer-core`.
- The CHANGELOG is generated from the pull request titles since the last release. Sometimes these titles need
  to be fixed up to make any sense for a reader
- The release notes should just highlight the most important changes for a release, not dump everything.
- The following files should be released (TODO: more consistent naming schema):
  - wasmer-darwin-amd64.tar.gz
  - wasmer-darwin-arm64.tar.gz
  - wasmer-linux-aarch64.tar.gz
  - wasmer-linux-amd64.tar.gz
  - wasmer-linux-musl-amd64.tar.gz
  - wasmer-windows-amd64.tar.gz
  - wasmer-windows-gnu64.tar.gz
  - wasmer-windows.exe

## Videos

- [Creating the release pull request](https://www.youtube.com/watch?v=RMPTT-rnykA)
- [Triggering the build.yml action manually](https://www.youtube.com/watch?v=7mF0nlfpQfA)
   - Note that the version should always be tagged with a "v" tag
   - The commit to tag for the version should be the merge commit of the release PR
- [Publishing to crates.io](https://www.youtube.com/watch?v=uLdxIr6YwuY)
