use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "detect-wasm-features")]
use wasmparser::{Parser, Payload, Validator, WasmFeatures};

/// Controls which experimental features will be enabled.
/// Features usually have a corresponding [WebAssembly proposal].
///
/// [WebAssembly proposal]: https://github.com/WebAssembly/proposals
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
#[derive(RkyvSerialize, RkyvDeserialize, Archive)]
#[rkyv(derive(Debug), compare(PartialEq))]
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
    /// Tail call proposal should be enabled
    pub tail_call: bool,
    /// Module Linking proposal should be enabled
    pub module_linking: bool,
    /// Multi Memory proposal should be enabled
    pub multi_memory: bool,
    /// 64-bit Memory proposal should be enabled
    pub memory64: bool,
    /// Wasm exceptions proposal should be enabled
    pub exceptions: bool,
    /// Relaxed SIMD proposal should be enabled
    pub relaxed_simd: bool,
    /// Extended constant expressions proposal should be enabled
    pub extended_const: bool,
}

impl Features {
    /// Create a new feature
    pub fn new() -> Self {
        Self {
            threads: true,
            // Reference types should be on by default
            reference_types: true,
            // SIMD should be on by default
            simd: true,
            // Bulk Memory should be on by default
            bulk_memory: true,
            // Multivalue should be on by default
            multi_value: true,
            tail_call: false,
            module_linking: false,
            multi_memory: false,
            memory64: false,
            exceptions: false,
            relaxed_simd: false,
            extended_const: false,
        }
    }

    /// Create a new feature set with all features enabled.
    pub fn all() -> Self {
        Self {
            threads: true,
            reference_types: true,
            simd: true,
            bulk_memory: true,
            multi_value: true,
            tail_call: true,
            module_linking: true,
            multi_memory: true,
            memory64: true,
            exceptions: true,
            relaxed_simd: true,
            extended_const: true,
        }
    }

    /// Create a new feature set with all features disabled.
    pub fn none() -> Self {
        Self {
            threads: false,
            reference_types: false,
            simd: false,
            bulk_memory: false,
            multi_value: false,
            tail_call: false,
            module_linking: false,
            multi_memory: false,
            memory64: false,
            exceptions: false,
            relaxed_simd: false,
            extended_const: false,
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
    /// The [WebAssembly reference types proposal][proposal] is now
    /// fully standardized and enabled by default.
    ///
    /// This feature gates items such as the `externref` type and multiple tables
    /// being in a module. Note that enabling the reference types feature will
    /// also enable the bulk memory feature.
    ///
    /// This is `true` by default.
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
    /// The [WebAssembly bulk memory operations proposal][proposal] is now
    /// fully standardized and enabled by default.
    ///
    /// This feature gates items such as the `memory.copy` instruction, passive
    /// data/table segments, etc, being in a module.
    ///
    /// This is `true` by default.
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
    /// The [WebAssembly multi-value proposal][proposal] is now fully
    /// standardized and enabled by default, except with the singlepass
    /// compiler which does not support it.
    ///
    /// This feature gates functions and blocks returning multiple values in a
    /// module, for example.
    ///
    /// This is `true` by default.
    ///
    /// [proposal]: https://github.com/webassembly/multi-value
    pub fn multi_value(&mut self, enable: bool) -> &mut Self {
        self.multi_value = enable;
        self
    }

    /// Configures whether the WebAssembly tail-call proposal will
    /// be enabled.
    ///
    /// The [WebAssembly tail-call proposal][proposal] is not
    /// currently fully standardized and is undergoing development.
    /// Support for this feature can be enabled through this method for
    /// appropriate WebAssembly modules.
    ///
    /// This feature gates tail-call functions in WebAssembly.
    ///
    /// This is `false` by default.
    ///
    /// [proposal]: https://github.com/webassembly/tail-call
    pub fn tail_call(&mut self, enable: bool) -> &mut Self {
        self.tail_call = enable;
        self
    }

    /// Configures whether the WebAssembly module linking proposal will
    /// be enabled.
    ///
    /// The [WebAssembly module linking proposal][proposal] is not
    /// currently fully standardized and is undergoing development.
    /// Support for this feature can be enabled through this method for
    /// appropriate WebAssembly modules.
    ///
    /// This feature allows WebAssembly modules to define, import and
    /// export modules and instances.
    ///
    /// This is `false` by default.
    ///
    /// [proposal]: https://github.com/webassembly/module-linking
    pub fn module_linking(&mut self, enable: bool) -> &mut Self {
        self.module_linking = enable;
        self
    }

    /// Configures whether the WebAssembly multi-memory proposal will
    /// be enabled.
    ///
    /// The [WebAssembly multi-memory proposal][proposal] is not
    /// currently fully standardized and is undergoing development.
    /// Support for this feature can be enabled through this method for
    /// appropriate WebAssembly modules.
    ///
    /// This feature adds the ability to use multiple memories within a
    /// single Wasm module.
    ///
    /// This is `false` by default.
    ///
    /// [proposal]: https://github.com/WebAssembly/multi-memory
    pub fn multi_memory(&mut self, enable: bool) -> &mut Self {
        self.multi_memory = enable;
        self
    }

    /// Configures whether the WebAssembly 64-bit memory proposal will
    /// be enabled.
    ///
    /// The [WebAssembly 64-bit memory proposal][proposal] is not
    /// currently fully standardized and is undergoing development.
    /// Support for this feature can be enabled through this method for
    /// appropriate WebAssembly modules.
    ///
    /// This feature gates support for linear memory of sizes larger than
    /// 2^32 bits.
    ///
    /// This is `false` by default.
    ///
    /// [proposal]: https://github.com/WebAssembly/memory64
    pub fn memory64(&mut self, enable: bool) -> &mut Self {
        self.memory64 = enable;
        self
    }

    /// Configures whether the WebAssembly exception-handling proposal will be enabled.
    ///
    /// The [WebAssembly exception-handling proposal][eh] is not currently fully
    /// standardized and is undergoing development. Support for this feature can
    /// be enabled through this method for appropriate WebAssembly modules.
    ///
    /// This is `false` by default.
    ///
    /// [eh]: https://github.com/webassembly/exception-handling
    pub fn exceptions(&mut self, enable: bool) -> &mut Self {
        self.exceptions = enable;
        self
    }

    /// Checks if this features set contains all the features required by another set
    pub fn contains_features(&self, required: &Self) -> bool {
        // Check all required features
        (!required.simd || self.simd)
            && (!required.bulk_memory || self.bulk_memory)
            && (!required.reference_types || self.reference_types)
            && (!required.threads || self.threads)
            && (!required.multi_value || self.multi_value)
            && (!required.exceptions || self.exceptions)
            && (!required.tail_call || self.tail_call)
            && (!required.module_linking || self.module_linking)
            && (!required.multi_memory || self.multi_memory)
            && (!required.memory64 || self.memory64)
            && (!required.relaxed_simd || self.relaxed_simd)
            && (!required.extended_const || self.extended_const)
    }

    #[cfg(feature = "detect-wasm-features")]
    /// Detects required WebAssembly features from a module binary.
    ///
    /// This method analyzes a WebAssembly module's binary to determine which
    /// features it requires. It does this by:
    /// 1. Attempting to validate the module with different feature sets
    /// 2. Analyzing validation errors to detect required features
    /// 3. Parsing the module to detect certain common patterns
    ///
    /// # Arguments
    ///
    /// * `wasm_bytes` - The binary content of the WebAssembly module
    ///
    /// # Returns
    ///
    /// A new `Features` instance with the detected features enabled.
    pub fn detect_from_wasm(wasm_bytes: &[u8]) -> Result<Self, wasmparser::BinaryReaderError> {
        let mut features = Self::default();

        // Simple test for exceptions - try to validate with exceptions disabled
        let mut exceptions_test = WasmFeatures::default();
        // Enable most features except exceptions
        exceptions_test.set(WasmFeatures::BULK_MEMORY, true);
        exceptions_test.set(WasmFeatures::REFERENCE_TYPES, true);
        exceptions_test.set(WasmFeatures::SIMD, true);
        exceptions_test.set(WasmFeatures::MULTI_VALUE, true);
        exceptions_test.set(WasmFeatures::THREADS, true);
        exceptions_test.set(WasmFeatures::TAIL_CALL, true);
        exceptions_test.set(WasmFeatures::MULTI_MEMORY, true);
        exceptions_test.set(WasmFeatures::MEMORY64, true);
        exceptions_test.set(WasmFeatures::EXCEPTIONS, false);

        let mut validator = Validator::new_with_features(exceptions_test);

        if let Err(e) = validator.validate_all(wasm_bytes) {
            let err_msg = e.to_string();
            if err_msg.contains("exception") {
                features.exceptions(true);
            }
        }

        // Now try with all features enabled to catch anything we might have missed
        let mut wasm_features = WasmFeatures::default();
        wasm_features.set(WasmFeatures::EXCEPTIONS, true);
        wasm_features.set(WasmFeatures::BULK_MEMORY, true);
        wasm_features.set(WasmFeatures::REFERENCE_TYPES, true);
        wasm_features.set(WasmFeatures::SIMD, true);
        wasm_features.set(WasmFeatures::MULTI_VALUE, true);
        wasm_features.set(WasmFeatures::THREADS, true);
        wasm_features.set(WasmFeatures::TAIL_CALL, true);
        wasm_features.set(WasmFeatures::MULTI_MEMORY, true);
        wasm_features.set(WasmFeatures::MEMORY64, true);

        let mut validator = Validator::new_with_features(wasm_features);
        match validator.validate_all(wasm_bytes) {
            Err(e) => {
                // If validation fails due to missing feature support, check which feature it is
                let err_msg = e.to_string().to_lowercase();

                if err_msg.contains("exception") || err_msg.contains("try/catch") {
                    features.exceptions(true);
                }

                if err_msg.contains("bulk memory") {
                    features.bulk_memory(true);
                }

                if err_msg.contains("reference type") {
                    features.reference_types(true);
                }

                if err_msg.contains("simd") {
                    features.simd(true);
                }

                if err_msg.contains("multi value") || err_msg.contains("multiple values") {
                    features.multi_value(true);
                }

                if err_msg.contains("thread") || err_msg.contains("shared memory") {
                    features.threads(true);
                }

                if err_msg.contains("tail call") {
                    features.tail_call(true);
                }

                if err_msg.contains("module linking") {
                    features.module_linking(true);
                }

                if err_msg.contains("multi memory") {
                    features.multi_memory(true);
                }

                if err_msg.contains("memory64") {
                    features.memory64(true);
                }
            }
            Ok(_) => {
                // The module validated successfully with all features enabled,
                // which means it could potentially use any of them.
                // We'll do a more detailed analysis by parsing the module.
            }
        }

        // A simple pass to detect certain common patterns
        for payload in Parser::new(0).parse_all(wasm_bytes) {
            let payload = payload?;
            if let Payload::CustomSection(section) = payload {
                let name = section.name();
                // Exception handling has a custom section
                if name.contains("exception") {
                    features.exceptions(true);
                }
            }
        }

        Ok(features)
    }

    /// Extend this feature set with another set.
    ///
    /// Self will be modified to include all features that are required by
    /// either set.
    pub fn extend(&mut self, other: &Self) {
        // Written this way to cause compile errors when new features are added.
        let Self {
            threads,
            reference_types,
            simd,
            bulk_memory,
            multi_value,
            tail_call,
            module_linking,
            multi_memory,
            memory64,
            exceptions,
            relaxed_simd,
            extended_const,
        } = other.clone();

        *self = Self {
            threads: self.threads || threads,
            reference_types: self.reference_types || reference_types,
            simd: self.simd || simd,
            bulk_memory: self.bulk_memory || bulk_memory,
            multi_value: self.multi_value || multi_value,
            tail_call: self.tail_call || tail_call,
            module_linking: self.module_linking || module_linking,
            multi_memory: self.multi_memory || multi_memory,
            memory64: self.memory64 || memory64,
            exceptions: self.exceptions || exceptions,
            relaxed_simd: self.relaxed_simd || relaxed_simd,
            extended_const: self.extended_const || extended_const,
        };
    }
}

impl Default for Features {
    fn default() -> Self {
        Self::new()
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
                threads: true,
                reference_types: true,
                simd: true,
                bulk_memory: true,
                multi_value: true,
                tail_call: false,
                module_linking: false,
                multi_memory: false,
                memory64: false,
                exceptions: false,
                relaxed_simd: false,
                extended_const: false,
            }
        );
    }

    #[test]
    fn features_extend() {
        let all = Features::all();
        let mut target = Features::none();
        target.extend(&all);
        assert_eq!(target, all);
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

    #[test]
    fn enable_tail_call() {
        let mut features = Features::new();
        features.tail_call(true);
        assert!(features.tail_call);
    }

    #[test]
    fn enable_module_linking() {
        let mut features = Features::new();
        features.module_linking(true);
        assert!(features.module_linking);
    }

    #[test]
    fn enable_multi_memory() {
        let mut features = Features::new();
        features.multi_memory(true);
        assert!(features.multi_memory);
    }

    #[test]
    fn enable_memory64() {
        let mut features = Features::new();
        features.memory64(true);
        assert!(features.memory64);
    }
}
