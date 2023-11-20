# WASM Journal Functionality

Wasmer now supports journals for the state of a WASM process. This gives the ability
to persist changes made to the temporary file system and to save and store snapshots
of the running process.

The journal file is a linear history of events that occurred when the process was
running that if replayed will bring the process made to a discrete and deterministic
state.

Journal files can be concatenated, compacted and filtered to change the discrete state.

These journals are maintained in a consistent and durable way thus ensuring that
failures of the system while the process is running does not corrupt the journal.

# Snapshot Triggers

The journal will record state changes to the sandbox built around the WASM process as
it runs however it may be important to certain use-cases to take explicit snapshot
restoration points in the journal at key points that make sense.

When a snapshot is triggered all the running threads of the process are paused and
the state of the WASM memory and thread stacks are recorded into the journal so that
they can be restored.

In order to use the snapshot functionality the WASM process must be compiled with
the `asyncify` modifications, this can be done using the `wasm-opt` tool.

Note: If a process does not have the `asyncify` modifications you can still use
the journal functionality for recording the file system and WASM memory state
however the stacks of the threads will be omitted meaning a restoration will
restart the main thread.

Various triggers are possible that will cause a snapshot to be taken at a specific
point in time, these are as follows:

## On Idle

Triggered when all the threads in the process go into an idle state. This trigger
is useful to take snapshots at convenient moments without causing unnecessary overhead.

For processes that have TTY/STDIN input this is particularly useful.

## On FirstListen

Triggered when a listen syscall is invoked on a socket. This can be an important
milestone to take a snapshot when one wants to speed up the boot time of a WASM process
up to the moment where it is ready to accept requests.
    
## On FirstStdin

Triggered when the process reads stdin for the first time. This can be useful to
speed up the boot time of a WASM process.

## On FirstEnviron

Triggered when the process reads an environment variable for the first time. This can
be useful to speed up the boot time of a CGI WASM process which reads the environment
variables to parse the request that it must execute.

## On Timer Interval

Triggered periodically based on a timer (default 10 seconds) which can be specified
using the `journal-interval` option. This can be useful for asynchronous replication
of a WASM process from one machine to another with a particular lag latency.

## On Sigint (Ctrl+C)

Issued if the user sends an interrupt signal (Ctrl + C).

## On Sigalrm

Alarm clock signal (used for timers)
(see `man alarm`)

## On Sigtstp

The SIGTSTP signal is sent to a process by its controlling terminal to request it to stop
temporarily. It is commonly initiated by the user pressing Ctrl-Z.

# On Sigstop

The SIGSTOP signal instructs the operating system to stop a process for later resumption

# On Non Deterministic Call

When a non-deterministic call is made from WASM process to the outside world (i.e. it reaches
out of the sandbox)

# Limitations

- The WASM process that wish to record the state of the threads must have had the `asyncify`
  post processing step applied to the binary (see `wasm-opt`).
- Taking a snapshot can consume large amounts of memory while its processing.
- Snapshots are not instant and have overhead when generating.
- The layout of the memory must be known by the runtime in order to take snapshots.

# Design

On startup if the restore journal file is specified then the runtime will restore the
state of the WASM process by reading and processing the log entries in the snapshot
journal. This restoration will bring the memory and the thread stacks back to a previous
point in time and then resume all the threads.

When a trigger occurs a new journal will be taken of the WASM process which will
take the following steps:

1. Pause all threads
2. Capture the stack of each thread
3. Write the thread state to the journal
4. Write the memory (excluding stacks) to the journal
5. Resume execution.

The implementation is currently able to save and restore the following:

- WASM Memory
- Stack memory
- Call stack
- Open sockets
- Open files
- Terminal text

# Journal Capturer Implementations

## Log File Journal

Writes the log events to a linear log file on the local file system
as they are received by the trait. Log files can be concatenated
together to make larger log files.

## Unsupported Journal

The default implementation does not support snapshots and will error
out if an attempt is made to send it events. Using the unsupported
capturer as a restoration point will restore nothing but will not
error out.

## Compacting Journal

Deduplicates memory and stacks to reduce the number of volume of
log events sent to its inner capturer. Compacting the events occurs
in line as the events are generated

## Filtered Journal

Filters out a specific set of log events and drops the rest, this
capturer can be useful for restoring to a previous call point but
retaining the memory changes (e.g. WCGI runner).
