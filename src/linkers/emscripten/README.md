# HOST APIS

## EMSCRIPTEN APIS
#### PROCESS
- <a name="abort"></a>SYS_fstat (___syscall197) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="_abort"></a>abort &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
fn abort()
```
- <a name="_clock_gettime"></a>abort &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
fn abort()
```
- <a name="_emscripten_memcpy_big"></a>abort &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
fn abort()
```
- <a name="_getenv"></a>abort &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
fn abort()
```

#### THREAD
- <a name="_pthread_getspecific"></a>abort &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
fn abort()
```
- <a name="_pthread_key_create"></a>abort &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
fn abort()
```
- <a name="_pthread_setspecific"></a>abort &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
fn abort()
```
- <a name="_unsetenv"></a>abort &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
fn abort()
```
- <a name="___lock"></a>SYS_fstat (___syscall197) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="___unlock"></a>SYS_fstat (___syscall197) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```

#### MEMORY
- <a name="abortOnCannotGrowMemory"></a>SYS_fstat (___syscall197) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="enlargeMemory"></a>SYS_fstat (___syscall197) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="getTotalMemory"></a>SYS_fstat (___syscall197) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```

#### TIMING

- <a name="_clock_gettime"></a>SYS_fstat (___syscall197) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```

#### STATUS
- <a name="___setErrNo"></a>SYS_fstat (___syscall197) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```

## EMSCRIPTEN SYSCALLS
- <a name="SYS_fstat"></a>SYS_fstat (___syscall197) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="SYS_getdents"></a>SYS_getdents (___syscall220) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="SYS_lstat"></a>SYS_lstat (___syscall196) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="SYS_stat"></a>SYS_stat (___syscall195) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="access"></a>access (___syscall33) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="acct"></a>acct (___syscall51) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="chdir"></a>chdir (___syscall12) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="chmod"></a>chmod (___syscall15) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="chown"></a>chown (___syscall212) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="clock_nanosleep"></a>clock_nanosleep (___syscall265) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="close"></a>close (___syscall6) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="dup"></a>dup (___syscall330) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="dup"></a>dup (___syscall41) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="dup"></a>dup (___syscall63) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="exit"></a>exit (___syscall1) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
fn exit(status: c_int)
```
- <a name="faccessat"></a>faccessat (___syscall307) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="fadvise"></a>fadvise (___syscall272) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="fallocate"></a>fallocate (___syscall324) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="fchdir"></a>fchdir (___syscall133) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="fchmod"></a>fchmod (___syscall94) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="fchmodat"></a>fchmodat (___syscall306) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="fchown"></a>fchown (___syscall207) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="fchownat"></a>fchownat (___syscall298) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="fcntl"></a>fcntl (___syscall221) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="fdatasync"></a>fdatasync (___syscall148) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="fstatat"></a>fstatat (___syscall300) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="fstatfs"></a>fstatfs (___syscall269) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="fsync"></a>fsync (___syscall118) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="ftruncate"></a>ftruncate (___syscall194) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="futimesat"></a>futimesat (___syscall299) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="getcwd"></a>getcwd (___syscall183) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="getgid"></a>getgid (___syscall202) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="getgroups"></a>getgroups (___syscall205) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="getpgid"></a>getpgid (___syscall132) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="getpgrp"></a>getpgrp (___syscall65) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="getpid"></a>getpid (___syscall20) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="getppid"></a>getppid (___syscall64) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="getpriority"></a>getpriority (___syscall96) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="getresgid"></a>getresgid (___syscall211) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="getrusage"></a>getrusage (___syscall77) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="getsid"></a>getsid (___syscall147) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="ioctl"></a>ioctl (___syscall54) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="lchown"></a>lchown (___syscall198) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="link"></a>link (___syscall9) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="linkat"></a>linkat (___syscall303) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="llseek"></a>llseek (___syscall140) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="madvise"></a>madvise (___syscall219) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="mincore"></a>mincore (___syscall218) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="mkdir"></a>mkdir (___syscall39) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="mkdirat"></a>mkdirat (___syscall296) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="mknod"></a>mknod (___syscall14) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="mknodat"></a>mknodat (___syscall297) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="mmap"></a>mmap (___syscall192) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="mprotect"></a>mprotect (___syscall125) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="mremap"></a>mremap (___syscall163) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="msync"></a>msync (___syscall144) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="munlockall"></a>munlockall (___syscall153) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="munmap"></a>munmap (___syscall91) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="newselect"></a>newselect (___syscall142) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="nice"></a>nice (___syscall34) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="open"></a>open (___syscall5) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
fn open(path: *const c_char, oflag: c_int, ...) -> c_int
```
- <a name="openat"></a>openat (___syscall295) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="pause"></a>pause (___syscall29) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="pipe"></a>pipe (___syscall331) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="pipe"></a>pipe (___syscall42) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="poll"></a>poll (___syscall168) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="pread"></a>pread (___syscall180) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="preadv"></a>preadv (___syscall333) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="prlimit"></a>prlimit (___syscall340) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="pselect"></a>pselect (___syscall308) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="pwrite"></a>pwrite (___syscall181) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="pwritev"></a>pwritev (___syscall334) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="read"></a>read (___syscall3) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
fn read(fd: usize, buf: *mut c_void, count: usize) -> isize
```
- <a name="readlink"></a>readlink (___syscall85) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="readlinkat"></a>readlinkat (___syscall305) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="readv"></a>readv (___syscall145) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="recvmmsg"></a>recvmmsg (___syscall337) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="rename"></a>rename (___syscall38) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="renameat"></a>renameat (___syscall302) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="rmdir"></a>rmdir (___syscall40) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="rt_sigqueueinfo"></a>rt_sigqueueinfo (___syscall178) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="sendmmsg"></a>sendmmsg (___syscall345) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="setdomainname"></a>setdomainname (___syscall121) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="setgid"></a>setgid (___syscall214) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="setitimer"></a>setitimer (___syscall104) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="setpgid"></a>setpgid (___syscall57) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="setpriority"></a>setpriority (___syscall97) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="setresgid"></a>setresgid (___syscall210) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="setrlimit"></a>setrlimit (___syscall75) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="setsid"></a>setsid (___syscall66) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="socketcall"></a>socketcall (___syscall102) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="statfs"></a>statfs (___syscall268) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="symlink"></a>symlink (___syscall83) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="symlinkat"></a>symlinkat (___syscall304) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="sync"></a>sync (___syscall36) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="truncate"></a>truncate (___syscall193) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="ugetrlimit"></a>ugetrlimit (___syscall191) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="umask"></a>umask (___syscall60) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="uname"></a>uname (___syscall122) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="unlink"></a>unlink (___syscall10) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="unlinkat"></a>unlinkat (___syscall301) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="utimensat"></a>utimensat (___syscall320) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="wait"></a>wait (___syscall114) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="write"></a>write (___syscall4) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
- <a name="writev"></a>writev (___syscall146) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
```rust
```
