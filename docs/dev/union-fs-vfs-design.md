# VFS-Inspired Union FS Design

## Goal

Design a `MountFileSystem` that owns mount topology the way a VFS does, instead
of delegating nested mount behavior into arbitrary mounted filesystems.

This is intended to fix two classes of problems:

- shared-prefix merge bugs when combining multiple package mount trees
- nested mounts under non-mountable leaf filesystems such as
  `WebcVolumeFileSystem`

## Architectural Decisions

These points should be treated as target constraints for the refactor:

- no `mount()` method on the base `FileSystem` trait
- no public `mount()` method on `TmpFileSystem`; all mount management should go
  through the union/VFS layer
- the dedicated mount-topology owner should be named `MountFileSystem`
- the root WASIX filesystem in `wasmer-wasix` should always be this
  union/VFS-style filesystem

## Current Behavior

Today, `UnionFileSystem` uses mounted filesystems themselves to represent
internal path structure.

Relevant implementation:

- `lib/virtual-fs/src/union_fs.rs`
  - `mount()`
  - `find_mount()`
  - `read_dir()`
  - `open()`

Current mounting behavior:

- mounting `/opt/assets` may create a synthetic nested `UnionFileSystem` under
  `opt`
- if a mount already exists at `opt`, `UnionFileSystem::mount()` delegates the
  remainder of the path into that mounted filesystem

That means nested mounts depend on the mounted leaf filesystem supporting
`mount()`.

Example failure:

- mount package A at `/opt` using `WebcVolumeFileSystem`
- mount package B at `/opt/assets`
- second mount attempts to recurse into `/opt` leaf
- `WebcVolumeFileSystem::mount()` returns `FsError::Unsupported`
- nested mount cannot be represented

Current open behavior:

- `new_open_options()` is only a builder
- actual path routing happens in `FileOpener::open()`
- `UnionFileSystem::open("/opt/assets/xxx/yyy")`:
  - finds the mount for the parent path
  - peels off only the first path segment at each union layer
  - delegates the remaining suffix into the mounted filesystem

This routing is direct path forwarding, not repeated `read_dir()` traversal.

## Design Principle

Separate:

- filesystem contents
- mount topology

Leaf filesystems such as:

- `WebcVolumeFileSystem`
- `mem_fs::FileSystem`
- `host_fs::FileSystem`
- `static_fs`

should only implement operations for their own trees.

The union/VFS layer should own:

- which filesystem is mounted at which path
- how path resolution crosses mount boundaries
- how directory listings expose submounts

This is closer to Linux VFS behavior, with one deliberate difference:

- Linux generally replaces the covered directory at a mountpoint
- this union layer still wants merged directory listings where child mounts are
  visible as entries in the parent

## Proposed Data Model

Replace the current flat map of path component to mounted filesystem with a real
mount tree.

Sketch:

```rust
struct MountNode {
    name: Option<String>,
    mount: Option<Arc<dyn FileSystem + Send + Sync>>,
    children: DashMap<OsString, MountNode>,
}

struct MountFileSystem {
    root: MountNode,
}
```

Notes:

- `mount` is the filesystem mounted exactly at this node path
- `children` are deeper submount points
- a node may have both:
  - `mount = Some(...)`
  - non-empty `children`

That is required for paths like:

- `/opt`
- `/opt/assets`

to coexist

## Core Resolution Model

The union layer should resolve a path by walking the mount tree and tracking the
deepest mounted node encountered.

Example for `/opt/assets/xxx/yyy`:

1. visit root
2. visit child `opt`
3. if `opt.mount` exists, remember it as current best mount
4. visit child `assets`
5. if `assets.mount` exists, replace current best mount
6. continue while children exist
7. delegate to the deepest mounted node with the remaining path suffix

Expected result:

- if mounts exist at both `/opt` and `/opt/assets`, opening
  `/opt/assets/xxx/yyy` delegates to the `/opt/assets` filesystem with suffix
  `/xxx/yyy`
- no leaf filesystem is ever asked to accept a nested mount

## Suggested Helper API

Add a path resolution helper inside `MountFileSystem`.

Sketch:

```rust
struct ResolvedMount {
    delegated_fs: Arc<dyn FileSystem + Send + Sync>,
    delegated_path: PathBuf,
    matched_components: usize,
}

fn resolve_deepest_mount(&self, path: &Path) -> Option<ResolvedMount>;
```

Responsibilities:

- normalize the union-visible path
- walk path components through the mount tree
- remember the deepest mounted node
- return the delegated filesystem plus the suffix path to use within it

This helper should become the basis for:

- `open()`
- `metadata()`
- `symlink_metadata()`
- `readlink()`
- `remove_file()`
- `create_dir()`
- `remove_dir()`
- `rename()`

## Directory Listing Semantics

Directory listing is the main operation that needs merging behavior.

For `read_dir("/opt")`:

1. locate the node for `/opt`
2. if `node.mount` exists, call `node.mount.read_dir("/")`
3. collect synthetic child mount names from `node.children`
4. merge entries
5. if both base fs and child mounts provide the same entry name, child mount
   wins

This gives:

- normal entries from the mounted filesystem at `/opt`
- visible submounts such as `assets`

The returned entry paths must still be rewritten to union-visible paths.

## Metadata Semantics

We need explicit rules for paths that are:

- pure branch nodes with children but no exact mount
- exact mount nodes
- names covered by both a base mounted filesystem and a child submount

Recommended rules:

- branch-only nodes return synthetic directory metadata
- exact mount nodes return metadata from the mounted filesystem root if
  available, otherwise synthetic directory metadata
- child submount path wins over same-named entry exposed by the parent mounted
  filesystem

Example:

- `/opt` mounted to fs A
- `/opt/assets` mounted to fs B
- fs A also has a directory named `assets`

Then:

- `metadata("/opt/assets")` should resolve to fs B root
- `read_dir("/opt")` should show one `assets` entry corresponding to the
  submount

## Expected Mounted-Node Topology

After the refactor, the root WASIX filesystem should be a `MountFileSystem`.

Important distinction:

- `MountFileSystem` owns namespace topology and mount-point resolution
- `OverlayFileSystem` owns mutability, precedence, and whiteout behavior for a
  mounted node

This means the common runtime shape should be:

- root = `MountFileSystem`
- `/` mount = writable root filesystem, typically:
  - `TmpFileSystem`, or
  - `OverlayFileSystem<TmpFileSystem, ...>`
- each package-defined WEBC mount point should usually be mounted as:
  - `OverlayFileSystem<TmpFileSystem, [WebcVolumeFileSystem]>`

Why:

- mounting a raw `WebcVolumeFileSystem` directly at `/app` or `/opt/assets`
  would make that whole subtree read-only
- current WASIX behavior often relies on writes under package-defined mount
  paths being possible
- using an overlay at the mount point preserves that behavior:
  - writes go to the writable upper tmpfs
  - reads fall through to the readonly WEBC volume

Example target topology:

- `MountFileSystem`
- `/` -> `TmpFileSystem`
- `/app` -> `OverlayFileSystem<TmpFileSystem, [WebcVolumeFileSystem]>`
- `/opt/assets` -> `OverlayFileSystem<TmpFileSystem, [WebcVolumeFileSystem]>`

Nested mount precedence still comes from `MountFileSystem`:

- `/opt/assets/...` resolves to the deeper `/opt/assets` mounted filesystem
- the parent `/opt` mount does not own nested mount management

This should be policy-driven rather than forced for every mount:

- package/WEBC mounts should likely default to overlay-backed mounts to preserve
  existing writability expectations
- explicitly readonly mounts should still be able to mount a raw leaf
  filesystem directly when desired

## Open Semantics

`open()` should not perform directory listing.

Recommended flow:

1. normalize path
2. resolve the deepest mount for the full file path or its parent, depending on
   operation needs
3. delegate once to the selected leaf filesystem using the suffix path

This matches the current spirit of direct routing, but with correct deepest
mount selection.

## Rename And Mutation Semantics

These need to be decided before implementation is complete.

Recommended starting rules:

- operations on branch-only nodes:
  - `metadata()` succeeds with synthetic directory metadata
  - `read_dir()` succeeds
  - `create_dir()` may succeed as a no-op if the branch already exists
  - destructive mutation on synthetic branch nodes should likely be rejected
- cross-mount `rename()` should fail
- mutation under an exact mount node should delegate to the mounted filesystem,
  unless a deeper submount intercepts the path

These rules can be tightened after the first implementation pass.

## Merge Semantics

With a mount tree, merging should be structural.

For two union filesystems:

- merge nodes recursively by path component
- merge child maps recursively
- resolve conflicts at `mount` slots according to `UnionMergeMode`

Important open question:

- for shared prefixes where both sides have intermediate nodes and deeper
  children, should `Replace` and `Fail` apply only to exact `mount` slot
  conflicts, or to any overlapping subtree?

Recommended initial interpretation:

- overlap in branch structure alone is not a conflict
- only conflicting exact `mount` slots should trigger `Replace`, `Skip`, or
  `Fail`

That interpretation best fits the original bug report and nested mount use case.

## Impact Scope

This should mostly be localized to the current
`lib/virtual-fs/src/union_fs.rs` implementation that would become
`MountFileSystem`.

Leaf filesystem implementations should remain largely unchanged.

Expected touched areas:

- `lib/virtual-fs/src/union_fs.rs`
- union-fs tests in the same file
- possibly a few callers or tests that depend on current mount collision
  behavior

What should not need broad changes:

- `WebcVolumeFileSystem`
- `host_fs`
- `static_fs`
- most of `mem_fs`

The public `FileSystem` trait can stay unchanged in the first pass.

However, the intended end state is broader:

- `FileSystem` no longer exposes `mount()`
- `TmpFileSystem` no longer exposes its own mount API
- mounting happens only through the dedicated union/VFS filesystem type
- `wasmer-wasix` always uses that filesystem type at the root

## Implementation Sequence

### Phase 1: Introduce Tree Representation

1. add `MountNode`
2. move the current `UnionFileSystem` storage from flat
   `DashMap<PathBuf, MountPoint>` to
   root node plus children
3. preserve public API shape where possible

### Phase 2: Implement Resolution Helper

1. add `resolve_deepest_mount()`
2. add helper to locate an exact node by path for `read_dir()` and branch-node
   metadata
3. write focused tests for resolution behavior before switching all operations

### Phase 3: Rewire Read Operations

1. rewrite `open()`
2. rewrite `metadata()` and `symlink_metadata()`
3. rewrite `readlink()`
4. rewrite `read_dir()` with child-mount merging

### Phase 4: Rewire Mutation Operations

1. rewrite `create_dir()`
2. rewrite `remove_dir()`
3. rewrite `remove_file()`
4. rewrite `rename()`

### Phase 5: Rework Merge

1. rewrite `merge()` as structural tree merge
2. remove RTTI / downcast checks
3. add tests for `Skip`, `Replace`, and `Fail`

## Tests To Add First

Add these before or during the implementation:

- open file through exact mount:
  - `/app/index.php`
- open file through nested submount:
  - mount `/opt`
  - mount `/opt/assets`
  - open `/opt/assets/css/site.css`
- `read_dir("/")` with multiple package roots
- `read_dir("/opt")` where:
  - base mount contributes entries
  - child submount contributes `assets`
- metadata precedence for same-named child mount vs parent fs entry
- merge of two union filesystems with shared prefixes and nested mounts

## Concrete First Coding Step

Start with an internal-only refactor in `union_fs.rs`:

1. define `MountNode`
2. implement path insertion for mounts
3. implement `resolve_deepest_mount()`
4. add tests that exercise resolution without yet rewriting every operation

That gives a stable foundation before touching `read_dir()` and `merge()`.

## Non-Goals For The First Pass

- changing leaf filesystem implementations to be VFS-aware
- reproducing Linux mount semantics exactly

The immediate goal is a cleaner mount-topology owner inside
`MountFileSystem`,
not a repo-wide filesystem API redesign.

Note:

- although removing `mount()` from `FileSystem` is not required for the very
  first internal refactor, it is part of the intended target architecture and
  should be planned shortly after the new union/VFS layer is stable
