use std::sync::{Condvar, Mutex};
use std::time::Duration;

pub struct StopWait {
    lock: Mutex<()>,
    condvar: Condvar,
}

impl StopWait {
    pub fn new() -> Self {
        Self {
            lock: Mutex::new(()),
            condvar: Condvar::new(),
        }
    }

    pub fn wait_timeout(&self, duration: Duration) {
        let guard = self.lock.lock().unwrap();
        let _ = self.condvar.wait_timeout(guard, duration).unwrap();
    }

    pub fn notify_all(&self) {
        self.condvar.notify_all();
    }
}
