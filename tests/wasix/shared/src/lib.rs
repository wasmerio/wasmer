use std::time::Duration;

/// Declare a WASIX test (or tests).
///
/// ## Examples
///
/// To start declaring test suites, pass a callable to the [`declare!()`] macro
/// that accepts a [`&mut Suite`][Suite].
///
/// ```rust
/// use wasix_conformance_suite_shared::Suite;
///
/// wasix_conformance_suite_shared::declare!(|suite: &mut Suite| {
///     suite.register("greet")
///         .assert_stdout("Hello, World!");
///     suite.register("greet michael")
///         .arg("Michael")
///         .assert_stdout("Hello, Michael!");
/// });
/// ```
///
/// If a [`Assertion::ExitCode`] assertion isn't provided, the runner will
/// implicitly assume the process succeeded with an exit code of 0.
#[macro_export]
macro_rules! declare {
    ($initializer:expr) => {
        const _: () = {
            static SUITE: $crate::rt::Lazy<$crate::Suite> = $crate::rt::Lazy::new(|| {
                let mut suite = $crate::Suite::default();
                let initializer: fn(&mut $crate::Suite) = $initializer;
                initializer(&mut suite);
                suite
            });
            static SERIALIZED_SUITE: $crate::rt::Lazy<String> = $crate::rt::Lazy::new(|| {
                $crate::rt::serde_json::to_string_pretty(&*SUITE).unwrap()
            });

            #[no_mangle]
            pub extern "C" fn wcs_suite() -> *const u8 {
                SERIALIZED_SUITE.as_ptr()
            }

            #[no_mangle]
            pub extern "C" fn wcs_suite_len() -> u32 {
                SERIALIZED_SUITE.len().try_into().unwrap()
            }
        };
    };
}

/// Extra types used by the [`declare!()`] macro.
#[doc(hidden)]
pub mod rt {
    pub use once_cell::sync::Lazy;
    pub use serde_json;
}

/// A test suite builder.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Suite {
    pub specs: Vec<(String, TestCase)>,
}

impl Suite {
    /// Register a test case.
    pub fn register(&mut self, name: impl Into<String>) -> &mut TestCase {
        self.specs.push((name.into(), TestCase::default()));
        let (_, s) = self.specs.last_mut().unwrap();
        s
    }
}

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TestCase {
    /// What does this test do?
    pub description: Option<String>,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub stdin: String,
    pub assertions: Vec<Assertion>,
    /// The "current" time, as a [`Duration`] since the UNIX epoch.
    pub time_since_epoch: Option<Duration>,
}

impl TestCase {
    /// Attach some human-friendly context to this test.
    pub fn description(&mut self, description: impl Into<String>) -> &mut Self {
        self.description = Some(description.into());
        self
    }

    /// Add a command-line argument. Can be repeated.
    pub fn arg<A>(&mut self, arg: A) -> &mut Self
    where
        A: Into<String>,
    {
        self.args.push(arg.into());
        self
    }

    /// Add multiple command-line arguments. Can be repeated.
    pub fn args<I, A>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = A>,
        A: Into<String>,
    {
        for arg in args {
            self.arg(arg);
        }

        self
    }

    pub fn env(&mut self, name: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.env.push((name.into(), value.into()));
        self
    }

    pub fn envs<I, K, V>(&mut self, envs: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        for (key, value) in envs {
            self.env(key, value);
        }
        self
    }

    /// Set the "current" time, as a [`Duration`] since the UNIX epoch.
    pub fn current_time(&mut self, time_since_epoch: Duration) -> &mut Self {
        self.time_since_epoch = Some(time_since_epoch);
        self
    }

    pub fn assert_stdout(&mut self, stdout: impl Into<String>) -> &mut Self {
        self.assertions
            .push(Assertion::StdoutContains(stdout.into()));
        self
    }

    pub fn assert_stderr(&mut self, stderr: impl Into<String>) -> &mut Self {
        self.assertions
            .push(Assertion::StderrContains(stderr.into()));
        self
    }

    pub fn assert_exit_code(&mut self, code: u8) -> &mut Self {
        self.assertions.push(Assertion::ExitCode(code));
        self
    }

    pub fn assert_success(&mut self) -> &mut Self {
        self.assertions.push(Assertion::ExitCode(0));
        self
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Assertion {
    StdoutContains(String),
    StderrContains(String),
    ExitCode(u8),
}
