//! The work waiting for the workers: what the pointer needs first, then everything else in the order RustCut chose.

use std::collections::VecDeque;

use parking_lot::{Condvar, Mutex};
use rustcut_thumbs_protocol::Job;

#[derive(Default)]
struct State {
    front: VecDeque<Job>,
    back: VecDeque<Job>,
    running: usize,
    closed: bool,
}

/// `Idle` is reported once per time the queue runs dry.
#[derive(Default)]
pub struct Queue {
    state: Mutex<State>,
    wake: Condvar,
}

/// What a worker got: a job, or the end.
pub enum Next {
    Job(Job),
    Closed,
}

impl Queue {
    pub fn add(&self, jobs: Vec<Job>) {
        self.state.lock().back.extend(jobs);
        self.wake.notify_all();
    }

    /// Replaces the waiting front jobs: the pointer moved on, the old place does not count any more.
    pub fn front(&self, jobs: Vec<Job>) {
        let mut state = self.state.lock();
        state.front = jobs.into();
        drop(state);
        self.wake.notify_all();
    }

    pub fn clear(&self) {
        let mut state = self.state.lock();
        state.front.clear();
        state.back.clear();
    }

    pub fn close(&self) {
        self.state.lock().closed = true;
        self.wake.notify_all();
    }

    /// A front job waits, so a long span should let go.
    pub fn front_waiting(&self) -> bool {
        !self.state.lock().front.is_empty()
    }

    /// Puts the rest of an interrupted job back, first in the normal line.
    pub fn resume(&self, job: Job) {
        self.state.lock().back.push_front(job);
    }

    /// The next job; blocks while nothing waits.
    pub fn next(&self) -> Next {
        let mut state = self.state.lock();
        loop {
            if state.closed {
                return Next::Closed;
            }
            if let Some(job) = state.front.pop_front().or_else(|| state.back.pop_front()) {
                state.running += 1;
                return Next::Job(job);
            }
            self.wake.wait(&mut state);
        }
    }

    /// A worker finished its job. True when that was the last of everything.
    pub fn finished(&self) -> bool {
        let mut state = self.state.lock();
        state.running = state.running.saturating_sub(1);
        state.running == 0 && state.front.is_empty() && state.back.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(ms: u64) -> Job {
        Job::Key { ms }
    }

    fn take(queue: &Queue) -> Job {
        match queue.next() {
            Next::Job(job) => job,
            Next::Closed => panic!("closed"),
        }
    }

    #[test]
    fn the_newest_front_goes_first_and_replaces_the_older() {
        let queue = Queue::default();
        queue.add(vec![key(1), key(2)]);
        queue.front(vec![key(10)]);
        queue.front(vec![key(20)]);
        assert_eq!(take(&queue), key(20));
        assert_eq!(take(&queue), key(1));
    }

    #[test]
    fn idle_comes_with_the_last_finished_job() {
        let queue = Queue::default();
        queue.add(vec![key(1), key(2)]);
        take(&queue);
        take(&queue);
        assert!(!queue.finished());
        assert!(queue.finished());
    }

    #[test]
    fn an_interrupted_job_comes_back_before_the_others() {
        let queue = Queue::default();
        queue.add(vec![key(1)]);
        queue.resume(key(9));
        assert_eq!(take(&queue), key(9));
    }

    #[test]
    fn close_ends_the_workers() {
        let queue = Queue::default();
        queue.close();
        assert!(matches!(queue.next(), Next::Closed));
    }
}
