//! Refuse native work in a child forked after the bridge started.
//!
//! `fork()` copies the bridge's state but not the Tokio runtime's threads: a
//! call in such a child would never complete, and its exit hook would wait for
//! calls that only the parent runs. A child forked before the first native
//! call builds its own runtime on first use, like any other process.
//!
//! The bridge records the process that started it and compares that record
//! with the current process. Nothing runs inside `fork()`.
//!
//! The package refuses such a child in Python, before it enters this
//! module. What a child still reaches here must take no lock a thread of
//! its parent may have held at the fork: it lets go of what it inherited
//! without releasing it ([`Kept`]), and its exit waits for nothing.

use std::sync::atomic::{AtomicU32, Ordering};

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

/// The process that started the runtime's threads; zero before the first call.
static STARTED_BY: AtomicU32 = AtomicU32::new(0);

/// Record that this process may now own runtime threads.
pub(super) fn mark_started() {
    if STARTED_BY.load(Ordering::Relaxed) == 0 {
        STARTED_BY.store(std::process::id(), Ordering::Relaxed);
    }
}

/// Whether this process was forked after its parent started native work.
pub(super) fn is_forked() -> bool {
    let started_by = STARTED_BY.load(Ordering::Relaxed);
    started_by != 0 && started_by != std::process::id()
}

/// What an object Python holds keeps of the bridge's: released with the
/// object, but in a forked child, which leaves it as it is. Releasing it
/// there could take a lock a thread of the parent held at the fork, and
/// nothing in the child would ever let go of that lock.
#[derive(Debug)]
pub(super) struct Kept<T>(Option<T>);

impl<T> Kept<T> {
    pub(super) const fn new(value: T) -> Self {
        Self(Some(value))
    }

    /// What is kept: always there while the object lives.
    pub(super) const fn get(&self) -> Option<&T> {
        self.0.as_ref()
    }
}

impl<T> Drop for Kept<T> {
    fn drop(&mut self) {
        if is_forked() {
            std::mem::forget(self.0.take());
        }
    }
}

/// Fail at once, without touching state that a parent thread may have held.
pub(super) fn ensure_unforked() -> PyResult<()> {
    if is_forked() {
        return Err(PyRuntimeError::new_err(
            "a provider cannot be called in a process forked after its parent's first native \
             call; make native calls only after forking, or use the spawn or forkserver \
             start method",
        ));
    }
    Ok(())
}
