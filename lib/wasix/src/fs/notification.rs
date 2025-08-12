use std::{
    collections::VecDeque,
    sync::Mutex,
    task::{Poll, Waker},
};

use virtual_mio::{InterestHandler, InterestType};

#[derive(Debug)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
struct NotificationState {
    /// Used for event notifications by the user application or operating system
    /// (positive number means there are events waiting to be processed)
    counter: u64,
    /// Counter used to prevent duplicate notification events
    last_poll: u64,
    /// Flag that indicates if this is operating
    is_semaphore: bool,
    /// All the registered wakers
    #[cfg_attr(feature = "enable-serde", serde(skip))]
    wakers: VecDeque<Waker>,
    /// InterestHandler for use with epoll
    #[cfg_attr(feature = "enable-serde", serde(skip))]
    interest_handler: Option<Box<dyn InterestHandler>>,
}

impl NotificationState {
    fn add_waker(&mut self, waker: &Waker) {
        if !self.wakers.iter().any(|a| a.will_wake(waker)) {
            self.wakers.push_front(waker.clone());
        }
    }

    fn wake_all(&mut self) {
        self.last_poll = u64::MAX;
        while let Some(waker) = self.wakers.pop_front() {
            waker.wake();
        }
        if let Some(handler) = self.interest_handler.as_mut() {
            handler.push_interest(InterestType::Readable);
        }
    }

    fn inc(&mut self, val: u64) {
        self.counter += val;
        self.wake_all();
    }

    fn dec(&mut self) -> u64 {
        let val = self.counter;
        if self.is_semaphore {
            if self.counter > 0 {
                self.counter -= 1;
                if self.counter > 0 {
                    self.wake_all();
                }
            }
        } else {
            self.counter = 0;
        }
        val
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct NotificationInner {
    /// Receiver that wakes sleeping threads
    #[cfg_attr(feature = "enable-serde", serde(skip))]
    state: Mutex<NotificationState>,
}

impl NotificationInner {
    pub fn new(initial_val: u64, is_semaphore: bool) -> Self {
        Self {
            state: Mutex::new(NotificationState {
                counter: initial_val,
                last_poll: u64::MAX,
                is_semaphore,
                wakers: Default::default(),
                interest_handler: None,
            }),
        }
    }
    pub fn poll(&self, waker: &Waker) -> Poll<usize> {
        let mut state = self.state.lock().unwrap();
        state.add_waker(waker);

        if state.last_poll != state.counter {
            state.last_poll = state.counter;
            Poll::Ready(state.counter as usize)
        } else {
            Poll::Pending
        }
    }

    pub fn write(&self, val: u64) {
        let mut state = self.state.lock().unwrap();
        state.inc(val);
    }

    pub fn read(&self, waker: &Waker) -> Poll<u64> {
        let mut state = self.state.lock().unwrap();
        state.add_waker(waker);
        match state.dec() {
            0 => Poll::Pending,
            res => Poll::Ready(res),
        }
    }

    pub fn try_read(&self) -> Option<u64> {
        let mut state = self.state.lock().unwrap();
        match state.dec() {
            0 => None,
            res => Some(res),
        }
    }

    pub fn reset(&self) {
        let mut state = self.state.lock().unwrap();
        state.last_poll = u64::MAX;
    }

    pub fn set_interest_handler(&self, handler: Box<dyn InterestHandler>) {
        let mut state = self.state.lock().unwrap();
        state.interest_handler.replace(handler);
    }

    pub fn remove_interest_handler(&self) -> Option<Box<dyn InterestHandler>> {
        let mut state = self.state.lock().unwrap();
        state.interest_handler.take()
    }
}
