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
run 

```sh
python3 scripts/make-release.py 3.2.0
python3 scripts/publish.py publish
``` 

After the GitHub release (first command), the crates need to be published to crates.io - the order
is important because if anything goes wrong in the first command or a release needs to be amended
because of last-minute fixes, we can still revert the GitHub release, but publishing on crates.io
is final because we can't yank crates (this has caused a lot of version-renumbering issues in the past).

## Issues to watch out for

There are a couple of problems with the scripts that you should watch out for:

- On the release pull request, the CHANGELOG might be generated incorrectly
- The script might fail (in this case there will be an audible message being read using the macos `say` command)
- The script might not trigger the `build.yaml` action, in some cases it has to be run manually
- Publishing to crates.io might fail because of new crates that have to be published manually