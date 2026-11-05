# Browser filesystem providers

`wasixv2-vfs-web` is an engine-independent driver. It builds on native Rust and
`wasm32-unknown-unknown`; it does not require JSPI, a foreign-memory service, or
a particular worker executor.

`memory(clock, mode, uid, gid).await` creates a shared directory backed by
`wasixv2-vfs-mem`. Keep its `Arc<WebFs>` in the embedder and mount that same
filesystem into guest VFS trees with `Vfs::mount`. Files and directory identity
outlive a guest process. The embedder can edit files using another `Vfs::new`
over the same filesystem, with an explicit `Ctx` (credentials, root and umask),
or use the machine's VFS directly. Path and permission checks stay in VFS.

`LocalProvider::mount(filesystem, capabilities)` also accepts an existing overlay
or package driver. Build an overlay from a memory upper and package lowers for
copy-on-write storage. Supply the capabilities the backing driver implements;
the adapter never emulates unsupported atomic append, hard links or rename
modes. Once wrapped, all edits of the backing filesystem must go through the
provider. Do not retain a second mutable view of an overlay's layers.

## JS provider contract

Implement `protocol::Provider` with shared request/completion queues. Construct
`WebFs::new(provider, info, capabilities).await` before mounting it. The driver
acquires its root asynchronously; VFS sees a ready root. Worker bootstrap,
message delivery, JS callback registration and SDK bindings belong to the
embedding layer. This crate supplies the owned request/reply protocol and the
filesystem behavior on either side of that transport.

Each `Request` variant in [protocol.rs](src/protocol.rs) is one operation, with
its matching `Reply` variant:

| Request | Reply |
| --- | --- |
| Root, Lookup, Create | Node |
| CreateOpen, CreateUnnamed | Created (node and handle) |
| Open | Open (handle) |
| GetAttr, SetAttr | Attr |
| StatFs | StatFs |
| Read, ReadLink, GetXattr | Bytes |
| ListXattr | Names |
| Write | Count |
| Append | Appended (start offset and byte count) |
| SeekHole | Offset |
| ReadDir | Entries |
| Other successful operations | Done |

Errors are explicit Linux errno values. A transport failure completes every
pending call with an error; it must not silently retry a mutation or reconnect
an existing object into a different session. Request sequence/generation and
completion-buffer ownership are transport responsibilities.

- Names, symlink targets and data are bytes (`Uint8Array` in JS), never UTF-8
  paths. The driver rejects invalid components before submitting them. A name
  never contains `/`, NUL, `.` or `..` as a component. VFS interprets symlinks
  within the guest root, including `openat2` containment flags.
- Represent every Rust `u64`/`i64` exactly in JS, for example as `bigint` or
  two 32-bit words. Do not round inode IDs, cookies, offsets or timestamps through
  a JS `number`. `usize` request lengths are bounded by this driver.
- Every Node/Open/Created reply transfers fresh resource leases. A node token
  is distinct from its stable `(ino, generation, file_type)` identity. Different
  hard links and lookups return the same identity. Rename/unlink cannot invalidate
  an open node or handle. Tokens are never reused within a provider session.
- `release(Resource)` must enqueue nonblocking cleanup from any runtime worker.
  The provider event loop drops the rooted JS reference after all in-flight uses
  complete. Release of an open handle precedes the driver's final inode release.
  Cancelled requests clean up undelivered result leases; cancellation cannot
  recycle a reply buffer while the provider can still write it.
- Append is one atomic end-of-file write. Rename is one atomic namespace change;
  do not implement it with copy/remove. Providers serialize namespace checks and
  mutations even when several VFS clients access the directory. `LocalProvider`
  supplies that serialization for the reused memory/overlay drivers.
- Read/write/append requests hold at most `MAX_IO` (64 KiB) data bytes. Larger
  calls return a short count. ReadDir requests/replies hold at most `READDIR_BATCH`
  (64) entries. Cookies are opaque nonzero continuation values no greater than
  VFS's `MAX_DIR_COOKIE`; they survive entry insertion/removal and do not include
  `.` or `..`. Empty Entries means end of directory. A full consumer can reject
  an entry and later resume with its last accepted cookie.
- Names always revalidate because an embedder may change them. For every ACL
  change, publish the shared `acl_version` invalidation sequence before completing
  the mutation. Providers store metadata and implicit timestamps as required by
  the existing Node/Handle traits; VFS checks permissions and supplies explicit
  ownership/mode changes. Provider callbacks cannot reenter the VFS operation
  waiting for them while its async inode locks remain held.
- Readonly filesystem metadata is enforced both by VFS and the driver. Missing
  optional capabilities return `EOPNOTSUPP` (`EINVAL` for unsupported rename flags).
  File attributes, export handles, host shared pages, quota administration and
  readiness notifications are not advertised by this adapter. Its regular files
  use VFS's ordinary always-ready/copy-based behavior.

File System Access and OPFS are optional future implementations of this provider
contract. Neither is selected implicitly or presented as a native Linux inode
filesystem. An OPFS implementation needs its own explicit feature and capability
set; no persistence is promised by the memory provider.

Linux semantic authorities are the existing VFS/driver contracts, in particular
`fs/namei.c:{link_path_walk,may_create_dentry,lock_rename,vfs_rename}`,
`fs/open.c:do_sys_openat2`, `mm/filemap.c:generic_file_write_iter`,
`fs/readdir.c:iterate_dir`, and `fs/file_table.c:__fput` / `fs/inode.c:iput`.

The crate tests run the same protocol against real memfs/overlay and independent
VFS clients. They cover shared edits and retained unlinked handles, permissions,
readonly errors, ACL invalidation, openat2 containment, resumable cookies, atomic
append, sparse I/O and resource/error validation. Actual JS event-loop/worker and
guest-process browser integration is supplied by the browser executor/harness.
