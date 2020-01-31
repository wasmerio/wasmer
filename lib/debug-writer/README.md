# Wasmer debug info writer

This crate deals with passing DWARF debug information along from
compiled Wasm modules to the machine code that we generate.

This crate is effectively a derivative work of WasmTime's
[`wasmtime-debug`](https://github.com/bytecodealliance/wasmtime/tree/master/crates/debug)
crate. After beginning work on a clean reimplementation we realized
that the WasmTime implementation is high quality and it didn't make
sense for us to duplicate their hard work.

Additionally by keeping the code structure of `wasmer-debug-writer`
similar to `wasmtime-debug`, we hope to upstream bug fixes and
improvements to `wasmtime-debug`.

Copied files include the copyright notice as well, but as a catch all,
this crate is a derivative work of WasmTime's `wasmtime-debug`

```
Copyright 2020 WasmTime Project Developers

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```

The latest revision at the time of cloning is `3992b8669f9b9e185abe81e9998ce2ff4d40ff68`.

Changes to this crate are copyright of Wasmer inc. unless otherwise indicated
and are licensed under the Wasmer project's license:

```
MIT License

Copyright (c) 2020 Wasmer, Inc. and its affiliates.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

```
