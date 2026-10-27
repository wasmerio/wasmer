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
    /// descriptors, so it announces the same preopens, and the same arguments. Families whose
    /// state follows descriptors or dispositions copy theirs here.
    pub fn fork(&self) -> Self {
        let startup = OnceLock::new();
        if let Some(loaded) = self.startup.get() {
            let _ = startup.set(loaded.clone());
        }
        Self {
            startup,
            fs: Default::default(),
            net: Default::default(),
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

/// The compat state of every process that runs v1 code, by pid, and the package commands
/// v1 programs start with their defaults.
#[derive(Default)]
pub struct Procs {
    procs: Mutex<HashMap<TaskId, Arc<ProcState>>>,
    commands: HashMap<Vec<u8>, CommandDefaults>,
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
    /// No processes yet; v1 programs that start the guest path of one of `commands` (such as
    /// `/bin/python`) pass it its [`CommandDefaults`].
    pub fn with_commands(commands: HashMap<Vec<u8>, CommandDefaults>) -> Self {
        Self {
            procs: Mutex::default(),
            commands,
        }
    }

    /// The defaults v1 programs that start the file at `path` pass it.
    pub fn command(&self, path: &[u8]) -> Option<&CommandDefaults> {
        self.commands.get(path)
    }

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
