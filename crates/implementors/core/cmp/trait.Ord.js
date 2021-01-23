(function() {var implementors = {};
implementors["test_generator"] = [{"text":"impl Ord for Test","synthetic":false,"types":[]}];
implementors["wasmer_compiler"] = [{"text":"impl Ord for JumpTable","synthetic":false,"types":[]},{"text":"impl Ord for SectionIndex","synthetic":false,"types":[]}];
implementors["wasmer_compiler_llvm"] = [{"text":"impl Ord for ElfSectionIndex","synthetic":false,"types":[]}];
implementors["wasmer_compiler_singlepass"] = [{"text":"impl Ord for Location","synthetic":false,"types":[]},{"text":"impl Ord for Size","synthetic":false,"types":[]},{"text":"impl Ord for GPR","synthetic":false,"types":[]},{"text":"impl Ord for XMM","synthetic":false,"types":[]}];
implementors["wasmer_engine"] = [{"text":"impl Ord for EngineId","synthetic":false,"types":[]}];
if (window.register_implementors) {window.register_implementors(implementors);} else {window.pending_implementors = implementors;}})()