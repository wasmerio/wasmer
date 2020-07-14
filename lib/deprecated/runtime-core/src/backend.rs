use crate::new;

pub use new::wasmer_compiler::{Architecture, CpuFeature as Features};

/// Enum used to select which compiler should be used to generate code.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Backend {
    #[cfg(feature = "singlepass")]
    /// Singlepass backend
    Singlepass,
    #[cfg(feature = "cranelift")]
    /// Cranelift backend
    Cranelift,
    #[cfg(feature = "llvm")]
    /// LLVM backend
    LLVM,
    /// Auto backend
    Auto,
}

impl Backend {
    /// Get a list of the currently enabled (via feature flag) backends.
    pub fn variants() -> &'static [&'static str] {
        &[
            #[cfg(feature = "singlepass")]
            "singlepass",
            #[cfg(feature = "cranelift")]
            "cranelift",
            #[cfg(feature = "llvm")]
            "llvm",
            "auto",
        ]
    }

    /// Stable string representation of the backend.
    /// It can be used as part of a cache key, for example.
    pub fn to_string(&self) -> &'static str {
        match self {
            #[cfg(feature = "singlepass")]
            Backend::Singlepass => "singlepass",
            #[cfg(feature = "cranelift")]
            Backend::Cranelift => "cranelift",
            #[cfg(feature = "llvm")]
            Backend::LLVM => "llvm",
            Backend::Auto => "auto",
        }
    }
}

impl Default for Backend {
    fn default() -> Self {
        #[cfg(feature = "default-backend-singlepass")]
        return Backend::Singlepass;

        #[cfg(feature = "default-backend-cranelift")]
        return Backend::Cranelift;

        #[cfg(feature = "default-backend-llvm")]
        return Backend::LLVM;

        #[cfg(not(any(
            feature = "default-backend-singlepass",
            feature = "default-backend-cranelift",
            feature = "default-backend-llvm",
        )))]
        panic!("There is no default-backend set.");
    }
}

impl std::str::FromStr for Backend {
    type Err = String;
    fn from_str(s: &str) -> Result<Backend, String> {
        match s.to_lowercase().as_str() {
            #[cfg(feature = "singlepass")]
            "singlepass" => Ok(Backend::Singlepass),
            #[cfg(feature = "cranelift")]
            "cranelift" => Ok(Backend::Cranelift),
            #[cfg(feature = "llvm")]
            "llvm" => Ok(Backend::LLVM),
            "auto" => Ok(Backend::Auto),
            _ => Err(format!("The backend {} doesn't exist", s)),
        }
    }
}
