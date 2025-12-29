use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError};
use std::time::{Duration, Instant};

const LOCK_RETRY_DELAY: Duration = Duration::from_micros(200);

fn log_poisoned(lock_type: &str, context: &str) {
    log::error!("{} lock poisoned in {}", lock_type, context);
}

pub fn lock_mutex<T>(mutex: &Mutex<T>, context: &str) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            log_poisoned("Mutex", context);
            poisoned.into_inner()
        }
    }
}

pub fn lock_mutex_with_timeout<T>(
    mutex: &Mutex<T>,
    context: &str,
    timeout: Duration,
) -> Option<MutexGuard<'_, T>> {
    let start = Instant::now();
    loop {
        match mutex.try_lock() {
            Ok(guard) => return Some(guard),
            Err(TryLockError::Poisoned(poisoned)) => {
                log_poisoned("Mutex", context);
                return Some(poisoned.into_inner());
            }
            Err(TryLockError::WouldBlock) => {
                if start.elapsed() >= timeout {
                    log::warn!("Mutex lock timed out in {}", context);
                    return None;
                }
                std::thread::sleep(LOCK_RETRY_DELAY);
            }
        }
    }
}

pub fn lock_rwlock_read<T>(lock: &RwLock<T>, context: &str) -> RwLockReadGuard<'_, T> {
    match lock.read() {
        Ok(guard) => guard,
        Err(poisoned) => {
            log_poisoned("RwLock(read)", context);
            poisoned.into_inner()
        }
    }
}

pub fn lock_rwlock_write<T>(lock: &RwLock<T>, context: &str) -> RwLockWriteGuard<'_, T> {
    match lock.write() {
        Ok(guard) => guard,
        Err(poisoned) => {
            log_poisoned("RwLock(write)", context);
            poisoned.into_inner()
        }
    }
}

pub fn lock_rwlock_read_with_timeout<T>(
    lock: &RwLock<T>,
    context: &str,
    timeout: Duration,
) -> Option<RwLockReadGuard<'_, T>> {
    let start = Instant::now();
    loop {
        match lock.try_read() {
            Ok(guard) => return Some(guard),
            Err(TryLockError::Poisoned(poisoned)) => {
                log_poisoned("RwLock(read)", context);
                return Some(poisoned.into_inner());
            }
            Err(TryLockError::WouldBlock) => {
                if start.elapsed() >= timeout {
                    log::warn!("RwLock read timed out in {}", context);
                    return None;
                }
                std::thread::sleep(LOCK_RETRY_DELAY);
            }
        }
    }
}

pub fn lock_rwlock_write_with_timeout<T>(
    lock: &RwLock<T>,
    context: &str,
    timeout: Duration,
) -> Option<RwLockWriteGuard<'_, T>> {
    let start = Instant::now();
    loop {
        match lock.try_write() {
            Ok(guard) => return Some(guard),
            Err(TryLockError::Poisoned(poisoned)) => {
                log_poisoned("RwLock(write)", context);
                return Some(poisoned.into_inner());
            }
            Err(TryLockError::WouldBlock) => {
                if start.elapsed() >= timeout {
                    log::warn!("RwLock write timed out in {}", context);
                    return None;
                }
                std::thread::sleep(LOCK_RETRY_DELAY);
            }
        }
    }
}
