 # napi/v8

 `napi/v8` is a standalone compatibility layer that implements Node-API (N-API)
 on top of V8 while keeping V8 details internal.

 ## Scope

 - Public surface: N-API headers and ABI (`js_native_api.h`, `node_api.h`).
 - Internal implementation: V8-backed code only in `src/`.
 - Test strategy: port portable tests from `node/test/js-native-api` first.

 ## Porting Policy

 - Source and tests should be ported from upstream Node as fully as possible.
 - Keep upstream files/logic verbatim unless adaptation is strictly required.
 - The only intended code adaptation rule: replace direct V8 API usage with
   equivalent N-API usage.
 - Favor harness/environment shims over rewriting upstream test content.

 ## Layout

- `../include/`: shared public C headers (engine-agnostic surface)
- `../tests/`: shared canonical Node-API fixtures
- `src/`: V8-backed implementation and environment glue
- `tests/`: V8-specific test assets and compatibility test docs

 ## Current Phase

 This directory implements the initial `napi/v8` scaffold, a core runtime slice,
and a first portable test subset (`2_function_arguments`, `3_callbacks`) using
GoogleTest.

See `tests/README.md` for build/run instructions and
`tests/PORTABILITY_MATRIX.md` for the current portability classification.
