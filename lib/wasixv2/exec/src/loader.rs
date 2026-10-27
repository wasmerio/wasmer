//! Turning the bytes of a process image into a compiled module: the images `execve` loads and
//! those of vfork children that called it. An embedder plugs in a [`Loader`] to pick the engine
//! per image and to cache compiled modules, as Linux's binary format handlers load the formats
//! they recognise.

use std::error::Error;

use wasixv2_host::LocalFuture;
use wasixv2_syscalls::Errno;
use wasmer::{CompileError, Engine, Module};

/// Why a [`Loader`] could not produce a module.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// The image is in no format the loader runs. From [`Loader::probe`], `execve` fails with
    /// the errno (normally `ENOEXEC`) and the caller keeps its image.
    #[error("not an executable format the loader runs ({0:?})")]
    Format(Errno),
    /// Anything else, such as a module that does not compile.
    #[error(transparent)]
    Other(Box<dyn Error + Send + Sync>),
}

impl LoadError {
    /// [`LoadError::Other`] for `err`.
    pub fn other(err: impl Into<Box<dyn Error + Send + Sync>>) -> Self {
        Self::Other(err.into())
    }
}

impl From<CompileError> for LoadError {
    fn from(err: CompileError) -> Self {
        Self::other(err)
    }
}

/// Compiles the process images that `execve` loads. A loader adds the preemption points
/// [`crate::load`] does (see [`crate::instrument`]), or the guest cannot notice signals while
/// it computes.
pub trait Loader: Send + Sync {
    /// Whether `image`, a file `execve` is about to start, is in a format [`Loader::load`]
    /// handles, from a cheap look at it: `execve` asks before the point of no return, so an
    /// image this refuses with [`LoadError::Format`] fails `execve` instead of killing the
    /// process. By default, a WebAssembly binary: wasm's magic and version 1.
    fn probe(&self, image: &[u8]) -> Result<(), LoadError> {
        match image.starts_with(WASM_HEADER) {
            true => Ok(()),
            false => Err(LoadError::Format(Errno::ENOEXEC)),
        }
    }

    /// The module for `image`, a WebAssembly binary the kernel loaded for a process that ran on
    /// `engine` until now, and the engine that compiled it, which the image's instances run on.
    fn load<'a>(
        &'a self,
        engine: &'a Engine,
        image: &'a [u8],
    ) -> LocalFuture<'a, Result<(Engine, Module), LoadError>>;
}

/// The preamble of a WebAssembly module: the magic and version 1. Components (a later
/// version) are a different format.
const WASM_HEADER: &[u8] = b"\0asm\x01\0\0\0";

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
