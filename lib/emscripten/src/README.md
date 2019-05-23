## HOST APIS

#### EMSCRIPTEN APIS

###### PROCESS

- **\_abort** ✅ &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust
  fn _abort()
  ```
- **abort** ✅ 🔥 &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust
  fn abort(ctx: &mut Ctx, message: u32, )
  ```
- **abort_on_cannot_grow_memory** ✅ &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust
  fn abort_on_cannot_grow_memory()
  ```

###### TIMING

- **\_clock_gettime** &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```

###### ENVIRONMENT

- **\_getenv** ✅ &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust
  fn _getenv(ctx: &mut Ctx, name: c_int, )
  ```
- **\_putenv** ✅ &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust
  fn _putenv(ctx: &mut Ctx, name: c_int, )
  ```
- **\_setenv** ✅ &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust
  fn _setenv(name: c_int, value: c_int, overwrite: c_int, instance: &mut Instance
  ```
- **\_unsetenv** ✅ &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust
  fn _unsetenv(ctx: &mut Ctx, name: c_int, )
  ```

###### THREAD

- **\_pthread_getspecific** &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **\_pthread_key_create** &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **\_pthread_rwlock_destroy** &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **\_pthread_rwlock_init** &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **\_pthread_rwlock_wrlock** &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **\_pthread_setspecific** &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **\_\_\_lock** &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **\_\_\_unlock** &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```

###### MEMORY

- **\_emscripten_memcpy_big** ✅ 🔥 &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust
  fn _emscripten_memcpy_big(ctx: &mut Ctx, dest: u32, src: u32, len: u32, ) -> u32
  ```
- **enlarge_memory** ✅ &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust
  fn enlarge_memory()
  ```
- **get_total_memory** ✅ &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust
  fn get_total_memory(ctx: &mut Ctx, ) -> u32
  ```

###### TIMING

- **\_clock_gettime** &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```

###### STATUS

- **\_\_\_set_err_no** &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```

---

#### EMSCRIPTEN SYSCALLS

- **access** (\_\_\_syscall33) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **acct** (\_\_\_syscall51) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **chdir** (\_\_\_syscall12) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **chmod** (\_\_\_syscall15) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **chown** (\_\_\_syscall212) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **clock_nanosleep** (\_\_\_syscall265) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **close** (\_\_\_syscall6) ✅ ❗️ &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust
  fn close(fd: c_int) -> c_int
  ```
- **dup** (\_\_\_syscall330) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **dup** (\_\_\_syscall41) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **dup2** (\_\_\_syscall63) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **exit** (\_\_\_syscall1) ✅ &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust
  fn exit(status: c_int)
  ```
- **faccessat** (\_\_\_syscall307) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **fadvise** (\_\_\_syscall272) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **fallocate** (\_\_\_syscall324) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **fchdir** (\_\_\_syscall133) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **fchmod** (\_\_\_syscall94) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **fchmodat** (\_\_\_syscall306) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **fchown** (\_\_\_syscall207) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **fchownat** (\_\_\_syscall298) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **fcntl** (\_\_\_syscall221) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **fdatasync** (\_\_\_syscall148) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **fstat** (\_\_\_syscall197) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **fstatat** (\_\_\_syscall300) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **fstatfs** (\_\_\_syscall269) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **fsync** (\_\_\_syscall118) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **ftruncate** (\_\_\_syscall194) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **futimesat** (\_\_\_syscall299) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **getcwd** (\_\_\_syscall183) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **getdents** (\_\_\_syscall220) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **getgid** (\_\_\_syscall202) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **getgroups** (\_\_\_syscall205) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **getpgid** (\_\_\_syscall132) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **getpgrp** (\_\_\_syscall65) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **getpid** (\_\_\_syscall20) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **getppid** (\_\_\_syscall64) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **getpriority** (\_\_\_syscall96) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **getresgid** (\_\_\_syscall211) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **getrusage** (\_\_\_syscall77) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **getsid** (\_\_\_syscall147) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **ioctl** (\_\_\_syscall54) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **lchown** (\_\_\_syscall198) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **link** (\_\_\_syscall9) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **linkat** (\_\_\_syscall303) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **llseek** (\_\_\_syscall140) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **lstat** (\_\_\_syscall196) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **madvise** (\_\_\_syscall219) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **mincore** (\_\_\_syscall218) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **mkdir** (\_\_\_syscall39) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **mkdirat** (\_\_\_syscall296) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **mknod** (\_\_\_syscall14) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **mknodat** (\_\_\_syscall297) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **mmap** (\_\_\_syscall192) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **mprotect** (\_\_\_syscall125) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **mremap** (\_\_\_syscall163) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **msync** (\_\_\_syscall144) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **munlockall** (\_\_\_syscall153) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **munmap** (\_\_\_syscall91) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **newselect** (\_\_\_syscall142) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **nice** (\_\_\_syscall34) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **open** (\_\_\_syscall5) ✅ ❗️ 🔥 &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust
  fn open(ctx: &mut Ctx, path: u32, flags: c_int, mode: c_int, ) -> c_int
  ```
- **openat** (\_\_\_syscall295) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **pause** (\_\_\_syscall29) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **pipe** (\_\_\_syscall331) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **pipe** (\_\_\_syscall42) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **poll** (\_\_\_syscall168) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **pread** (\_\_\_syscall180) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **preadv** (\_\_\_syscall333) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **prlimit** (\_\_\_syscall340) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **pselect** (\_\_\_syscall308) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **pwrite** (\_\_\_syscall181) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **pwritev** (\_\_\_syscall334) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **read** (\_\_\_syscall3) ✅ ❗️ &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust
  fn read(ctx: &mut Ctx, fd: c_int, buf: u32, count: size_t, ) -> ssize_t
  ```
- **readlink** (\_\_\_syscall85) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **readlinkat** (\_\_\_syscall305) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **readv** (\_\_\_syscall145) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **recvmmsg** (\_\_\_syscall337) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **rename** (\_\_\_syscall38) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **renameat** (\_\_\_syscall302) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **rmdir** (\_\_\_syscall40) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **rt_sigqueueinfo** (\_\_\_syscall178) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **sendmmsg** (\_\_\_syscall345) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **setdomainname** (\_\_\_syscall121) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **setgid** (\_\_\_syscall214) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **setitimer** (\_\_\_syscall104) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **setpgid** (\_\_\_syscall57) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **setpriority** (\_\_\_syscall97) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **setresgid** (\_\_\_syscall210) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **setrlimit** (\_\_\_syscall75) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **setsid** (\_\_\_syscall66) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **socketcall** (\_\_\_syscall102) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **stat** (\_\_\_syscall195) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **statfs** (\_\_\_syscall268) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **symlink** (\_\_\_syscall83) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **symlinkat** (\_\_\_syscall304) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **sync** (\_\_\_syscall36) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **truncate** (\_\_\_syscall193) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **ugetrlimit** (\_\_\_syscall191) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **umask** (\_\_\_syscall60) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **uname** (\_\_\_syscall122) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **unlink** (\_\_\_syscall10) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **unlinkat** (\_\_\_syscall301) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **utimensat** (\_\_\_syscall320) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **wait** (\_\_\_syscall114) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **write** (\_\_\_syscall4) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```
- **writev** (\_\_\_syscall146) &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
  ```rust

  ```

### LEGEND &nbsp;&nbsp;&nbsp;&nbsp;[:top:](#host-apis)
✅ - Implemented

❗️ - Elevated privilege

🔥 - Possible memory access violation

📥 - Access to external memory

📝 - External write to internal memory
