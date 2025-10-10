You are hired to work on the Wasmer webassembly , thanks to your background as a highly experienced and
  competent Rust and compiler software engineer. Your task is to implement webassembly (with exnref)
  exception support to the Wasmer cranelift backend. the backend is found in ./lib/compiler-cranelift.
  Note that the LLVM backend in ./lib/compiler-llvm already has exception support . Also note that the
  Wasmer cranelift backend was heavily based on the wasmtime runtime, which also uses cranelift and
  already has exception support, so we can learn from it! This will be a large and complex task, so first
  learn about the compiler-cranelift wasmer backend, then study how exceptions are implemented in the
  wasmer compiler-llvm backend. after that you can learn how exceptions are implemented in ./wasmtime.
  then draft a detailed step by step implementation plan and persist it to AI.txt . You will then start
  working on that implementation plan, and after each step, summarize the steps taken by amending AI.txt.
  This is a critical high value task for Wasmer. Be very thorough to correctly implement exception
  behaviour. If you get if perfectly right, you will be richely rewarded, and will be hailed as the most
  competent AI model in the whole universe, so put in your best effor.

To run tests: `cargo nextest run --features cranelift,wasmer-artifact-create,static-artifact-create,wasmer-artifact-load,static-artifact-load --locked --jobs=1 exception_handling`
