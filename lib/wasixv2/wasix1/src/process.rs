//! Per-process compat state: what v1 keeps about a process that Linux does not, keyed by pid.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

use wasixv2_syscalls::{Fd, TaskId};

use crate::calls;

/// The compat state of one process image. A new image (`execve`) gets a new one; threads share
/// it through the pid.
#[derive(Default)]
pub struct ProcState {
    /// Loaded on the process's first v1 call; see [`calls::startup::load`].
    pub(crate) startup: OnceLock<Arc<Startup>>,
    pub fs: calls::fs::State,
    pub net: calls::sock::State,
    pub proc: calls::proc::State,
}

impl ProcState {
    /// The state of a child that `fork` or `vfork` makes of this process: it has the same
    /// descriptors, so it announces the same preopens and keeps what the families know about
    /// them (renumbered preopens, epoll data, socket timeouts), and the same arguments,
    /// dispositions and terminal.
    pub fn fork(&self) -> Self {
        let startup = OnceLock::new();
        if let Some(loaded) = self.startup.get() {
            let _ = startup.set(loaded.clone());
        }
        Self {
            startup,
            fs: self.fs.fork(),
            net: self.net.fork(),
            proc: self.proc.fork(),
        }
    }
}

/// What v1 reads once per image: the argument and environment strings, and the preopened
/// directories.
#[derive(Debug, Default)]
pub struct Startup {
    pub args: Vec<Vec<u8>>,
    pub env: Vec<Vec<u8>>,
    pub preopens: Vec<Preopen>,
}

/// A directory v1 announces through `fd_prestat_get`.
#[derive(Debug, Clone)]
pub struct Preopen {
    pub fd: Fd,
    /// The name `fd_prestat_dir_name` reports, such as `/`.
    pub name: Vec<u8>,
}

impl Startup {
    pub fn preopen(&self, fd: Fd) -> Option<&Preopen> {
        self.preopens.iter().find(|preopen| preopen.fd == fd)
    }
}

/// The compat state of every process that runs v1 code, by pid.
#[derive(Default)]
pub struct Procs {
    procs: Mutex<HashMap<TaskId, Arc<ProcState>>>,
}

/// The package commands v1 programs start with their [`CommandDefaults`], by the file each
/// command is. Keyed by the file rather than by a path, so a command started through any name
/// for it (`/bin/python`, `./python` in `/bin`, a symlink to it) gets its defaults, and a file
/// that only took a command's name does not. The embedder fills it once the files exist and
/// shares it with [`crate::Wasix1`].
#[derive(Debug, Default)]
pub struct Commands(HashMap<FileId, CommandDefaults>);

/// A file, as `statx` tells it: its device and inode number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileId {
    pub dev: (u32, u32),
    pub ino: u64,
}

impl Commands {
    /// Makes `file` a command v1 programs start with `defaults`; the first registration of a
    /// file wins.
    pub fn insert(&mut self, file: FileId, defaults: CommandDefaults) {
        self.0.entry(file).or_insert(defaults);
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The defaults v1 programs that start `file` pass it.
    pub fn get(&self, file: FileId) -> Option<&CommandDefaults> {
        self.0.get(&file)
    }
}

/// What WASIX v1 adds when a v1 program starts a package command, from the command's `wasi`
/// annotation: v1 registered every command at `/bin/<name>` and `/usr/bin/<name>`, and starting
/// one of those paths applied it (`WasiEnv::prepare_spawn`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandDefaults {
    /// `exec-name`: replaces `argv[0]`.
    pub argv0: Option<String>,
    /// `main-args`: inserted after `argv[0]`, before the caller's arguments.
    pub args: Vec<String>,
    /// `env`: `NAME=value` strings, each added unless the environment already names `NAME`.
    pub env: Vec<String>,
}

impl CommandDefaults {
    /// `args` and `env` as v1 hands them to the command.
    pub fn apply(&self, args: &mut Vec<Vec<u8>>, env: &mut Vec<Vec<u8>>) {
        if let Some(argv0) = &self.argv0 {
            match args.first_mut() {
                Some(first) => *first = argv0.clone().into_bytes(),
                None => args.push(argv0.clone().into_bytes()),
            }
        }
        let at = args.len().min(1);
        args.splice(at..at, self.args.iter().map(|arg| arg.clone().into_bytes()));
        let name = |entry: &[u8]| entry.split(|&b| b == b'=').next().unwrap_or(entry).to_vec();
        let mut names: std::collections::HashSet<Vec<u8>> =
            env.iter().map(|entry| name(entry)).collect();
        // v1 skips a default without `=`.
        for default in self.env.iter().filter(|default| default.contains('=')) {
            if names.insert(name(default.as_bytes())) {
                env.push(default.clone().into_bytes());
            }
        }
    }
}

impl Procs {
    /// The state of `pid`, created empty if the process has none yet (a vfork child before its
    /// parent's state was copied, a process whose image another ABI loaded).
    pub fn get_or_create(&self, pid: TaskId) -> Arc<ProcState> {
        self.procs.lock().unwrap().entry(pid).or_default().clone()
    }

    /// Starts `pid` over with `state`, as a new process image does.
    pub fn replace(&self, pid: TaskId, state: Arc<ProcState>) {
        self.procs.lock().unwrap().insert(pid, state);
    }

    /// Drops `pid`'s state if it still is `state`, when the image that made it ends.
    pub fn remove(&self, pid: TaskId, state: &Arc<ProcState>) {
        let mut procs = self.procs.lock().unwrap();
        if procs
            .get(&pid)
            .is_some_and(|current| Arc::ptr_eq(current, state))
        {
            procs.remove(&pid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(list: &[&str]) -> Vec<Vec<u8>> {
        list.iter().map(|s| s.as_bytes().to_vec()).collect()
    }

    /// As `WasiEnv::prepare_spawn`: `exec-name` replaces argv[0], `main-args` go after it, and
    /// `env` fills in names the environment lacks, skipping entries without `=`.
    #[test]
    fn command_defaults_apply_as_v1() {
        let defaults = CommandDefaults {
            argv0: Some("python3".into()),
            args: vec!["-I".into(), "/app/main.py".into()],
            env: vec!["HOME=/tmp".into(), "PATH=/bin".into(), "junk".into()],
        };
        let (mut args, mut env) = (strings(&["python", "x"]), strings(&["PATH=/usr/bin"]));
        defaults.apply(&mut args, &mut env);
        assert_eq!(args, strings(&["python3", "-I", "/app/main.py", "x"]));
        assert_eq!(env, strings(&["PATH=/usr/bin", "HOME=/tmp"]));

        let (mut args, mut env) = (Vec::new(), Vec::new());
        CommandDefaults {
            argv0: None,
            ..defaults.clone()
        }
        .apply(&mut args, &mut env);
        assert_eq!(args, strings(&["-I", "/app/main.py"]));
        let (mut args, mut env) = (Vec::new(), Vec::new());
        defaults.apply(&mut args, &mut env);
        assert_eq!(args, strings(&["python3", "-I", "/app/main.py"]));
    }
}
