# How to Contribute to Wasmer

Thank you for your interest in contributing to Wasmer. This document outlines some recommendations on how to contribute.

## Issues & Feature Requests
Please use the issue template and provide a failing example if possible to help us recreate the issue.

## Pull Requests
For large changes, please try reaching the Wasmer using Github Issues or Spectrum Chat to ensure we can accept the change once it is ready.  

We recommend trying the following commands before sending a pull request to ensure code quality:
- `cargo fmt --all` Ensures all code is correctly formatted.
- Run `cargo test` in the crates that you are modifying.
- Run `cargo build --all` (nightly) or `cargo build --all --exclude wasmer-singlepass-backend`

A comprehensive CI test suite will be run by a Wasmer team member after the PR has been created.

### Common Build Issues

**LLVM Dependency**

The LLVM backend requires LLVM to be installed to compile.

So, you may run into the following error:
```
Didn't find usable system-wide LLVM.
No suitable version of LLVM was found system-wide or pointed
```

**Singlepass Nightly Only**

The singlepass crate depends on nightly so you may need to add the `+nightly` cargo flag to compile this crate.
`error[E0554]: #![feature] may not be used on the stable release channel`
