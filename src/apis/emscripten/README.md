## HOST APIS

#### EMSCRIPTEN APIS
###### PROCESS
- **_abort** ✅ &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    fn _abort()
    ```
- **abort** ✅ 🔥 &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    fn abort(message: u32, instance: &mut Instance)
    ```
- **abort_on_cannot_grow_memory** ✅ &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    fn abort_on_cannot_grow_memory()
    ```

###### TIMING
- **_clock_gettime** &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```

###### ENVIRONMENT
- **_getenv** &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```

###### THREAD
- **_pthread_getspecific** &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **_pthread_key_create** &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **_pthread_setspecific** &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **_unsetenv** &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **___lock** &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **___unlock** &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```

###### MEMORY
- **_emscripten_memcpy_big** ✅ 🔥 &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    fn _emscripten_memcpy_big(dest: u32, src: u32, len: u32, instance: &mut Instance) -> u32
    ```
- **enlarge_memory** ✅ &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    fn enlarge_memory()
    ```
- **get_total_memory** ✅ &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    fn get_total_memory(instance: &mut Instance) -> u32
    ```

###### TIMING

- **_clock_gettime** &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```

###### STATUS
- **___set_err_no** &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```

-------------------------------------------------------------------

#### EMSCRIPTEN SYSCALLS
- **access** (___syscall33) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **acct** (___syscall51) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **chdir** (___syscall12) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **chmod** (___syscall15) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **chown** (___syscall212) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **clock_nanosleep** (___syscall265) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **close** (___syscall6)  ✅ ❗️ &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    fn close(fd: c_int) -> c_int
    ```
- **dup** (___syscall330) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **dup** (___syscall41) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **dup** (___syscall63) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **exit** (___syscall1) ✅ &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    fn exit(status: c_int)
    ```
- **faccessat** (___syscall307) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **fadvise** (___syscall272) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **fallocate** (___syscall324) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **fchdir** (___syscall133) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **fchmod** (___syscall94) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **fchmodat** (___syscall306) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **fchown** (___syscall207) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **fchownat** (___syscall298) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **fcntl** (___syscall221) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **fdatasync** (___syscall148) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **fstat** (___syscall197) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **fstatat** (___syscall300) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **fstatfs** (___syscall269) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **fsync** (___syscall118) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **ftruncate** (___syscall194) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **futimesat** (___syscall299) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **getcwd** (___syscall183) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **getdents** (___syscall220) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **getgid** (___syscall202) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **getgroups** (___syscall205) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **getpgid** (___syscall132) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **getpgrp** (___syscall65) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **getpid** (___syscall20) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **getppid** (___syscall64) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **getpriority** (___syscall96) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **getresgid** (___syscall211) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **getrusage** (___syscall77) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **getsid** (___syscall147) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **ioctl** (___syscall54) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **lchown** (___syscall198) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **link** (___syscall9) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **linkat** (___syscall303) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **llseek** (___syscall140) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **lstat** (___syscall196) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **madvise** (___syscall219) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **mincore** (___syscall218) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **mkdir** (___syscall39) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **mkdirat** (___syscall296) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **mknod** (___syscall14) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **mknodat** (___syscall297) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **mmap** (___syscall192) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **mprotect** (___syscall125) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **mremap** (___syscall163) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **msync** (___syscall144) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **munlockall** (___syscall153) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **munmap** (___syscall91) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **newselect** (___syscall142) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **nice** (___syscall34) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **open** (___syscall5) ✅ ❗️ 🔥 &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    fn open(path: u32, flags: c_int, mode: c_int, instance: &mut Instance) -> c_int
    ```
- **openat** (___syscall295) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **pause** (___syscall29) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **pipe** (___syscall331) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **pipe** (___syscall42) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **poll** (___syscall168) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **pread** (___syscall180) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **preadv** (___syscall333) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **prlimit** (___syscall340) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **pselect** (___syscall308) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **pwrite** (___syscall181) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **pwritev** (___syscall334) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **read** (___syscall3) ✅ ❗️ &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    fn read(fd: c_int, buf: u32, count: size_t, instance: &mut Instance) -> ssize_t
    ```
- **readlink** (___syscall85) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **readlinkat** (___syscall305) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **readv** (___syscall145) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **recvmmsg** (___syscall337) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **rename** (___syscall38) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **renameat** (___syscall302) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **rmdir** (___syscall40) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **rt_sigqueueinfo** (___syscall178) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **sendmmsg** (___syscall345) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **setdomainname** (___syscall121) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **setgid** (___syscall214) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **setitimer** (___syscall104) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **setpgid** (___syscall57) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **setpriority** (___syscall97) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **setresgid** (___syscall210) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **setrlimit** (___syscall75) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **setsid** (___syscall66) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **socketcall** (___syscall102) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **stat** (___syscall195) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **statfs** (___syscall268) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **symlink** (___syscall83) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **symlinkat** (___syscall304) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **sync** (___syscall36) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **truncate** (___syscall193) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **ugetrlimit** (___syscall191) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **umask** (___syscall60) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **uname** (___syscall122) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **unlink** (___syscall10) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **unlinkat** (___syscall301) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **utimensat** (___syscall320) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **wait** (___syscall114) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **write** (___syscall4) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
- **writev** (___syscall146) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
    ```rust
    ```
