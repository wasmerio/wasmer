# Wasmer deprecated packages

## Deprecation notice: please read

Thanks to users feedback, collected experience and various use cases,
Wasmer has decided to entirely improve its API to offer the best user
experience and the best features to as many users as possible.

The new version of Wasmer (`1.0.0-alpha.1`) includes many improvements
in terms of performance or the memory consumption, in addition to a ton
of new features and much better flexibility!
You can check revamped new API in the [`wasmer`] crate.

In order to help our existing users to enjoy the performance boost and
memory improvements without updating their program that much, we have
created a new version of the `wasmer-runtime` crate, which is now
*an adaptation* of the new API but with the old API syntax, as much as
possible. Indeed, it was not always possible to provide the exact same
API, but changes are subtle.

We have carefully documented most of the differences in [the
`runtime-core/CHANGES.md` document][changes].

It is important to understand the public of this port. We do not
recommend to advanced users of Wasmer to use this port. Advanced API,
like `ModuleInfo` or the `vm` module (incl. `vm::Ctx`) have not been
fully ported because it was very internals to Wasmer. For advanced
users, we highly recommend to migrate to the new version of Wasmer,
which is awesome by the way (completely neutral opinion). The public
for this port is beginners or regular users that do not necesarily
have time to update their code immediately but that want to enjoy a
performance boost and memory improvements.

[`wasmer`]: https://crates.io/crates/wasmer/
[changes]: ./runtime-core/CHANGES.md
