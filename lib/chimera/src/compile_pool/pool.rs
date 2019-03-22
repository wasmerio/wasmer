use super::job::{Job, Priority};
use crate::utils::lazy::Lazy;
use crossbeam_deque::{Injector, Steal, Stealer, Worker};
use num_cpus;
use std::{iter, sync::Arc, thread};

enum Msg {
    Compile(Job),
    Terminate,
}

struct Queue {
    local: Worker<Msg>,
    global: Arc<Injector<Msg>>,
    stealers: Arc<[Stealer<Msg>]>,
}

impl Queue {
    fn create_pool(num: usize) -> (Arc<Injector<Msg>>, Vec<Queue>) {
        let global = Arc::new(Injector::new());
        let locals = {
            let mut v = Vec::new();
            v.resize_with(num, || Worker::new_fifo());
            v
        };
        let stealers: Arc<[_]> = locals
            .iter()
            .map(|local| local.stealer())
            .collect::<Vec<_>>()
            .into();

        let queues = locals
            .into_iter()
            .map(|local| Queue {
                local,
                global: Arc::clone(&global),
                stealers: Arc::clone(&stealers),
            })
            .collect();

        (global, queues)
    }

    fn get(&self) -> Option<Msg> {
        self.local.pop().or_else(|| {
            // Otherwise, we must look for a task elsewhere.
            iter::repeat_with(|| {
                // Try stealing a batch from the global queue.
                self.global
                    .steal_batch_and_pop(&self.local)
                    // Or steal a task from another worker.
                    .or_else(|| self.stealers.iter().map(|s| s.steal()).collect())
            })
            .find(|s| !s.is_retry())
            .and_then(|s| s.success())
        })
    }
}

struct CompilerThread {
    cold: Queue,
    warm: Queue,
    hot: Queue,
}

impl CompilerThread {
    fn work(self) {
        loop {
            match self.find_job() {
                Some(Msg::Compile(job)) => job.do_compile(),
                Some(Msg::Terminate) => break,
                None => thread::yield_now(),
            }
        }
    }

    fn find_job(&self) -> Option<Msg> {
        self.hot
            .get()
            .or_else(|| self.warm.get())
            .or_else(|| self.cold.get())
    }
}

struct CompilePool {
    num_workers: usize,
    cold_injector: Arc<Injector<Msg>>,
    warm_injector: Arc<Injector<Msg>>,
    hot_injector: Arc<Injector<Msg>>,
}

impl CompilePool {
    fn new() -> Self {
        let num_workers = num_cpus::get();

        info!(target: "initialization", "Spinning up {} compiler worker threads", num_workers);

        let (cold_injector, cold_queues) = Queue::create_pool(num_workers);
        let (warm_injector, warm_queues) = Queue::create_pool(num_workers);
        let (hot_injector, hot_queues) = Queue::create_pool(num_workers);

        for ((cold, warm), hot) in cold_queues
            .into_iter()
            .zip(warm_queues.into_iter())
            .zip(hot_queues.into_iter())
        {
            let compiler_thread = CompilerThread { cold, warm, hot };

            thread::spawn(move || {
                compiler_thread.work();
            });
        }

        Self {
            num_workers,
            cold_injector,
            warm_injector,
            hot_injector,
        }
    }

    /// Add a job. The priority is taken from the `Job::priority()` method.
    /// Hot jobs will run before warm jobs, which will run before cold jobs.
    fn inject_job(&self, job: Job) {
        match job.priority() {
            Priority::Hot => self.hot_injector.push(Msg::Compile(job)),
            Priority::Warm => self.warm_injector.push(Msg::Compile(job)),
            Priority::Cold => self.cold_injector.push(Msg::Compile(job)),
        }
    }
}

impl Drop for CompilePool {
    fn drop(&mut self) {
        for _ in 0..self.num_workers {
            // Push into the queue most likely to be read soon.
            self.hot_injector.push(Msg::Terminate);
        }
    }
}

pub struct Compiler;

impl Compiler {
    pub fn inject(&self, job: Job) {
        static COMPILE_POOL: Lazy<CompilePool> = Lazy::new(|| CompilePool::new());

        COMPILE_POOL.inject_job(job);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create() {
        let _pool = CompilePool::new();
    }

    #[test]
    fn send_jobs() {
        let pool = CompilePool::new();
    }
}
