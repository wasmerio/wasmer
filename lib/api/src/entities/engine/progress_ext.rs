use wasmer_types::CompilationProgressCallback;

/// Provides progress-related extensions to the `Engine` trait.
pub trait ProgressEngineExt {
    /// Compile a module from bytes with a progress callback.
    ///
    /// The callback is invoked with progress updates during the compilation process.
    /// The callback also may return an error to abort the compilation.
    ///
    /// Signature of the callback function: `Fn(CompilationProgress) -> Result<(), UserAbort> + Send + Sync + 'static`
    ///
    /// # Aborting compilation
    ///
    /// The callback has to return a `Result<(), UserAbort>`.
    ///
    /// If the callback returns an error, the compilation will fail with a `CompileError::Aborted`.
    ///
    /// See [`CompilationProgressCallback::new`] for more details.
    ///
    /// **NOTE**: Not all engines/backends support progress reporting.
    fn new_module_with_progress(
        &self,
        bytes: &[u8],
        on_progress: CompilationProgressCallback,
    ) -> Result<crate::Module, wasmer_types::CompileError>;
}

impl ProgressEngineExt for crate::Engine {
    /// See [`ProgressEngineExt::new_module_with_progress`].
    fn new_module_with_progress(
        &self,
        bytes: &[u8],
        on_progress: CompilationProgressCallback,
    ) -> Result<crate::Module, wasmer_types::CompileError> {
        crate::BackendModule::new_with_progress(self, bytes, on_progress).map(crate::Module)
    }
}
