# How to Contribute to Wasmer

Thank you for your interest in contributing to Wasmer. This document outlines some recommendations on how to contribute.

## Issues & Feature Requests

Please use the issue template and provide a failing example if possible to help us recreate the issue.

## Pull Requests

For large changes, please try reaching communicating with the Wasmer maintainers via GitHub Issues or Spectrum Chat to ensure we can accept the change once it is ready.

We recommend trying the following commands before sending a pull request to ensure code quality:

- `cargo fmt --all` Ensures all code is correctly formatted.
- Run `cargo test` in the crates that you are modifying.
- Run `cargo build --all`.

A comprehensive CI test suite will be run by a Wasmer team member after the PR has been created.

### Common Build Issues

#### LLVM Dependency

`Didn't find usable system-wide LLVM`

Building Wasmer with the LLVM backend requires LLVM 14 or better to be installed
On debian family you need `sudo apt install llvm14 libclang-common-14-dev libpolly-14-dev`
