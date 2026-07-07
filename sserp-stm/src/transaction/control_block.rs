use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, Thread};
use std::time::Duration;

pub struct ControlBlock {
    thread: Thread,
    blocked: AtomicBool,
    max_parked_time: Duration,
}

impl Default for ControlBlock {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlBlock {
    pub fn new() -> ControlBlock {
        ControlBlock {
            thread: thread::current(),
            blocked: AtomicBool::new(true),
            max_parked_time: Duration::from_secs(1),
        }
    }

    pub fn set_changed(&self) {
        if self.blocked.swap(false, Ordering::SeqCst) {
            self.thread.unpark();
        }
    }

    pub fn wait(&self) {
        while self.blocked.load(Ordering::SeqCst) {
            thread::park_timeout(self.max_parked_time);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test::{terminates, terminates_async};
    use std::sync::Arc;

    #[test]
    fn blocked() {
        let ctrl = ControlBlock::new();
        assert!(!terminates(100, move || ctrl.wait()));
    }

    #[test]
    fn wait_after_change() {
        let ctrl = ControlBlock::new();
        ctrl.set_changed();
        assert!(terminates(50, move || ctrl.wait()));
    }

    #[test]
    fn wait_after_multiple_changes() {
        let ctrl = ControlBlock::new();
        ctrl.set_changed();
        ctrl.set_changed();
        ctrl.set_changed();
        ctrl.set_changed();

        assert!(terminates(50, move || ctrl.wait()));
    }

    #[test]
    fn wait_threaded_wakeup() {
        let ctrl = Arc::new(ControlBlock::new());
        let ctrl2 = ctrl.clone();
        let terminated = terminates_async(500, move || ctrl.wait(), move || ctrl2.set_changed());

        assert!(terminated);
    }
}
