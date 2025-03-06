use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};

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

    /// Check if SIMD is enabled
    pub fn has_simd(&self) -> bool {
        self.simd
    }

    /// Check if threads are enabled
    pub fn has_threads(&self) -> bool {
        self.threads
    }

    /// Check if reference types are enabled
    pub fn has_reference_types(&self) -> bool {
        self.reference_types
    }

    /// Check if multi-value is enabled
    pub fn has_multi_value(&self) -> bool {
        self.multi_value
    }

    /// Check if bulk memory operations are enabled
    pub fn has_bulk_memory(&self) -> bool {
        self.bulk_memory
    }

    /// Check if exceptions are enabled
    pub fn has_exceptions(&self) -> bool {
        self.exceptions
    }

    /// Check if tail call is enabled
    pub fn has_tail_call(&self) -> bool {
        self.tail_call
    }

    /// Check if module linking is enabled
    pub fn has_module_linking(&self) -> bool {
        self.module_linking
    }

    /// Check if multi memory is enabled
    pub fn has_multi_memory(&self) -> bool {
        self.multi_memory
    }

    /// Check if memory64 is enabled
    pub fn has_memory64(&self) -> bool {
        self.memory64
    }

    /// Check if relaxed SIMD is enabled
    pub fn has_relaxed_simd(&self) -> bool {
        self.relaxed_simd
    }

    /// Check if extended const is enabled
    pub fn has_extended_const(&self) -> bool {
        self.extended_const
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
}

impl Default for Features {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "std")]
impl Features {
    /// Detect required WebAssembly features from a module binary
    pub fn detect_from_wasm(wasm_bytes: &[u8]) -> Option<Self> {
        use wasmparser::{Parser, Payload};

        // Check for Wasm magic bytes
        if wasm_bytes.len() < 4 || &wasm_bytes[0..4] != b"\0asm" {
            return None;
        }

        // Start with default features
        let mut features = Self::default();

        // Simple pass to detect features from module structure and instructions
        let parser = Parser::new(0);

        for payload_result in parser.parse_all(wasm_bytes) {
            if let Ok(payload) = payload_result {
                match payload {
                    // Look for SIMD operations in code
                    Payload::CodeSectionEntry(body) => {
                        if let Ok(operators) = body.get_operators_reader() {
                            for op in operators {
                                if let Ok(op) = op {
                                    let op_string = format!("{:?}", op);

                                    // SIMD instructions will contain V128
                                    if op_string.contains("V128") {
                                        features.simd(true);
                                    }

                                    // Bulk memory operations
                                    if op_string.contains("MemoryCopy")
                                        || op_string.contains("MemoryFill")
                                        || op_string.contains("TableCopy")
                                    {
                                        features.bulk_memory(true);
                                    }

                                    // Reference types
                                    if op_string.contains("RefNull")
                                        || op_string.contains("RefFunc")
                                    {
                                        features.reference_types(true);
                                    }

                                    // Exception handling
                                    if op_string.contains("Try")
                                        || op_string.contains("Catch")
                                        || op_string.contains("Throw")
                                    {
                                        features.exceptions(true);
                                    }

                                    // Tail call
                                    if op_string.contains("ReturnCall") {
                                        features.tail_call(true);
                                    }
                                }
                            }
                        }
                    }

                    // Check for shared memories (threads)
                    Payload::MemorySection(memories) => {
                        for memory in memories {
                            if let Ok(memory) = memory {
                                if memory.shared {
                                    features.threads(true);
                                }
                                // Check for memory64
                                if memory.memory64 {
                                    features.memory64(true);
                                }
                            }
                        }
                    }

                    // Check for multi-value returns in function signatures
                    Payload::TypeSection(types) => {
                        for type_entry in types {
                            if let Ok(typ) = type_entry {
                                // Use the debug representation to check for multi-results
                                let type_str = format!("{:?}", typ);
                                if type_str.contains("results: [") && type_str.contains(",") {
                                    features.multi_value(true);
                                }
                            }
                        }
                    }

                    // Tag section indicates exception handling
                    Payload::TagSection(_) => {
                        features.exceptions(true);
                    }

                    // Multi-memory check
                    Payload::ImportSection(imports) => {
                        let mut memory_count = 0;
                        for import in imports {
                            if let Ok(import) = import {
                                // Use string comparison as a workaround for TypeRef comparison
                                let type_str = format!("{:?}", import.ty);
                                if type_str.contains("Memory") {
                                    memory_count += 1;
                                    if memory_count > 1 {
                                        features.multi_memory(true);
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    // Custom sections might have hints about features
                    Payload::CustomSection(section) => {
                        let name = section.name();
                        if name.contains("exception") {
                            features.exceptions(true);
                        }
                    }

                    _ => {}
                }
            }
        }

        Some(features)
    }

    /// Convert features to list of feature names for WebC annotations
    pub fn to_feature_names(&self) -> Vec<String> {
        let mut feature_names = Vec::new();

        if self.has_simd() {
            feature_names.push("simd".to_string());
        }

        if self.has_threads() {
            feature_names.push("threads".to_string());
        }

        if self.has_reference_types() {
            feature_names.push("reference-types".to_string());
        }

        if self.has_multi_value() {
            feature_names.push("multi-value".to_string());
        }

        if self.has_bulk_memory() {
            feature_names.push("bulk-memory".to_string());
        }

        if self.has_exceptions() {
            feature_names.push("exception-handling".to_string());
        }

        // Other features that might be relevant for WebC
        if self.has_tail_call() {
            feature_names.push("tail-call".to_string());
        }

        if self.has_multi_memory() {
            feature_names.push("multi-memory".to_string());
        }

        if self.has_memory64() {
            feature_names.push("memory64".to_string());
        }

        feature_names
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
