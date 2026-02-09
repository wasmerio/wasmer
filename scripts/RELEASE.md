# 🔧 Wasmer Release Guide

## 1. Prepare the Branch

Check out `main` and ensure you're on the latest commit

```bash
git checkout main
git pull
```

## 2. Generate the Release PR

Run the release script (adjust the version as needed):

```bash
python3 ./scripts/make-release.py v6.1.0
```

Wait for the script to open a PR and note the PR branch name.

## 3. Edit the Changelog

 Check out the PR branch:

```bash
git checkout <release-branch>
```

Update the auto-generated `CHANGELOG.md`:

- Verify the commits listed
- Add a proper summary of what's new

Commit and push your changes.

## 4. Merge the Release PR

Wait for CI to pass and merge the PR into `main`.
The `make-release.py` script automatically monitors the PR and both tags the release and properly publishes artifacts. Plus, a new **draft release** is created as well.

## 5. Publish the Release

Open the draft release, add a meaningful description (often the CHANGELOG summary), and publish it.

## 6. Publish Crates

Locally, check out the release tag and publish the crates:

```bash
git checkout v6.1.0
python3 ./scripts/publish.py publish
```

🎉 **Release complete!**
