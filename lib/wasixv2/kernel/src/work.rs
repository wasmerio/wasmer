//! Work the execution layer does for a task on its next return to the guest: Linux's thread info
//! flags and what they stand for (a pending exit, `task_work`, `TIF_NOTIFY_RESUME`). Signals are
//! not queued here; they are dequeued from the signal state when the queue is empty.

use std::{collections::VecDeque, sync::Arc};

use wasixv2_syscalls::{GuestAction, Image, TaskId, VforkParent, WorkFlag};

pub(crate) struct Work {
    queue: VecDeque<GuestAction>,
    /// Raised with every action queued, and by the signal state for signals.
    flag: Arc<WorkFlag>,
    /// The image of the queued [`GuestAction::Exec`], until the execution layer takes it. It
    /// goes with the task when the exec never runs (`bprm` freed with the exiting task).
    image: Option<Image>,
}

impl Work {
    pub(crate) fn new(flag: Arc<WorkFlag>) -> Self {
        Self {
            queue: VecDeque::new(),
            flag,
            image: None,
        }
    }

    pub(crate) fn flag(&self) -> &Arc<WorkFlag> {
        &self.flag
    }

    /// Queues `action` behind what is already there. A task's first exit is final: a later one is
    /// dropped, and the tasks it has yet to start never will be: threads, and the children of a
    /// fork or vfork queued behind the exit. Returns those tasks, which the caller aborts.
    #[must_use]
    pub(crate) fn push(&mut self, action: GuestAction) -> Vec<TaskId> {
        let exiting = self.exit().is_some();
        let unstarted = match action {
            GuestAction::Exit(_) if exiting => Vec::new(),
            GuestAction::StartThread { task, .. } if exiting => vec![task],
            GuestAction::SwitchTask { task, .. } if exiting => vec![task],
            GuestAction::Fork { child, .. } if exiting => vec![child],
            GuestAction::Exit(_) => {
                let unstarted = self.take_unstarted();
                self.queue.push_back(action);
                unstarted
            }
            // A vfork child's end runs in its parent's instance, which its exit must not unwind:
            // resuming the parent is all that is left to do.
            GuestAction::VforkReturn { .. } => {
                self.queue
                    .retain(|action| !matches!(action, GuestAction::Exit(_)));
                self.queue.push_back(action);
                Vec::new()
            }
            _ => {
                self.queue.push_back(action);
                Vec::new()
            }
        };
        self.flag.raise();
        unstarted
    }

    /// The oldest queued action.
    pub(crate) fn take(&mut self) -> Option<GuestAction> {
        self.queue.pop_front()
    }

    /// The status of the queued exit, if there is one.
    pub(crate) fn exit(&self) -> Option<i32> {
        self.queue.iter().find_map(|action| match action {
            GuestAction::Exit(status) => Some(*status),
            _ => None,
        })
    }

    /// Whether the task's image ends on its next return to the guest: it exits, or an `execve`
    /// left it a new image.
    pub(crate) fn ends_image(&self) -> bool {
        self.queue
            .iter()
            .any(|action| matches!(action, GuestAction::Exit(_) | GuestAction::Exec { .. }))
    }

    /// Whether an `execve` left its image to run.
    pub(crate) fn has_exec(&self) -> bool {
        self.queue
            .iter()
            .any(|action| matches!(action, GuestAction::Exec { .. }))
    }

    /// Queues the `execve` that runs `image`.
    #[must_use]
    pub(crate) fn exec(&mut self, image: Image, vfork_parent: Option<VforkParent>) -> Vec<TaskId> {
        self.image = Some(image);
        self.push(GuestAction::Exec { vfork_parent })
    }

    /// The image of the queued `execve`, for the execution layer to run.
    pub(crate) fn take_image(&mut self) -> Option<Image> {
        self.image.take()
    }

    /// Takes the `execve`'s own action off the queue, leaving the rest as it was, and returns
    /// its image.
    pub(crate) fn take_exec(&mut self) -> Option<Image> {
        let before = self.queue.len();
        self.queue
            .retain(|action| !matches!(action, GuestAction::Exec { .. }));
        let image = self.image.take();
        (self.queue.len() != before).then_some(image).flatten()
    }

    /// Takes the threads the task was to start, which never will be.
    #[must_use]
    pub(crate) fn take_unstarted(&mut self) -> Vec<TaskId> {
        let mut unstarted = Vec::new();
        self.queue.retain(|action| match action {
            GuestAction::StartThread { task, .. } => {
                unstarted.push(*task);
                false
            }
            _ => true,
        });
        unstarted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work() -> Work {
        Work::new(Arc::default())
    }

    #[test]
    fn actions_come_out_in_order_and_raise_the_flag() {
        let mut work = work();
        assert!(!work.flag().is_raised());
        let start = GuestAction::StartThread {
            task: TaskId(2),
            start_arg: 0,
        };
        let switch = GuestAction::SwitchTask {
            task: TaskId(3),
            ret: 0,
        };
        assert!(work.push(start).is_empty());
        assert!(work.push(switch).is_empty());
        assert!(work.flag().is_raised());
        assert_eq!(work.take(), Some(start));
        assert_eq!(work.take(), Some(switch));
        assert_eq!(work.take(), None);
    }

    #[test]
    fn an_exit_wins_and_unstarted_threads_never_start() {
        let mut work = work();
        let start = |n| GuestAction::StartThread {
            task: TaskId(n),
            start_arg: 0,
        };
        assert!(work.push(start(2)).is_empty());
        assert_eq!(work.push(GuestAction::Exit(3)), vec![TaskId(2)]);
        assert!(work.push(GuestAction::Exit(4)).is_empty());
        assert_eq!(work.push(start(5)), vec![TaskId(5)]);
        assert_eq!(work.exit(), Some(3));
        assert_eq!(work.take(), Some(GuestAction::Exit(3)));
        assert_eq!(work.take(), None);
    }

    #[test]
    fn a_fork_or_vfork_behind_an_exit_never_starts_its_child() {
        let mut work = work();
        assert!(work.push(GuestAction::Exit(9)).is_empty());
        let switch = GuestAction::SwitchTask {
            task: TaskId(2),
            ret: 0,
        };
        let fork = GuestAction::Fork {
            child: TaskId(3),
            parent_tid: None,
            child_tid: None,
            tls: None,
        };
        assert_eq!(work.push(switch), vec![TaskId(2)]);
        assert_eq!(work.push(fork), vec![TaskId(3)]);
        assert_eq!(work.take(), Some(GuestAction::Exit(9)));
        assert_eq!(work.take(), None);
    }

    #[test]
    fn taking_the_exec_leaves_what_was_queued_before_it() {
        let mut work = work();
        assert!(work.push(GuestAction::Exit(137)).is_empty());
        assert!(work.exec(Image::copied(b"image".to_vec()), None).is_empty());
        assert_eq!(
            work.take_exec().as_ref().map(Image::bytes),
            Some(&b"image"[..])
        );
        assert!(work.take_exec().is_none());
        assert_eq!(work.take(), Some(GuestAction::Exit(137)));
        assert_eq!(work.take(), None);
    }

    #[test]
    fn a_vfork_childs_end_resumes_the_parent_instead_of_unwinding() {
        let mut work = work();
        let parent = VforkParent {
            task: TaskId(1),
            resumes: true,
        };
        let resume = GuestAction::VforkReturn {
            parent,
            child: TaskId(2),
        };
        assert!(work.push(GuestAction::Exit(127)).is_empty());
        assert!(work.push(resume).is_empty());
        assert_eq!(work.take(), Some(resume));
        assert_eq!(work.take(), None);
    }
}
