searchState.loadedDescShard("wasmer_c_api", 0, "Wasmer C API.\nUtilities to read errors.\nUtilities to set up tracing and logging.\nImplementation of the official WebAssembly C API for …\nRust function to register a new error.\nGets the length in bytes of the last error if any, zero …\nGets the last error message if any into the provided buffer\nAn engine drives the compilation and the runtime.\ncbindgen:ignore\nA WebAssembly instance is a stateful, executable instance …\nA WebAssembly module contains stateless WebAssembly code …\nA store represents all global state that can be …\nA trap represents an error which stores trace message with …\ncbindgen:ignore\nThis module contains <em>unstable non-standard</em> C API.\nPossible runtime values that a WebAssembly module can …\nWasmer-specific API to get or query the version of this …\nUnofficial API for WASI integrating with the standard Wasm …\nWasmer-specific API to transform the WAT format into Wasm …\nVariant to represent the Cranelift compiler. See the …\nVariant to represent the LLVM compiler. See the […\nVariant to represent the Singlepass compiler. See the […\nVariant to represent the Universal engine. See the […\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nDelete a Wasmer config object.\nCreate a new default Wasmer configuration.\nUpdates the configuration to specify a particular compiler …\nUpdates the configuration to specify a particular engine …\nA configuration holds the compiler and the engine used by …\nDeletes an engine.\nCreates a new Universal engine with the default compiler.\nCreates an engine with a particular configuration.\nAn engine is used by the store to drive the compilation …\nKind of compilers that can be used by the engines.\nKind of engines that can be used by the store.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCopy a <code>wasm_extern_t</code>.\nDelete an extern.\nPerforms a deep copy of a vector of [<code>wasm_extern_t *</code>].\nDeletes a vector of [<code>wasm_extern_t *</code>].\nCreates a new vector of [<code>wasm_extern_t *</code>].\nCreates an empty vector of [<code>wasm_extern_t *</code>].\nCreates a new uninitialized vector of [<code>wasm_extern_t *</code>].\nRepresents a vector of <code>wasm_extern_t *</code>.\nNote: This function returns nothing by design but it can …\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nDeletes an instance.\nGets the exports of the instance.\nCreates a new instance from a WebAssembly module and a set …\nOpaque type representing a WebAssembly instance.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nDeletes a WebAssembly module.\nDeserializes a serialized module binary into a …\nReturns an array of the exported types in the module.\nReturns an array of the imported types in the module.\nA WebAssembly module contains stateless WebAssembly code …\nSerializes a module into a binary representation that the …\nOpaque type representing a WebAssembly module.\nValidates a new WebAssembly module given the configuration …\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nDeletes a WebAssembly store.\nCreates a new WebAssembly store given a specific engine.\nOpaque type representing a WebAssembly store.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nDeletes a trap.\nGets the message attached to the trap.\nCreate a new trap message.\nGets the origin frame attached to the trap.\nGets the trace (as a list of frames) attached to the trap.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nPerforms a deep copy of a vector of <code>wasm_byte_t</code>.\nDeletes a vector of <code>wasm_byte_t</code>.\nCreates a new vector of <code>wasm_byte_t</code>.\nCreates an empty vector of <code>wasm_byte_t</code>.\nCreates a new uninitialized vector of <code>wasm_byte_t</code>.\nRepresents a vector of <code>wasm_byte_t</code>.\nPerforms a deep copy of a vector of [<code>wasm_exporttype_t *</code>].\nDeletes a vector of [<code>wasm_exporttype_t *</code>].\nCreates a new vector of [<code>wasm_exporttype_t *</code>].\nCreates an empty vector of [<code>wasm_exporttype_t *</code>].\nCreates a new uninitialized vector of [<code>wasm_exporttype_t *</code>]…\nRepresents a vector of <code>wasm_exporttype_t *</code>.\nPerforms a deep copy of a vector of [<code>wasm_frame_t *</code>].\nDeletes a vector of [<code>wasm_frame_t *</code>].\nCreates a new vector of [<code>wasm_frame_t *</code>].\nCreates an empty vector of [<code>wasm_frame_t *</code>].\nCreates a new uninitialized vector of [<code>wasm_frame_t *</code>].\nRepresents a vector of <code>wasm_frame_t *</code>.\nPerforms a deep copy of a vector of [<code>wasm_functype_t *</code>].\nDeletes a vector of [<code>wasm_functype_t *</code>].\nCreates a new vector of [<code>wasm_functype_t *</code>].\nCreates an empty vector of [<code>wasm_functype_t *</code>].\nCreates a new uninitialized vector of [<code>wasm_functype_t *</code>].\nRepresents a vector of <code>wasm_functype_t *</code>.\nPerforms a deep copy of a vector of [<code>wasm_globaltype_t *</code>].\nDeletes a vector of [<code>wasm_globaltype_t *</code>].\nCreates a new vector of [<code>wasm_globaltype_t *</code>].\nCreates an empty vector of [<code>wasm_globaltype_t *</code>].\nCreates a new uninitialized vector of [<code>wasm_globaltype_t *</code>]…\nRepresents a vector of <code>wasm_globaltype_t *</code>.\nPerforms a deep copy of a vector of [<code>wasm_importtype_t *</code>].\nDeletes a vector of [<code>wasm_importtype_t *</code>].\nCreates a new vector of [<code>wasm_importtype_t *</code>].\nCreates an empty vector of [<code>wasm_importtype_t *</code>].\nCreates a new uninitialized vector of [<code>wasm_importtype_t *</code>]…\nRepresents a vector of <code>wasm_importtype_t *</code>.\nPerforms a deep copy of a vector of [<code>wasm_memorytype_t *</code>].\nDeletes a vector of [<code>wasm_memorytype_t *</code>].\nCreates a new vector of [<code>wasm_memorytype_t *</code>].\nCreates an empty vector of [<code>wasm_memorytype_t *</code>].\nCreates a new uninitialized vector of [<code>wasm_memorytype_t *</code>]…\nRepresents a vector of <code>wasm_memorytype_t *</code>.\nPerforms a deep copy of a vector of [<code>wasm_tabletype_t *</code>].\nDeletes a vector of [<code>wasm_tabletype_t *</code>].\nCreates a new vector of [<code>wasm_tabletype_t *</code>].\nCreates an empty vector of [<code>wasm_tabletype_t *</code>].\nCreates a new uninitialized vector of [<code>wasm_tabletype_t *</code>].\nRepresents a vector of <code>wasm_tabletype_t *</code>.\nPerforms a deep copy of a vector of [<code>wasm_valtype_t *</code>].\nDeletes a vector of [<code>wasm_valtype_t *</code>].\nCreates a new vector of [<code>wasm_valtype_t *</code>].\nCreates an empty vector of [<code>wasm_valtype_t *</code>].\nCreates a new uninitialized vector of [<code>wasm_valtype_t *</code>].\nRepresents a vector of <code>wasm_valtype_t *</code>.\nUnstable non-standard Wasmer-specific types for the …\nUnstable non-standard Wasmer-specific API that contains a …\nUnstable non-standard Wasmer-specific types to manipulate …\nUnstable non-standard Wasmer-specific extensions to the …\nUnstable non-standard Wasmer-specific types about …\nUnstable non-standard Wasmer-specific API that contains …\nUnstable non-standard Wasmer-specific API that contains …\nUpdates the configuration to enable NaN canonicalization.\nUnstable non-standard Wasmer-specific API to update the …\nUnstable non-standard Wasmer-specific API to update the …\nCheck whether the given compiler is available, i.e. part …\nCheck whether the given engine is available, i.e. part of …\nCheck whether there is no compiler available in this …\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nConfigures whether the WebAssembly bulk memory operations …\nDelete a <code>wasmer_features_t</code>.\nConfigures whether the WebAssembly 64-bit memory proposal …\nConfigures whether the WebAssembly tail-call proposal will …\nConfigures whether the WebAssembly multi-memory proposal …\nConfigures whether the WebAssembly multi-value proposal …\nCreates a new <code>wasmer_features_t</code>.\nConfigures whether the WebAssembly reference types …\nConfigures whether the WebAssembly SIMD proposal will be …\nControls which experimental features will be enabled. …\nConfigures whether the WebAssembly tail-call proposal will …\nConfigures whether the WebAssembly threads proposal will …\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nUnstable non-standard Wasmer-specific API that contains …\nUpdates the configuration to add a module middleware.\nOpaque representing any kind of middleware.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nTransforms a <code>wasmer_metering_t</code> into a generic …\nFunction type to represent a user-defined cost function …\nDeletes a <code>wasmer_metering_t</code>.\nReturns the remaining metering points. <code>u64::MAX</code> means …\nCreates a new metering middleware with an initial limit, …\nReturns true if the remaning points are exhausted, false …\nSet a new amount of points for the given metering …\nOpaque type representing a metering middleware.\nUnstable non-standard Wasmer-specific API to get the module…\nA WebAssembly module contains stateless WebAssembly code …\nUnstable non-standard Wasmer-specific API to set the module…\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nAdd a new CPU feature into the set represented by …\nDelete a <code>wasmer_cpu_features_t</code>.\nCreate a new <code>wasmer_cpu_features_t</code>.\nUnstable non-standard Wasmer-specific API to represent a …\nDelete a <code>wasmer_target_t</code>.\nCreates a new <code>wasmer_target_t</code>.\nUnstable non-standard Wasmer-specific API to represent a …\nDelete a <code>wasmer_triple_t</code>.\nCreate a new <code>wasmer_triple_t</code> based on a triple string.\nCreate the <code>wasmer_triple_t</code> for the current host.\nUnstable non-standard Wasmer-specific API to represent a …\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nNon-standard function to get the imports needed for the …\nNon-standard function to get the module name of a …\nNon-standard function to get the name of a …\nUnstable non-standard type wrapping <code>wasm_extern_t</code> with the …\nNon-standard function to get the wrapped extern of a …\nPerforms a deep copy of a vector of […\nDeletes a vector of [<code>wasmer_named_extern_t *</code>].\nCreates a new vector of [<code>wasmer_named_extern_t *</code>].\nCreates an empty vector of [<code>wasmer_named_extern_t *</code>].\nCreates a new uninitialized vector of […\nRepresents a vector of <code>wasmer_named_extern_t *</code>.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nThe kind of the value.\nThe real value.\nA Rust union, compatible with C, that holds a value of kind\nA WebAssembly value composed of its type and its value.\nPerforms a deep copy of a vector of <code>wasm_val_t</code>.\nDeletes a vector of <code>wasm_val_t</code>.\nCreates a new vector of <code>wasm_val_t</code>.\nCreates an empty vector of <code>wasm_val_t</code>.\nCreates a new uninitialized vector of <code>wasm_val_t</code>.\nRepresents a vector of <code>wasm_val_t</code>.\nRepresents the kind of values. The variants of this C enum …\nGet the version of the Wasmer C API.\nGet the major version of the Wasmer C API.\nGet the minor version of the Wasmer C API.\nGet the patch version of the Wasmer C API.\nGet the minor version of the Wasmer C API.\nAn invalid version.\nLatest version.\n<code>wasi_unstable</code>.\n<code>wasi_snapshot_preview1</code>.\n<code>wasix_32v1</code>.\n<code>wasix_64v1</code>.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nDelete a <code>wasi_env_t</code>.\nCreate a new WASI environment.\nSet the memory on a <code>wasi_env_t</code>.\nNon-standard function to get the imports needed for the …\nThe version of WASI. This is determined by the imports …\nParses in-memory bytes as either the WAT format, or a …")