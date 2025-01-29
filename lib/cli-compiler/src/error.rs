//! Implements `PretyError` to print pretty errors in the CLI (when they happen)

use anyhow::{Chain, Error};
use colored::*;
use std::fmt::{self, Debug, Write};

/// A `PrettyError` for printing `anyhow::Error` nicely.
pub struct PrettyError {
    error: Error,
}

/// A macro that prints a warning with nice colors
#[macro_export]
macro_rules! warning {
    ($($arg:tt)*) => ({
        use colored::*;
        eprintln!("{}: {}", "warning".yellow().bold(), format!($($arg)*));
    })
}

impl PrettyError {
    /// Process a `Result` printing any errors and exiting
    /// the process after
    pub fn report<T>(result: Result<T, Error>) -> ! {
        std::process::exit(match result {
            Ok(_t) => 0,
            Err(error) => {
                eprintln!("{:?}", PrettyError { error });
                1
            }
        });
    }
}

impl Debug for PrettyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let error = &self.error;

        if f.alternate() {
            return Debug::fmt(&error, f);
        }

        write!(f, "{}", format!("{}: {}", "error".red(), error).bold())?;
        // write!(f, "{}", error)?;

        if let Some(cause) = error.source() {
            // write!(f, "\n{}:", "caused by".bold().blue())?;
            let chain = Chain::new(cause);
            let (total_errors, _) = chain.size_hint();
            for (n, error) in chain.enumerate() {
                writeln!(f)?;
                let mut indented = Indented {
                    inner: f,
                    number: Some(n + 1),
                    is_last: n == total_errors - 1,
                    started: false,
                };
                write!(indented, "{error}")?;
            }
        }
        Ok(())
    }
}

struct Indented<'a, D> {
    inner: &'a mut D,
    number: Option<usize>,
    started: bool,
    is_last: bool,
}

impl<T> Write for Indented<'_, T>
where
    T: Write,
{
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for (i, line) in s.split('\n').enumerate() {
            if !self.started {
                self.started = true;
                match self.number {
                    Some(number) => {
                        if !self.is_last {
                            write!(
                                self.inner,
                                "{} {: >4} ",
                                "│".bold().blue(),
                                format!("{number}:").dimmed()
                            )?
                        } else {
                            write!(
                                self.inner,
                                "{}{: >2}: ",
                                "╰─▶".bold().blue(),
                                format!("{number}").bold().blue()
                            )?
                        }
                    }
                    None => self.inner.write_str("    ")?,
                }
            } else if i > 0 {
                self.inner.write_char('\n')?;
                if self.number.is_some() {
                    self.inner.write_str("       ")?;
                } else {
                    self.inner.write_str("    ")?;
                }
            }

            self.inner.write_str(line)?;
        }

        Ok(())
    }
}
