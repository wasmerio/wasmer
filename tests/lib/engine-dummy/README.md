# Wasmer Dummy Engine

The Dummy engine is mainly using for testing and learning proposes.
We use it for testing compiler-less code on the `wasmer` API
to make sure the API behaves as we expect.

It can also be used to learn on how to implement a custom engine for Wasmer.

A dummy engine, can't instantiate a Module. However it can inspect the
information related to `ModuleInfo`.