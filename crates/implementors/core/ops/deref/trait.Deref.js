(function() {var implementors = {};
implementors["wasmer_c_api"] = [{"text":"impl Deref for GLOBAL_STORE","synthetic":false,"types":[]},{"text":"impl Deref for VERSION_MAJOR","synthetic":false,"types":[]},{"text":"impl Deref for VERSION_MINOR","synthetic":false,"types":[]},{"text":"impl Deref for VERSION_PATCH","synthetic":false,"types":[]}];
implementors["wasmer_emscripten"] = [{"text":"impl Deref for LibcDirWrapper","synthetic":false,"types":[]},{"text":"impl Deref for OLD_ABORT_ON_CANNOT_GROW_MEMORY_SIG","synthetic":false,"types":[]}];
implementors["wasmer_engine"] = [{"text":"impl Deref for FRAME_INFO","synthetic":false,"types":[]}];
implementors["wasmer_vm"] = [{"text":"impl Deref for FunctionBodyPtr","synthetic":false,"types":[]},{"text":"impl Deref for SectionBodyPtr","synthetic":false,"types":[]}];
if (window.register_implementors) {window.register_implementors(implementors);} else {window.pending_implementors = implementors;}})()