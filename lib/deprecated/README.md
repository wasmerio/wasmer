# Wasmer deprecated packages

## Important Note; Please Read

Thanks to users' feedbacks, collected experience and various usecases,
Wasmer has decided to entirely changed its API to offer the best user
experience and the best features to as many users as possible, just
before the 1.0 release. This new version of Wasmer includes many
improvements in terms of performance or the memory consumption, in
addition to a ton of new features and much better flexibility!

In order to help our existing users to enjoy the performance boost and
memory improvements without updating their program that much, we have
created a new version of the `wasmer-runtime-core` crate, which is now
*a port* of the new API but with the old API, as much as
possible. Indeed, it was not always possible to provide the exact same
API, but changes are subtle.

We have carefully documented most of the differences in [the
`runtime-core/CHANGES.md` document](./runtime-core/CHANGES.md).

It is important to understand the public of this port. We do not
recommend to advanced users of Wasmer to use this port. Advanced API,
like `ModuleInfo` or the `vm` module (incl. `vm::Ctx`) have not been
fully ported because it was very internals to Wasmer. For advanced
users, we highly recommend to migrate to the new version of Wasmer,
which is awesome by the way (completely neutral opinion). The public
for this port is beginners or regular users that do not necesarily
have time to update their code immediately but that want to enjoy a
performance boost and memory improvements.
