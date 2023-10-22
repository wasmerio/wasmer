# WASM Snapshot Functionality

Wasmer now supports snapshots of the current running process into a journal
log file which allows for the resumption from an earlier point in time.

# Triggers

Various triggers are possible that will cause a snapshot to be taken at
a specific point in time, these are:

## On Idle

Triggered when all the threads in the process goes idle.

## On Listen

Triggered when a listen syscall is invoked on a socket.
    
## On Stdin

Triggered when the process reads stdin for the first time

## On Timer

Triggered periodically based on a timer (default 10 seconds) which can be specified using the `snapshot-timer` option

## On Sigint (Ctrl+C)

Issued if the user sends an interrupt signal (Ctrl + C).

## On Sigalrm

Alarm clock signal (used for timers)
(see `man alarm`)

## On Sigtstp

The SIGTSTP signal is sent to a process by its controlling terminal to request it to stop temporarily. It is commonly initiated by the user pressing Ctrl-Z.

# On Sigstop

The SIGSTOP signal instructs the operating system to stop a process for later resumption

# On Non Deterministic Call

When a non-determinstic call is made from WASM

# Limitations

- The WASM process must have had the `asyncify` post processing step applied to the binary.
- Taking a snapshot can consume large amounts of memory while its processing.
- Snapshots are not instant and have overhead when generating.
- The layout of the memory must be known by the runtime in order to take snapshots.

# Design

On startup if the restore snapshot file is specified then the runtime will restore the
state of the WASM process by reading and processing the log entries in the snapshot
journal. This restoration will bring the memory and the thread stacks back to a previous
point in time and then resume all the threads.

When a trigger occurs a new snapshot will be taken of the WASM process which will
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
