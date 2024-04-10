# wasmer-config

[![Continuous Integration](https://github.com/wasmerio/wasmer-toml/actions/workflows/ci.yml/badge.svg)](https://github.com/wasmerio/wasmer-toml/actions/workflows/ci.yml)

([API Docs](https://wasmerio.github.io/wasmer-toml))

Provides configuration types for Wasmer.

## For Developers

### Releasing

This repository uses [Release Please][release-please] to automate a lot of the
work around creating releases.

Every time a commit following the [Conventional Commit Style][conv] is merged
into `main`, the [`release-please.yml`](.github/workflows/release-please.yml)
workflow will run and update the "Release PR" to reflect the new changes.

For commits that just fix bugs (i.e. the message starts with `"fix: "`), the
associated crate will receive a changelog entry and a patch version bump.
Similarly, adding a new feature (i.e. `"feat:"`) does a minor version bump and
adding breaking changes (i.e. `"fix!:"` or `"feat!:"`) will result in a major
version bump.

When the release PR is merged, the updated changelogs and bumped version numbers
will be merged into the `main` branch, the `release-please.yml` workflow will
automatically generate GitHub Releases, and CI will publish the crate if
necessary.

TL;DR:

1. Use [Conventional Commit Messages][conv] whenever you make a noteworthy change
2. Merge the release PR when ready to release
3. Let the automation do everything else

## License

This project is licensed under the MIT license ([LICENSE](./LICENSE) or
<http://opensource.org/licenses/MIT>).

It is recommended to always use [`cargo crev`][crev] to verify the
trustworthiness of each of your dependencies, including this one.

[conv]: https://www.conventionalcommits.org/en/v1.0.0/
[crev]: https://github.com/crev-dev/cargo-crev
[release-please]: https://github.com/googleapis/release-please
[wasmer]: https://wasmer.io/
