
Below is a **detailed, junior-implementable spec for Phase 2.1 (“Canonical path normalization and resolution”)**, tailored to the **current `path_walker.rs` you pasted**. It focuses on **required additions/changes**, not re-explaining what’s already correct.


If details in related types have slightly diverged, prefer to keep the changes.

---

## Step 2.1: Canonical Path Normalization and Resolution (Mount-aware)

### Goal of 2.1

Deliver a single, correct, mount-aware path resolver that:

1. Resolves `.` and `..` with Linux/POSIX semantics (including mount root `..` behavior).
2. Preserves/handles trailing slash semantics (`"file/"` must fail with `NotDir`).
3. Implements correct symlink traversal rules (intermediate vs final component; NOFOLLOW; depth limit).
4. Implements `AT_FDCWD` and base-dir semantics (relative vs absolute).
5. Produces **stable outputs for downstream VFS ops** (Phase 2.2–2.5, Phase 3 mounts, Phase 5 Wasix integration):

   * return the resolved node/inode/mount
   * return parent+name when requested
   * optionally return parent info for “final” resolution to avoid re-walking later.

### Non-goals of 2.1

* No hostfs confinement implementation (Phase 6), but 2.1 **must** provide the hooks (`resolve_beneath`, `in_root`) so hostfs/wasix can use them safely.
* No overlay semantics (Phase 3.4).
* No rights/permissions gating (Phase 2.5 / Phase 5).

---

## Current file status: what’s already good

Your current implementation already covers most of the “classic” walker loop:

* Component iteration via `WorkQueue`
* `.` ignored
* `..` using a stack plus `try_mount_parent` mount boundary logic
* symlink following with depth count + `readlink()` splicing into queue
* mount entry via `enter_if_mountpoint` after lookup
* trailing slash enforcement at end
* `resolve_parent()` output format (`ResolvedParent { dir, name, had_trailing_slash }`)

So step 2.1 work is mostly:

1. **fix correctness gaps**, 2) **implement the currently-rejected flags**, 3) **tighten edge cases + test suite**, 4) **fill output fields that later steps rely on**.

---

## Deliverables

### Code deliverable

* Update `vfs/core/src/path_walker.rs` to fully meet the 2.1 semantics:

  * implement `resolve_beneath` and `in_root`
  * fix compile errors/bugs in `start_node` and any other issues
  * make `Resolved.parent` meaningful for `resolve()` (recommended; see below)
  * define and enforce the remaining edge-case rules explicitly

### Test deliverable

* Add a focused `vfs-core` test module (either `vfs/core/tests/path_walker.rs` or `#[cfg(test)] mod tests` inside the file) that asserts:

  * symlink NOFOLLOW behavior
  * symlink depth behavior
  * trailing slash behavior
  * `..` across mounts
  * empty path behavior (AT_EMPTY_PATH equivalent)
  * `resolve_beneath` / `in_root` escape prevention

---

## Required semantics and how to implement them

### 1) Fix: `start_node()` is currently wrong / won’t compile

You currently have:

```rust
fn start_node(&self, _inner: &MountTableInnerRef, req: &ResolutionRequest<'_>) -> VfsResult<(NodeRef, Option<NodeRef>)> {
    if req.path.is_absolute() {
        let root = self.root_node(inner)?; // inner is not in scope
        ...
    }
```

**Required change**

* Use the `inner` parameter, and don’t name it `_inner` if used.
* Decide how absolute paths behave when `in_root` is enabled (details below).

**Acceptance**

* `cargo check -p vfs-core` passes.

---

### 2) Implement `WalkFlags.in_root` (RESOLVE_IN_ROOT-like)

#### Meaning

When `in_root = true`, treat the **base directory as the “root” for this resolution**:

* Absolute path input (`/a/b`) does **not** jump to the global VFS root; it resolves **as if** root were the base directory. (This matches the behavior described for `RESOLVE_IN_ROOT`.) ([LWN.net][1])
* Absolute symlink targets encountered during traversal also resolve relative to that same base-root, not global root.

#### Implementation details

Add to `ResolutionRequest` handling:

* Compute a “root anchor” node at the start of `resolve_internal`:

  * If `in_root == false`: `root_anchor = global root mount root` (current behavior)
  * If `in_root == true`: `root_anchor = start node` (base) **but normalized to a directory boundary**

    * For `VfsBaseDir::Handle(dir)`: root_anchor is that dir node
    * For `VfsBaseDir::Cwd`: root_anchor is cwd node

Then modify root transitions:

* `WorkComponent::RootDir`:

  * today: sets current to global root
  * required: sets current to `root_anchor`
  * also clear the `stack` (but see beneath handling below)

* After `inject_symlink(target)`:

  * today: if absolute head, jump to global root
  * required: if absolute head, jump to `root_anchor`

#### Edge cases

* If base is not a directory and the operation requires directory traversal, treat that like normal (NotDir when trying to walk into it).
* If `in_root` is true and `path.is_absolute() == true`, you should still “pop_root” from queue, but the initial current should be the `root_anchor` not global root.

---

### 3) Implement `WalkFlags.resolve_beneath` (RESOLVE_BENEATH-like)

#### Meaning

When `resolve_beneath = true`, disallow resolving any path that escapes the base directory tree, including:

* Any attempt to escape via `..` above the base
* Absolute input paths (`/x`) should be rejected (because they imply a “root jump” not beneath dirfd) ([man7.org][2])
* Absolute symlink targets should be rejected (same reason) ([man7.org][2])

Linux openat2 reports `EXDEV` when an escape is detected for `RESOLVE_IN_ROOT`/`RESOLVE_BENEATH`. ([Arch Manual Pages][3])
(There are also `EAGAIN` cases in Linux when it cannot prove safety under races. ([Ubuntu Manpages][4]) In this VFS, we don’t have kernel races in the same way, so **do not implement EAGAIN** unless you later add a “concurrent rename escape” model; for now: deterministic EXDEV on escape.)

#### Implementation strategy (fits your current stack model)

You already maintain a `stack: SmallVec<[NodeRef; 8]>` that tracks parents.

To enforce “beneath”, you need a **base boundary marker**:

* When starting resolution with `resolve_beneath=true`, capture:

  * `beneath_root: NodeRef` = the start node (“dirfd”/base)
  * `beneath_mount: MountId` and `beneath_inode: VfsInodeId`

Then enforce these rules:

1. **Absolute input path**:

   * If `resolve_beneath == true` and `req.path.is_absolute() == true`: return `VfsErrorKind::CrossDevice` (or whichever kind maps to `EXDEV` in your errno mapping layer later).

2. **Encountering `RootDir` component**:

   * If `resolve_beneath == true`: return `EXDEV` immediately (because that’s an attempted root jump).

3. **Symlink target injection**:

   * If `resolve_beneath == true` and symlink target is absolute:

     * return `EXDEV` (reject).

4. **Handling `..`**:

   * If `resolve_beneath == false`: your existing behavior is fine.
   * If `resolve_beneath == true`:

     * Permit `..` only if the resulting node is still “at or below” beneath_root.
     * With your stack model:

       * If you’re at the base boundary, you must **not** pop past it.
       * So: maintain `stack` such that it never pops below the base boundary.

         * Simplest: at initialization, push a sentinel “parent” that represents the base boundary and never pop it.
         * Cleaner: keep an integer `min_stack_len` which is the stack length representing the base boundary, and refuse to pop below it.

           * Example:

             * When resolution starts: `min_stack_len = stack.len()` (after pushing base_parent if you do)
             * Also treat “current == beneath_root and stack.len() == min_stack_len” as the boundary case.
       * If a `..` would cross the boundary: return `EXDEV`.

5. **Mount boundary behavior with beneath**

   * `..` at mount root currently uses `try_mount_parent()` which jumps to parent mount’s mountpoint.
   * Under `resolve_beneath`, this is only allowed if it does not escape the beneath_root boundary.
   * That means:

     * If beneath_root is inside a mount, traversing to parent mount may be an escape.
     * Apply the same boundary check: if the resolved parent would be above boundary → `EXDEV`.

#### Required error kind

* Add or reuse an error kind that maps to `EXDEV` in `vfs/unix` later.

  * You already reference `VfsErrorKind::CrossDevice` elsewhere in plan; if present, use it.

---

### 4) Implement `WalkFlags.allow_empty_path` (AT_EMPTY_PATH-like)

#### Meaning

If `path.is_empty()` and `allow_empty_path == true`, resolve to the base node itself (like Linux’s “empty path” mode in various `*at` syscalls). This is used heavily by Wasix “fstatat with empty path” equivalents.

#### Required behavior

* If `path.is_empty()`:

  * If `allow_empty_path == false`: keep current behavior (InvalidInput)
  * If `allow_empty_path == true`:

    * If `mode == Parent`: error (InvalidInput) — there is no parent/name to return.
    * Else resolve to the start node, and apply `must_be_dir` check if requested.

#### Why this matters for later steps

* Phase 5 “at-style behavior is not optional”: Wasix needs empty-path semantics for fd-relative syscalls without converting to strings.

---

### 5) Tighten symlink rules and make them explicit in code

Your current rules are close; step 2.1 requires making them *fully specified*:

#### Required rules

1. **Intermediate symlink** (not final component)

* If `follow_symlinks == true`: follow it
* If `follow_symlinks == false`: error `NotDir` (because you can’t traverse through a non-followed symlink)

2. **Final symlink**

* If `follow_final_symlink == true`: follow it
* Else: treat symlink itself as the final node (allow returning symlink metadata).

3. **Depth limit**

* Enforce `max_symlinks` exactly; once exceeded → `TooManySymlinks` (maps to `ELOOP` later)
* Count only when you actually follow.

4. **Trailing slash interaction**

* If the path had a trailing slash, the final resolved node must be a directory, **even if it was reached via symlink following**.

#### Required small fix

Right now you do:

```rust
if follow {
  traversal.symlinks_followed += 1;
  if traversal.symlinks_followed > req.flags.max_symlinks { ... }
}
```

This effectively allows exactly `max_symlinks` follows, then fails on `max_symlinks + 1`. That’s fine—just document it.

---

### 6) Populate `Resolved.parent` for `resolve()` (recommended for later phases)

You currently always return:

```rust
Resolved { ..., parent: None, ... }
```

**Required change (strongly recommended, because Phase 2.3/2.4 will otherwise re-walk):**

* When resolving in `ResolveMode::Final`, return parent info if available:

  * For any non-root successful resolution, fill:

    * `parent.dir` = resolved node of parent directory
    * `parent.name` = final component name
    * `parent.had_trailing_slash` = original trailing slash
* For root resolution, `parent = None`.

**How**

* Track `last_parent: Option<NodeRef>` and `last_name: Option<VfsNameBuf>` during the loop:

  * When you are about to set `current = child_ref` for a `Normal(name)` component, set `last_parent = Some(current.clone())`, `last_name = Some(name_buf)`
* At end, if final outcome, convert those to `ResolvedParent` and store in `Resolved.parent`.

**Why it matters**

* Later operations like unlink/rename/chmod on a resolved node often need parent dir + name; this avoids a second traversal.

---

### 7) Define what `resolve_at_component_boundary()` must mean

Right now it’s effectively “resolve like normal, but if internal returns Parent, return the dir”.

That’s probably okay, but step 2.1 should **lock in** a definition so future callers use it correctly:

**Definition**

* Resolves the path as far as possible without returning a “dangling name”. Concretely:

  * If the request is resolvable to a concrete node: return it
  * If the request asked for Parent-mode, return the parent directory (not the child name)

**No extra behavior in 2.1**

* Do **not** add “stop before symlink” or “stop before mount” semantics unless you need them immediately; those are easy to get wrong and better introduced with explicit syscalls that demand them.

---

## Performance requirements (within 2.1 scope)

Keep resolution overhead low; the path walker will be a hot path.

### Required changes

1. **Avoid extra allocations on normal paths**

* Today `WorkQueue::from_path` allocates `Vec<u8>` for every `Normal` component.
* Improve to store borrowed references when possible, and only allocate when injecting symlinks.
* Suggested representation:

  * `enum WorkComponent<'a> { RootDir, CurDir, ParentDir, NormalBorrowed(&'a [u8]), NormalOwned(SmallVec<[u8; 24]>) }`
  * `WorkQueue` becomes generic over lifetime (or store `Cow<[u8]>`)
* If that’s too much churn right now: keep current code but add a TODO and at least ensure symlink targets use a small inline buffer (SmallVec) to reduce heap churn.

2. **Minimize mount-table lookups**

* You already do a single check per component after lookup (good).
* Keep the mount snapshot (`inner`) outside the loop (already done).

---

## Tests: required suite for 2.1

All tests should use `vfs-mem` once it’s capable enough; until then, you can implement a tiny fake `FsNode` for path tests, but the plan expects memfs soon.

### A) Basic normalization

* `a/./b` resolves to same as `a/b`
* `a//b` resolves to same as `a/b` (depending on how `VfsPath::components()` behaves; assert intended behavior based on your parser)

### B) Trailing slash

* `open/stat` semantics aren’t implemented here, but resolution must enforce:

  * resolving `file/` returns `NotDir`
  * resolving `dir/` succeeds

### C) Symlink rules

1. Intermediate symlink follow:

* `a -> dir` and resolving `a/b` works when `follow_symlinks=true`
* same path fails with `follow_symlinks=false` with `NotDir`

2. Final symlink nofollow:

* `x -> file`
* resolving `x` with `follow_final_symlink=false` returns node type symlink (no readlink traversal)

3. Symlink depth:

* chain length = `max_symlinks` succeeds
* chain length = `max_symlinks + 1` fails with `TooManySymlinks`

### D) Mount crossing with `..`

Set up:

* root memfs mount at `/`
* second mount at `/mnt`
* inside mounted fs root, resolve `..` → must land at `/mnt` in parent mount

### E) allow_empty_path

* `allow_empty_path=true`, path empty:

  * resolves to base node (cwd or handle)
* `allow_empty_path=false`, path empty:

  * InvalidInput

### F) in_root

Set base = `/sandbox`:

* `in_root=true`, path `/etc` resolves as `<base>/etc` (within sandbox), not global `/etc`
* absolute symlink should also respect in_root anchor

### G) resolve_beneath

Set base = `/sandbox/sub`:

* resolving `../x` fails with `CrossDevice`/EXDEV
* resolving `/x` fails with `CrossDevice`/EXDEV
* resolving symlink with absolute target fails with `CrossDevice`/EXDEV

(For the EXDEV mapping expectation, rely on your later `vfs/unix` mapping; for now assert the `VfsErrorKind`.)

---

## Concrete implementation checklist (what to change in this file)

1. **Fix compilation**

* `start_node()` must accept `inner: &MountTableInnerRef` and use it.
* Ensure any mismatched names (`_inner` vs `inner`) are corrected.

2. **Add root anchor**

* Compute `root_anchor: NodeRef` inside `resolve_internal`:

  * if `in_root`: start node
  * else: global root
* Route both `RootDir` component handling and absolute symlink handling through `root_anchor`.

3. **Add beneath boundary tracking**

* If `resolve_beneath`:

  * reject absolute input paths immediately
  * reject `RootDir` component immediately
  * reject absolute symlink targets immediately
  * enforce `..` boundary with either `min_stack_len` or a sentinel boundary node

4. **Implement allow_empty_path**

* If empty path and allowed:

  * return current resolved (or boundary behavior for Parent mode)

5. **Populate `Resolved.parent`**

* Track last parent+name and fill on success.

6. **Add tests**

* Put them in `vfs/core/tests/path_walker.rs` so they behave like black-box tests and will remain stable as internals change.

---

## Acceptance criteria for Step 2.1

* `cargo check -p vfs-core` passes.
* `cargo test -p vfs-core path_walker` (or full `cargo test -p vfs-core`) passes with the required test suite.
* `PathWalker` supports:

  * `.` `..` including mount-root `..`
  * trailing slash enforcement
  * symlink follow + nofollow + depth limit
  * empty path when enabled
  * `in_root` and `resolve_beneath` with deterministic `EXDEV`-class failure on escape ([Arch Manual Pages][3])
* Wasix integration work (Phase 5) will be able to call PathWalker for all path-based syscalls without re-implementing traversal logic.

---

If you want, I can also provide a **patch-style set of edits** (exact code changes) for `path_walker.rs` that implements the checklist above, but I kept this response as a spec you can hand directly to a junior engineer.

[1]: https://lwn.net/Articles/796868/?utm_source=chatgpt.com "Restricting path name lookup with openat2 () - LWN.net"
[2]: https://www.man7.org/linux/man-pages/man2/openat2.2.html?utm_source=chatgpt.com "openat2 (2) - Linux manual page - man7.org"
[3]: https://man.archlinux.org/man/openat2.2.en?utm_source=chatgpt.com "openat2 (2) — Arch manual pages"
[4]: https://manpages.ubuntu.com/manpages/noble/en/man2/openat2.2.html?utm_source=chatgpt.com "Ubuntu Manpage: openat2 - open and possibly create a file (extended)"
