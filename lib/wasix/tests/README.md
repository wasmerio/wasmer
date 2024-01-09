# WASIX integration tests

## default file system tree

We should see these four directories by default

```sh
cd ../../cli
cargo run --features compiler,cranelift -- ../wasix/tests/coreutils.wasm ls
```

Expected:

```
bin
dev
etc
tmp
```

## using /dev/stderr

This test ensures that the dev character devices are working properly, there should be two lines with blah as tee will
send it both to the console and to the file

```sh
cd ../../cli
echo blah | cargo run --features compiler,cranelift -- ../wasix/tests/coreutils.wasm tee /dev/stderr
```

Expected:

```
blah
blah
```

## atomic_wait and atomic_wake syscalls

When we convert this from syscalls to native language constructs in WASM this test
needs to continue to pass.

```sh
cd ../../cli
cargo run --features compiler,cranelift,debug -- ../wasix/tests/example-condvar.wasm
```

Expected:

```
condvar1 thread spawn
condvar1 thread started
condvar1 thread sleep(1sec) start
condvar loop
condvar wait
condvar1 thread sleep(1sec) end
condvar1 thread set condition
condvar1 thread notify
condvar woken
condvar parent done
condvar1 thread exit
all done
```

## cowsay

Piping to cowsay should, well.... display a cow that says something

```sh
cd ../../cli
echo blah | cargo run --features compiler,cranelift,debug -- ../wasix/tests/cowsay.wasm
```

Expected:

```
 ______
< blah >
 ------
        \   ^__^
         \  (oo)\_______
            (__)\       )\/\
               ||----w |
                ||     ||
```

## polling and event notifications

This test makes sure the event notifications works correctly `fd_event` - this construct is used
in `tokio` in order to wake up the main IO thread that is blocked on an `poll_oneoff`.

```sh
cd ../../cli
cargo run --features compiler,cranelift,debug -- ../wasix/tests/example-epoll.wasm
```

Expected:

```
EFD_NONBLOCK:4
success write to efd, write 8 bytes(4) at 1669077621s 935291us
success read from efd, read 8 bytes(4) at 1669077621s 937666us
success write to efd, write 8 bytes(4) at 1669077622s 937881us
success read from efd, read 8 bytes(4) at 1669077622s 938309us
success write to efd, write 8 bytes(4) at 1669077623s 939714us
success read from efd, read 8 bytes(4) at 1669077623s 940002us
success write to efd, write 8 bytes(4) at 1669077624s 941033us
success read from efd, read 8 bytes(4) at 1669077624s 941205us
success write to efd, write 8 bytes(4) at 1669077625s 943658us
success read from efd, read 8 bytes(4) at 1669077625s 943956us
```

## fork and execve

The ability to fork the current process and run a different image but retain the existing open
file handles (which is needed for stdin and stdout redirection)

```sh
cd ../../cli
cargo run --features compiler,cranelift,debug -- --use sharrattj/coreutils --enable-threads ../wasix/tests/example
-execve.wasm
```

Expected:

```
Main program started
execve: echo hi-from-child
hi-from-child
Child(1) exited with 0
execve: echo hi-from-parent
hi-from-parent
```

## longjmp

longjmp is used by C programs that save and restore the stack at specific points - this functionality
is often used for exception handling

```sh
cd ../../cli
cargo run --features compiler,cranelift,debug -- --enable-threads ../wasix/tests/example-longjmp.wasm
```

Expected:

```
(A1)
(B1)
(A2) r=10001
(B2) r=20001
(A3) r=10002
(B3) r=20002
(A4) r=10003
```

## Yet another longjmp implemenation

This one is initiated from `rust` code and thus has the risk of leaking memory but uses different interfaces

```sh
cd ../../cli
cargo run --features compiler,cranelift,debug -- --enable-threads ../wasix/tests/example-stack.wasm
```

Expected:

```
before long jump
after long jump [val=10]
before long jump
after long jump [val=20]
```

## fork

Simple fork example that is a crude multi-threading implementation - used by `dash`

```sh
cd ../../cli
cargo run --features compiler,cranelift,debug -- --enable-threads ../wasix/tests/example-fork.wasm
```

Expected:

```
Parent has x = 0
Child has x = 2
Child(1) exited with 0
```

## fork and longjmp

Performs a longjmp of a stack that was recorded before the fork - this test ensures that the stacks that have
been recorded are preserved after a fork. The behavior is needed for `dash`

```sh
cd ../../cli
cargo run --features compiler,cranelift,debug -- --enable-threads ../wasix/tests/example-fork-longjmp.wasm
```

Expected:

```
Parent has x = 0
Child has x = 2
Child(1) exited with 5
```

### multi threading

full multi-threading with shared memory and shared compiled modules

```sh
cd ../../cli
cargo run --features compiler,cranelift,debug -- --enable-threads ../wasix/tests/example-multi-threading.wasm
```

Expected:

```
thread 1 started
thread 2 started
thread 3 started
thread 4 started
thread 5 started
thread 6 started
thread 7 started
thread 8 started
thread 9 started
waiting for threads
thread 1 finished
thread 2 finished
thread 3 finished
thread 4 finished
thread 5 finished
thread 6 finished
thread 7 finished
thread 8 finished
thread 9 finished
all done
```

## pipes

Uses the `fd_pipe` syscall to create a bidirection pipe with two file descriptors then forks
the process to write and read to this pipe.

```sh
cd ../../cli
cargo run --features compiler,cranelift,debug -- --enable-threads ../wasix/tests/example-pipe.wasm
```

Expected:

```
this text should be printed by the child
this text should be printed by the parent
```

## signals

Tests that signals can be received and processed by WASM applications

```sh
cargo run --features compiler,cranelift,debug -- --enable-threads ../wasix/tests/example-signal.wasm
```

Note: This test requires that a signal is sent to the process asynchronously

```sh
kill -s SIGINT 16967
```

Expected:

```
received SIGHUP

```

## sleep

Puts the process to sleep for 50ms

```sh
cd ../../cli
cargo run --features compiler,cranelift,debug -- --enable-threads ../wasix/tests/example-sleep.wasm
```

Expected:

```
```

## Spawning sub-processes

Uses `posix_spawn` to launch a sub-process and wait on it to exit

```sh
cd ../../cli
cargo run --features compiler,cranelift,debug -- --enable-threads --use sharrattj/coreutils ../wasix/tests/example
-spawn.wasm
```

Expected:

```
Child pid: 1
hi
Child status 0
```

## TCP client

Connects to 8.8.8.8:53 over TCP to verify TCP clients work

```sh
cd ../../cli
cargo run --features compiler,cranelift,debug -- --enable-threads ../wasix/tests/example-tcp-client.wasm
```

Expected:

```
Successfully connected to server in port 53
Finished.
```

## TCP listener

Waits for a connection after listening on 127.0.0.1:7878

```sh
cd ../../cli
cargo run --features compiler,cranelift,debug -- --enable-threads ../wasix/tests/example-tcp-listener.wasm
```

In order to test this a curl command is needed below asynchronously and then it needs to be killed

```sh
curl 127.0.0.1:7878
```

Expected: 

```
Listening on 127.0.0.1:7878
Connection established!
```

## Thread local variables

Tests that thread local variables work correctly

```sh
cd ../../cli
cargo run --features compiler,cranelift,debug -- --enable-threads ../wasix/tests/example-thread-local.wasm
```

Expected:

```
VAR1 in main before change: FirstEnum
VAR1 in main after change: ThirdEnum(340282366920938463463374607431768211455)
VAR1 in thread step 1: FirstEnum
VAR1 in thread step 1: FirstEnum
VAR1 in thread step 1: FirstEnum
VAR1 in thread step 1: FirstEnum
VAR1 in thread step 1: FirstEnum
VAR1 in thread step 1: FirstEnum
VAR1 in thread step 1: FirstEnum
VAR1 in thread step 1: FirstEnum
VAR1 in thread step 1: FirstEnum
VAR1 in thread step 1: FirstEnum
VAR1 in thread step 2: FirstEnum
VAR1 in thread step 3: SecondEnum(4)
VAR1 in thread step 2: FirstEnum
VAR1 in thread step 3: SecondEnum(4)
VAR1 in thread step 2: FirstEnum
VAR1 in thread step 3: SecondEnum(4)
VAR1 in thread step 1: FirstEnum
VAR1 in thread step 2: FirstEnum
VAR1 in thread step 3: SecondEnum(4)
VAR1 in thread step 1: FirstEnum
VAR1 in thread step 4: SecondEnum(4)
VAR1 in thread step 4: SecondEnum(4)
VAR1 in thread step 4: SecondEnum(4)
VAR1 in thread step 2: FirstEnum
VAR1 in thread step 3: SecondEnum(4)
VAR1 in thread step 4: SecondEnum(4)
VAR1 in thread step 2: FirstEnum
VAR1 in thread step 3: SecondEnum(4)
VAR1 in thread step 4: SecondEnum(4)
VAR1 in thread step 2: FirstEnum
VAR1 in thread step 3: SecondEnum(4)
VAR1 in thread step 4: SecondEnum(4)
VAR1 in thread step 2: FirstEnum
VAR1 in thread step 3: SecondEnum(4)
VAR1 in thread step 4: SecondEnum(4)
VAR1 in thread step 2: FirstEnum
VAR1 in thread step 3: SecondEnum(4)
VAR1 in thread step 4: SecondEnum(4)
VAR1 in thread step 2: FirstEnum
VAR1 in thread step 3: SecondEnum(4)
VAR1 in thread step 4: SecondEnum(4)
VAR1 in thread step 2: FirstEnum
VAR1 in thread step 3: SecondEnum(4)
VAR1 in thread step 4: SecondEnum(4)
VAR1 in thread step 2: FirstEnum
VAR1 in thread step 3: SecondEnum(4)
VAR1 in thread step 4: SecondEnum(4)
VAR1 in thread step 4: SecondEnum(4)
VAR1 in main after thread midpoint: SecondEnum(998877)
VAR1 in main after thread join: SecondEnum(998877)
```

## vforking

Tests that lightweight forking that does not copy the memory but retains the
open file descriptors works correctly.

```sh
cd ../../cli
cargo run --features compiler,cranelift,debug -- --enable-threads ../wasix/tests/example-vfork.wasm
```

Expected:

```
Parent waiting on Child(1)
Child(1) exited with 10
```

## web server

Advanced test case that uses `tokio`, TCP listeners, asynchronous IO, event notifications, multi-threading
and mapped directories to serve HTTP content.


```sh
cd ../../cli
cargo run --features compiler,cranelift,debug -- --enable-threads --mapdir /public:/prog/deploy/wasmer-web/public ../wasix/tests/web-server.wasm -- --port 8080 --log-level trace
```

Note: This requires that a curl command be made to the HTTP server asynchronously

```sh
john@AlienWorld:/prog/wasix-libc/examples$ curl 127.0.0.1:8080
<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <meta name="Description" content="The Wasmer Shell is an browser based operating system powered by wasmer.io that integrates that allows the WebAssembly community to assembly and build browser hosted applications.">
    <meta property="og:url" content="https://wasmer.sh" />
    <meta property="og:title" content="wasmer.sh" />
    <meta property="og:description" content="An browser based operating system powered by wasmer.io that integrates that allows the WebAssembly community to assembly and build browser hosted applications" />
    <meta property="og:image" content="https://wasmer.sh/favicon.png" />
    <meta property="og:type" content="website" />
    <meta name="viewport" content="width=device-width,initial-scale=1">
    <meta name="mobile-web-app-capable" content="yes">
    <meta name="apple-mobile-web-app-capable" content="yes">
    <link rel="stylesheet" href="xterm.css" />
    <link rel="stylesheet" href="wasmer.css" />
    <link rel="icon" href="/favicon.png">
    <title>wasmer.sh</title>
    <meta name="viewport" content="width=device-width,initial-scale=1" />
    <script type="module" defer="defer" src="main.js"></script>
  </head>

  <body>
    <canvas id="frontBuffer"></canvas>
    <div id="terminal"></div>
  </body>
</html>
```

Expected:

```
2022-11-22T01:16:38.873595Z  INFO static_web_server::logger: logging level: trace
2022-11-22T01:16:38.873971Z DEBUG static_web_server::server: initializing tokio runtime with multi thread scheduler
2022-11-22T01:16:38.874273Z TRACE mio::sys::wasi: select::register: fd=7, token=Token(2147483648), interests=READABLE
2022-11-22T01:16:38.874750Z TRACE static_web_server::server: starting web server
2022-11-22T01:16:38.875090Z  INFO static_web_server::server: server bound to tcp socket [::]:8080
2022-11-22T01:16:38.875504Z  INFO static_web_server::server: runtime worker threads: 1
2022-11-22T01:16:38.875604Z  INFO static_web_server::server: security headers: enabled=false
2022-11-22T01:16:38.875894Z  INFO static_web_server::server: auto compression: enabled=true
2022-11-22T01:16:38.876151Z  INFO static_web_server::server: directory listing: enabled=false
2022-11-22T01:16:38.876238Z  INFO static_web_server::server: directory listing order code: 6
2022-11-22T01:16:38.876302Z  INFO static_web_server::server: cache control headers: enabled=true
2022-11-22T01:16:38.876690Z  INFO static_web_server::server: basic authentication: enabled=false
2022-11-22T01:16:38.876786Z  INFO static_web_server::server: log remote address: enabled=false
2022-11-22T01:16:38.877243Z  INFO static_web_server::server: grace period before graceful shutdown: 0s
2022-11-22T01:16:38.877405Z TRACE mio::poll: registering event source with poller: token=Token(0), interests=READABLE | WRITABLE
2022-11-22T01:16:38.877513Z TRACE mio::sys::wasi: select::register: fd=8, token=Token(0), interests=READABLE | WRITABLE
2022-11-22T01:16:38.877645Z  INFO Server::start_server{addr_str="[::]:8080" threads=1}: static_web_server::server: close time.busy=0.00ns time.idle=9.53µs
2022-11-22T01:16:38.877731Z  INFO static_web_server::server: listening on http://[::]:8080
2022-11-22T01:16:38.877793Z  INFO static_web_server::server: press ctrl+c to shut down the server
2022-11-22T01:16:47.494953Z TRACE mio::poll: registering event source with poller: token=Token(1), interests=READABLE | WRITABLE
2022-11-22T01:16:47.495488Z TRACE mio::sys::wasi: select::register: fd=10, token=Token(1), interests=READABLE | WRITABLE
2022-11-22T01:16:47.495966Z TRACE hyper::proto::h1::conn: Conn::read_head
2022-11-22T01:16:47.496094Z TRACE hyper::proto::h1::conn: flushed({role=server}): State { reading: Init, writing: Init, keep_alive: Busy }
2022-11-22T01:16:47.496342Z TRACE hyper::proto::h1::conn: Conn::read_head
2022-11-22T01:16:47.496762Z TRACE hyper::proto::h1::io: received 8114 bytes
2022-11-22T01:16:47.496877Z TRACE parse_headers: hyper::proto::h1::role: Request.parse bytes=8114
2022-11-22T01:16:47.496956Z TRACE parse_headers: hyper::proto::h1::role: Request.parse Complete(78)
2022-11-22T01:16:47.497058Z TRACE parse_headers: hyper::proto::h1::role: close time.busy=181µs time.idle=9.48µs
2022-11-22T01:16:47.497136Z DEBUG hyper::proto::h1::io: parsed 3 headers
2022-11-22T01:16:47.497202Z DEBUG hyper::proto::h1::conn: incoming body is empty
2022-11-22T01:16:47.497276Z  INFO static_web_server::handler: incoming request: method=GET uri=/
2022-11-22T01:16:47.497344Z TRACE static_web_server::static_files: dir? base="./public", route=""
2022-11-22T01:16:47.497755Z TRACE hyper::proto::h1::conn: flushed({role=server}): State { reading: KeepAlive, writing: Init, keep_alive: Busy }
2022-11-22T01:16:47.504970Z DEBUG static_web_server::static_files: dir: appending index.html to directory path
2022-11-22T01:16:47.505117Z TRACE static_web_server::static_files: dir: "./public/index.html"
2022-11-22T01:16:47.505321Z TRACE hyper::proto::h1::conn: flushed({role=server}): State { reading: KeepAlive, writing: Init, keep_alive: Busy }
2022-11-22T01:16:47.506066Z TRACE hyper::proto::h1::conn: flushed({role=server}): State { reading: KeepAlive, writing: Init, keep_alive: Busy }
2022-11-22T01:16:47.506221Z TRACE encode_headers: hyper::proto::h1::role: Server::encode status=200, body=Some(Unknown), req_method=Some(GET)
2022-11-22T01:16:47.506321Z TRACE encode_headers: hyper::proto::h1::role: close time.busy=99.2µs time.idle=7.56µs
2022-11-22T01:16:47.506744Z DEBUG hyper::proto::h1::io: flushed 209 bytes
2022-11-22T01:16:47.506911Z TRACE hyper::proto::h1::conn: flushed({role=server}): State { reading: KeepAlive, writing: Body(Encoder { kind: Length(1323), is_last: false }), keep_alive: Busy }
2022-11-22T01:16:47.508942Z TRACE hyper::proto::h1::encode: sized write, len = 1323
2022-11-22T01:16:47.509049Z TRACE hyper::proto::h1::io: buffer.queue self.len=0 buf.len=1323
2022-11-22T01:16:47.509134Z TRACE hyper::proto::h1::dispatch: no more write body allowed, user body is_end_stream = false
2022-11-22T01:16:47.509557Z DEBUG hyper::proto::h1::io: flushed 1323 bytes
2022-11-22T01:16:47.509654Z TRACE hyper::proto::h1::conn: flushed({role=server}): State { reading: Init, writing: Init, keep_alive: Idle }
2022-11-22T01:16:47.509957Z TRACE hyper::proto::h1::conn: Conn::read_head
2022-11-22T01:16:47.510057Z TRACE parse_headers: hyper::proto::h1::role: Request.parse bytes=8036
2022-11-22T01:16:47.510142Z TRACE parse_headers: hyper::proto::h1::role: close time.busy=81.8µs time.idle=9.02µs
2022-11-22T01:16:47.510217Z TRACE hyper::proto::h1::conn: State::close_read()
2022-11-22T01:16:47.510282Z DEBUG hyper::proto::h1::conn: parse error (invalid HTTP method parsed) with 8036 bytes
2022-11-22T01:16:47.510346Z DEBUG hyper::proto::h1::role: sending automatic response (400 Bad Request) for parse error
2022-11-22T01:16:47.510425Z TRACE encode_headers: hyper::proto::h1::role: Server::encode status=400, body=None, req_method=None
2022-11-22T01:16:47.510506Z TRACE encode_headers: hyper::proto::h1::role: close time.busy=78.0µs time.idle=7.84µs
2022-11-22T01:16:47.510662Z DEBUG hyper::proto::h1::io: flushed 84 bytes
2022-11-22T01:16:47.510742Z TRACE hyper::proto::h1::conn: flushed({role=server}): State { reading: Closed, writing: Closed, keep_alive: Disabled, error: hyper::Error(Parse(Method)) }
2022-11-22T01:16:47.510915Z TRACE hyper::proto::h1::conn: shut down IO complete
2022-11-22T01:16:47.510992Z DEBUG hyper::server::server::new_svc: connection error: invalid HTTP method parsed
2022-11-22T01:16:47.511058Z TRACE mio::poll: deregistering event source from poller
2022-11-22T01:16:47.511123Z TRACE mio::sys::wasi: select::deregister: fd=10
```
