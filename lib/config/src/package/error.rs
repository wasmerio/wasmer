/// Error that occurs during package ident/source parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageParseError {
    value: String,
    message: String,
}

impl PackageParseError {
    pub(crate) fn new(value: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for PackageParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "could not parse value as package identifier: {} (value: '{}')",
            self.message, self.value
        )
    }
}

impl std::error::Error for PackageParseError {}
