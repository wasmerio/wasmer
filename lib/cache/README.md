# `wasmer-cache` [![Build Status](https://github.com/wasmerio/wasmer/workflows/build/badge.svg?style=flat-square)](https://github.com/wasmerio/wasmer/actions?query=workflow%3Abuild) [![Join Wasmer Slack](https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square)](https://slack.wasmer.io) [![MIT License](https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square)](https://github.com/wasmerio/wasmer/blob/main/LICENSE)

The `wasmer-cache` crate allows to cache WebAssembly modules (of kind
`wasmer::Module`) in your system, so that next uses of the module does
imply a compilation time.

## Usage

The `Cache` trait represents a generic cache for storing and loading
compiled WebAssembly modules. The `FileSystemCache` type implements
`Cache` to store cache on the file system.

```rust
use wasmer::{DeserializeError, Module, SerializeError};
use wasmer_cache::{Cache, FileSystemCache, Hash};

fn store_module(module: &Module, bytes: &[u8]) -> Result<(), SerializeError> {
    // Create a new file system cache.
    let mut fs_cache = FileSystemCache::new("some/directory/goes/here")?;

    // Compute a key for a given WebAssembly binary
    let hash = Hash::generate(bytes);

    // Store a module into the cache given a key
    fs_cache.store(hash, module.clone())?;

    Ok(())
}
```
