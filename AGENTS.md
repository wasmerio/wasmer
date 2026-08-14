# Working in this repo as an agent

Before your first edit, read `docs/CONTRIBUTING.md` and `docs/BUILD.md` in
full, plus the row below covering whatever you are about to touch. This
file is an index and does not repeat what it links to. Skipping one of
those docs does not exempt you from what it says.

`AGENTS.md` contains only guidance specific to agents and not owned by one
topic. Put guidance that applies to humans and agents in the relevant
general doc:

| Read                   | For                                                     |
| ---------------------- | ------------------------------------------------------- |
| `docs/CONTRIBUTING.md` | PR expectations, commit format, lint gates, code style  |
| `docs/ARCHITECTURE.md` | the crate map, one place per concern, submodules        |
| `docs/BUILD.md`        | toolchain setup, compiler backends, fast iteration      |
| `docs/TEST.md`         | test suites, WAST spec tests, `tests/ignores.txt`       |
| `docs/DEBUGGING.md`    | logging, inspecting Wasm, authoring and reducing repros |
| `docs/SECURITY.md`     | supported versions, how to report vulnerabilities       |
| `docs/journal.md`      | snapshot and restore internals                          |
| `docs/PACKAGING.md`    | distro packaging constraints                            |
| `docs/RISCV.md`        | state of RISC-V support                                 |
| `docs/dev/release.md`  | the automated release process                           |

Do not restate those rules here. Add a `## For agents` section to the
owning doc only when the agent workflow genuinely differs from the human
workflow.

Trust the repository more than any doc. `Makefile` targets and `--help`
output beat external docs, and docs.wasmer.io lags this repository.

This repo is about changing the runtime, not using it. For use of the
`wasmer` CLI as a product and for app deployment, the `wasmer-cli-usage`
and `wasmer-edge` skills live in the docs.wasmer.io repository under
`.agents/skills/`.

## Before you write code

Every new test, fixture, syscall implementation, or backend arm has a
sibling in this repo that already does the same kind of thing. Find it and
match its location, naming, and structure. If you cannot find one, say so
before inventing a layout. Inspect the [architecture](./docs/ARCHITECTURE.md).

Before implementing a new mechanism, state the design in one or two
sentences and get agreement. Prioritise the smallest diff that slots into
existing machinery. Work already done is not an argument for keeping a
shape: if the design is wrong, say so rather than defending it.

## The ones that get violated most

These are in the docs, and are repeated here only because they are the
ones most often missed:

- **Workspace features are a trap.** `cargo build --workspace --features
<backend>` silently produces a headless binary that cannot compile Wasm
  (`docs/BUILD.md`). Use `-p wasmer-cli` or the Makefile, and read the
  `Enabled Compilers:` banner every time.
- **Do not commit drift.** All make targets run `--locked`; a drifted
  `Cargo.lock` fails every target. `git status` often shows submodules as
  modified — a pointer bump never rides along in an unrelated PR
  (`docs/ARCHITECTURE.md`).
- **Do not hand-edit generated files.** `CHANGELOG.md` and crate versions
  are produced by the release process (`docs/dev/release.md`).
- **Skip lists need reasons.** A backend- or platform-specific failure
  goes in `tests/ignores.txt` with a reason, and skipping is the user's
  call, never yours (`docs/TEST.md`).

## Security embargo

Do not fix a security vulnerability in this public repository. Embargoed
work goes through the private process in `docs/SECURITY.md`. A public fix
before disclosure exposes users.

## Personal execution preferences

The gitignored `AGENTS.override.md` is the user's personal addendum. When
it exists, it overrides this file's defaults.

## Working with the user

- A question is a request for an answer. Do not edit code in response to
  "why is this like that".
- Once a task is authorized, run it to completion. Stop only for a real
  blocker: a failed prerequisite, a genuine ambiguity, or a decision that
  is the user's to make.
- Read a file immediately before overwriting it. The user hand-edits files
  between your turns, and a stale copy in context will clobber their work.
- Never revert, discard, or undo a change unless asked.
- Do not state a tool or build-system behaviour as fact without checking
  it. If it is unverified, say so.
- Do not attribute intent to existing code ("deliberately",
  "intentionally") without a comment or commit backing it. If the reason
  is unknown, say that.
- Use the user's exact term for a concept. A backend is not an engine;
  `wasmer-headless` is not a build failure.

## Never act in the user's name

No `git push`. No opening or closing PRs or issues. No GitHub comments of
any kind, including replies to review threads, unless the user explicitly
asks you to. No commits unless the user gave you permission.

Writing the code is not authorization to ship it. "Fix this" and "go
ahead" authorize the edit, never the commit or the push. Do not offer a
push or a PR as the next step. When public text is needed, draft it in
chat or hand over the `gh` command.

## Before you report done

- `make lint` run, and `cargo test` run in the crates you touched — just
  now, not earlier in the session. No "should work".
- No edits to `CHANGELOG.md`, crate versions, or submodule pointers.
- Platform code touched? Say that the PR needs the `macos` or `musl`
  label (`docs/TEST.md`).
- Nothing pushed, posted, or committed unless asked for in this message.
