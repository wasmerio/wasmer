//! Turning the bytes of a process image into a compiled module: the images `execve` loads and
//! those of vfork children that called it. An embedder plugs in a [`Loader`] to pick the engine
//! per image and to cache compiled modules, as Linux's binary format handlers load the formats
//! they recognise.

use wasixv2_host::LocalFuture;
use wasmer::{Engine, Module};

/// Why a [`Loader`] could not produce a module.
pub type LoadError = Box<dyn std::error::Error + Send + Sync>;

/// Compiles the process images that `execve` loads. A loader adds the preemption points
/// [`crate::load`] does (see [`crate::instrument`]), or the guest cannot notice signals while
/// it computes.
pub trait Loader: Send + Sync {
    /// The module for `image`, a WebAssembly binary the kernel loaded for a process that ran on
    /// `engine` until now, and the engine that compiled it, which the image's instances run on.
    fn load<'a>(
        &'a self,
        engine: &'a Engine,
        image: &'a [u8],
    ) -> LocalFuture<'a, Result<(Engine, Module), LoadError>>;
}

/// The default [`Loader`]: [`crate::load`] with the engine the process ran on, without a cache.
pub struct Compile;

impl Loader for Compile {
    fn load<'a>(
        &'a self,
        engine: &'a Engine,
        image: &'a [u8],
    ) -> LocalFuture<'a, Result<(Engine, Module), LoadError>> {
        Box::pin(async move { Ok((engine.clone(), crate::load(engine, image)?)) })
    }
}
