# How to Contribute to Wasmer

Thank you for your interest in contributing to Wasmer. This document
outlines the expectations for issues and pull requests.

## Issues & Feature Requests

Please use the issue template and provide a failing example if possible to
help us recreate the issue.

> [!WARNING]
> Do not fix a security vulnerability in a public PR. Embargoed work goes
> through the private process in [SECURITY.md](./SECURITY.md). A public fix
> before disclosure exposes users.

## Code Style

Sparse "why" comments. Short single-responsibility functions.

## Lint and Format

CI rejects lint and format failures. Run before you push:

```bash
make lint
```

The `lint` target in the [Makefile](../Makefile) defines exactly what
runs. To apply clippy fixes in bulk:

```bash
cargo clippy --all --exclude wasmer-swift --locked --fix --allow-dirty -- -D clippy::all
```

`cargo fmt` does not cover `lib/wasix/tests/wasm_tests`. CI formats those
files with `rustfmt --edition 2024` directly.

## Pull Requests

For large changes, open a GitHub issue first to ensure we can accept the
change once it is ready.

- Title format is conventional-commit style with crate or area scopes:
  `fix(Singlepass): ...`, `feat(wasix): ...`, `feat!: ...` for breaking
  changes, `chore: ...`, `deps: ...`.
- PRs are squash-merged to `main`. The PR template has one `# Description`
  section.
- Before you submit: `make lint`, then `cargo test` in the crates you
  touched, then a build of the CLI.
- Do not hand-edit `CHANGELOG.md` or crate versions. Releases generate
  both (see [dev/release.md](./dev/release.md)).
- macOS and musl CI jobs run on a PR only when the PR has the `macos` or
  `musl` label. If your change touches platform code, add the label.
- Do not commit a submodule pointer bump in an unrelated PR.

## Common Build Issues

### LLVM Dependency

`Didn't find usable system-wide LLVM` means LLVM 22 is missing. Either
install it or build with `ENABLE_LLVM=0`. See
[Building Wasmer from Source](./BUILD.md#llvm-compiler) for installation
instructions.
