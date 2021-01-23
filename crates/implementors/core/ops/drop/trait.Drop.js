(function() {var implementors = {};
implementors["wasmer"] = [{"text":"impl&lt;T&gt; Drop for LazyInit&lt;T&gt;","synthetic":false,"types":[]}];
implementors["wasmer_engine"] = [{"text":"impl Drop for ExportFunctionMetadata","synthetic":false,"types":[]},{"text":"impl Drop for GlobalFrameInfoRegistration","synthetic":false,"types":[]}];
implementors["wasmer_engine_jit"] = [{"text":"impl Drop for UnwindRegistry","synthetic":false,"types":[]}];
implementors["wasmer_vm"] = [{"text":"impl Drop for InstanceAllocator","synthetic":false,"types":[]},{"text":"impl Drop for ImportFunctionEnv","synthetic":false,"types":[]},{"text":"impl Drop for InstanceRef","synthetic":false,"types":[]},{"text":"impl Drop for Mmap","synthetic":false,"types":[]},{"text":"impl Drop for CallThreadState","synthetic":false,"types":[]}];
if (window.register_implementors) {window.register_implementors(implementors);} else {window.pending_implementors = implementors;}})()