use crate::memory::UserMemory;

/// Identifies a task (a thread, in Linux terms) inside one kernel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TaskId(pub u32);

/// The task making a syscall and the memory its pointers refer to.
pub struct Caller<'a> {
    task: TaskId,
    memory: &'a dyn UserMemory,
}

impl<'a> Caller<'a> {
    pub fn new(task: TaskId, memory: &'a dyn UserMemory) -> Self {
        Self { task, memory }
    }

    pub fn task(&self) -> TaskId {
        self.task
    }

    pub fn memory(&self) -> &'a dyn UserMemory {
        self.memory
    }
}
