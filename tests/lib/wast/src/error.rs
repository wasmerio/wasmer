use std::fmt;
use thiserror::Error;

/// A Directive Error
#[derive(Debug)]
pub struct DirectiveError {
    /// The line where the directive is defined
    pub line: usize,
    /// The column where the directive is defined
    pub col: usize,
    /// The failing message received when running the directive
    pub message: String,
}

/// A structure holding the list of all executed directives
#[derive(Error, Debug)]
pub struct DirectiveErrors {
    /// The filename where the error occured
    pub filename: String,
    /// The list of errors
    pub errors: Vec<DirectiveError>,
}

impl fmt::Display for DirectiveErrors {
    // This trait requires `fmt` with this exact signature.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Write strictly the first element into the supplied output
        // stream: `f`. Returns `fmt::Result` which indicates whether the
        // operation succeeded or failed. Note that `write!` uses syntax which
        // is very similar to `println!`.
        writeln!(f, "Failed directives on {}:", self.filename)?;
        for error in self.errors.iter() {
            writeln!(f, "  • {} ({}:{})", error.message, error.line, error.col)?;
        }
        Ok(())
    }
}
