<div align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo.png" alt="Wasmer logo">
  </a>

  <h1>The <code>wasmer-cache</code> library</h1>

  <p>
    <a href="https://github.com/wasmerio/wasmer/actions?query=workflow%3Abuild">
      <img src="https://github.com/wasmerio/wasmer/workflows/build/badge.svg?style=flat-square" alt="Build Status" />
    </a>
    <a href="https://github.com/wasmerio/wasmer/blob/master/LICENSE">
      <img src="https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square" alt="License" />
    </a>
    <a href="https://slack.wasmer.io">
      <img src="https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square" alt="Slack channel" />
    </a>
    <a href="https://crates.io/crates/wasmer-cache">
      <img src="https://img.shields.io/crates/v/wasmer-cache.svg?style=flat-square" alt="crates.io" />
    </a>
    <a href="https://wasmerio.github.io/wasmer/crates/wasmer_cache/">
      <img src="https://img.shields.io/badge/documentation-read-informational?style=flat-square" alt="documentation" />
    </a>
  </p>
</div>

<br />

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
