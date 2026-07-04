use std::sync::{Mutex, MutexGuard, PoisonError};

/// Lock a mutex, recovering the guard even if a previous holder panicked and
/// poisoned it. A poisoned shared mutex must never cascade into a `.unwrap()`
/// panic on the main thread: on Windows that unwinds across the WebView2 FFI
/// boundary and aborts the whole app. Recovering the guard degrades to
/// "carry on with the last value" instead.
pub fn lock_ignoring_poison<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn recovers_the_guard_after_a_poisoning_panic() {
        // Regression: a poisoned PanelOffset used to reach `.lock().unwrap()`
        // on the main thread and abort the process. The helper must return
        // the last-written value instead of panicking.
        let m = Arc::new(Mutex::new(42));
        let m2 = Arc::clone(&m);
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("poison the mutex while holding the lock");
        })
        .join();
        assert!(m.lock().is_err(), "precondition: the mutex is poisoned");
        assert_eq!(*lock_ignoring_poison(&m), 42);
    }
}
