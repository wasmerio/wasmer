use wasmer_types::{CompilationProgressCallback, CompileError};

/// Compilation progress reporting binary size in defined chunks.
#[derive(Debug)]
pub(crate) struct ChunkedOutputReporter<'a> {
    progress_callback: Option<&'a CompilationProgressCallback>,
    accounted: usize,
    current: usize,
}

// TODO: move
const CHUNK_SIZE: usize = 1024;

impl<'a> ChunkedOutputReporter<'a> {
    pub(crate) fn new(progress_callback: Option<&'a CompilationProgressCallback>) -> Self {
        Self {
            progress_callback,
            accounted: 0,
            current: 0,
        }
    }

    #[inline]
    pub(crate) fn check(&mut self, output_size: usize) -> Result<(), CompileError> {
        let Some(progress_callback) = self.progress_callback.as_ref() else {
            return Ok(());
        };

        debug_assert!(output_size >= self.accounted);
        self.current = output_size;
        let pending = self.current - self.accounted;

        if pending >= CHUNK_SIZE {
            // report the entire difference
            self.accounted = self.current;
            progress_callback.reserve(pending)?;
        }

        Ok(())
    }

    pub(crate) fn finish(mut self, output_size: usize) -> Result<(), CompileError> {
        let Some(progress_callback) = self.progress_callback.as_ref() else {
            return Ok(());
        };

        debug_assert!(output_size >= self.accounted);
        self.current = output_size;
        let pending = self.current - self.accounted;
        self.accounted = self.current;

        Ok(progress_callback.reserve(pending)?)
    }
}

impl<'a> Drop for ChunkedOutputReporter<'a> {
    fn drop(&mut self) {
        debug_assert_eq!(
            self.current, self.accounted,
            "local output reporter dropped without calling finish"
        );
    }
}
