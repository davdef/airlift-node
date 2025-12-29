use std::sync::{
    Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError,
};
use std::time::{Duration, Instant};

const LOCK_RETRY_DELAY: Duration = Duration::from_micros(200);

fn log_poisoned(lock_type: &str, context: &str) {
    log::error!("{} lock poisoned in {}", lock_type, context);
}

/// Lock a mutex, ignoring poisoning (but logging it).
pub fn lock_mutex<'a, T>(
    mutex: &'a Mutex<T>,
    context: &str,
) -> MutexGuard<'a, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            log_poisoned("Mutex", context);
            poisoned.into_inner()
        }
    }
}

/// Try-lock a mutex with timeout.
pub fn lock_mutex_with_timeout<'a, T>(
    mutex: &'a Mutex<T>,
    context: &str,
    timeout: Duration,
) -> Option<MutexGuard<'a, T>> {
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

/// Read-lock an RwLock, ignoring poisoning (but logging it).
pub fn lock_rwlock_read<'a, T>(
    lock: &'a RwLock<T>,
    context: &str,
) -> RwLockReadGuard<'a, T> {
    match lock.read() {
        Ok(guard) => guard,
        Err(poisoned) => {
            log_poisoned("RwLock(read)", context);
            poisoned.into_inner()
        }
    }
}

/// Write-lock an RwLock, ignoring poisoning (but logging it).
pub fn lock_rwlock_write<'a, T>(
    lock: &'a RwLock<T>,
    context: &str,
) -> RwLockWriteGuard<'a, T> {
    match lock.write() {
        Ok(guard) => guard,
        Err(poisoned) => {
            log_poisoned("RwLock(write)", context);
            poisoned.into_inner()
        }
    }
}

/// Try read-lock an RwLock with timeout.
pub fn lock_rwlock_read_with_timeout<'a, T>(
    lock: &'a RwLock<T>,
    context: &str,
    timeout: Duration,
) -> Option<RwLockReadGuard<'a, T>> {
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

/// Try write-lock an RwLock with timeout.
pub fn lock_rwlock_write_with_timeout<'a, T>(
    lock: &'a RwLock<T>,
    context: &str,
    timeout: Duration,
) -> Option<RwLockWriteGuard<'a, T>> {
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
