#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};

/// Controls which experimental features will be enabled.
/// Features usually have a corresponding [WebAssembly proposal].
///
/// [WebAssembly proposal]: https://github.com/WebAssembly/proposals
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct Features {
    /// Threads proposal should be enabled
    pub threads: bool,
    /// Reference Types proposal should be enabled
    pub reference_types: bool,
    /// SIMD proposal should be enabled
    pub simd: bool,
    /// Bulk Memory proposal should be enabled
    pub bulk_memory: bool,
    /// Multi Value proposal should be enabled
    pub multi_value: bool,
}

impl Features {
    /// Create a new feature
    pub fn new() -> Self {
        Self {
            threads: false,
            reference_types: false,
            simd: false,
            bulk_memory: false,
            // Multivalue should be on by default
            multi_value: true,
        }
    }

    /// Configures whether the WebAssembly threads proposal will be enabled.
    ///
    /// The [WebAssembly threads proposal][threads] is not currently fully
    /// standardized and is undergoing development. Support for this feature can
    /// be enabled through this method for appropriate WebAssembly modules.
    ///
    /// This feature gates items such as shared memories and atomic
    /// instructions.
    ///
    /// This is `false` by default.
    ///
    /// [threads]: https://github.com/webassembly/threads
    pub fn threads(&mut self, enable: bool) -> &mut Self {
        self.threads = enable;
        self
    }

    /// Configures whether the WebAssembly reference types proposal will be
    /// enabled.
    ///
    /// The [WebAssembly reference types proposal][proposal] is not currently
    /// fully standardized and is undergoing development. Support for this
    /// feature can be enabled through this method for appropriate WebAssembly
    /// modules.
    ///
    /// This feature gates items such as the `anyref` type and multiple tables
    /// being in a module. Note that enabling the reference types feature will
    /// also enable the bulk memory feature.
    ///
    /// This is `false` by default.
    ///
    /// [proposal]: https://github.com/webassembly/reference-types
    pub fn reference_types(&mut self, enable: bool) -> &mut Self {
        self.reference_types = enable;
        // The reference types proposal depends on the bulk memory proposal
        if enable {
            self.bulk_memory(true);
        }
        self
    }

    /// Configures whether the WebAssembly SIMD proposal will be
    /// enabled.
    ///
    /// The [WebAssembly SIMD proposal][proposal] is not currently
    /// fully standardized and is undergoing development. Support for this
    /// feature can be enabled through this method for appropriate WebAssembly
    /// modules.
    ///
    /// This feature gates items such as the `v128` type and all of its
    /// operators being in a module.
    ///
    /// This is `false` by default.
    ///
    /// [proposal]: https://github.com/webassembly/simd
    pub fn simd(&mut self, enable: bool) -> &mut Self {
        self.simd = enable;
        self
    }

    /// Configures whether the WebAssembly bulk memory operations proposal will
    /// be enabled.
    ///
    /// The [WebAssembly bulk memory operations proposal][proposal] is not
    /// currently fully standardized and is undergoing development.
    /// Support for this feature can be enabled through this method for
    /// appropriate WebAssembly modules.
    ///
    /// This feature gates items such as the `memory.copy` instruction, passive
    /// data/table segments, etc, being in a module.
    ///
    /// This is `false` by default.
    ///
    /// [proposal]: https://github.com/webassembly/bulk-memory-operations
    pub fn bulk_memory(&mut self, enable: bool) -> &mut Self {
        self.bulk_memory = enable;
        // In case is false, we disable both threads and reference types
        // since they both depend on bulk memory
        if !enable {
            self.reference_types(false);
        }
        self
    }

    /// Configures whether the WebAssembly multi-value proposal will
    /// be enabled.
    ///
    /// The [WebAssembly multi-value proposal][proposal] is not
    /// currently fully standardized and is undergoing development.
    /// Support for this feature can be enabled through this method for
    /// appropriate WebAssembly modules.
    ///
    /// This feature gates functions and blocks returning multiple values in a
    /// module, for example.
    ///
    /// This is `false` by default.
    ///
    /// [proposal]: https://github.com/webassembly/multi-value
    pub fn multi_value(&mut self, enable: bool) -> &mut Self {
        self.multi_value = enable;
        self
    }
}

impl Default for Features {
    fn default() -> Features {
        Features::new()
    }
}

#[cfg(test)]
mod test_features {
    use super::*;
    #[test]
    fn default_features() {
        let default = Features::default();
        assert_eq!(
            default,
            Features {
                threads: false,
                reference_types: false,
                simd: false,
                bulk_memory: false,
                multi_value: false,
            }
        );
    }

    #[test]
    fn enable_threads() {
        let mut features = Features::new();
        features.bulk_memory(false).threads(true);

        assert!(features.threads);
    }

    #[test]
    fn enable_reference_types() {
        let mut features = Features::new();
        features.bulk_memory(false).reference_types(true);
        assert!(features.reference_types);
        assert!(features.bulk_memory);
    }

    #[test]
    fn enable_simd() {
        let mut features = Features::new();
        features.simd(true);
        assert!(features.simd);
    }

    #[test]
    fn enable_multi_value() {
        let mut features = Features::new();
        features.multi_value(true);
        assert!(features.multi_value);
    }

    #[test]
    fn enable_bulk_memory() {
        let mut features = Features::new();
        features.bulk_memory(true);
        assert!(features.bulk_memory);
    }

    #[test]
    fn disable_bulk_memory() {
        let mut features = Features::new();
        features
            .threads(true)
            .reference_types(true)
            .bulk_memory(false);
        assert!(!features.bulk_memory);
        assert!(!features.reference_types);
    }
}
