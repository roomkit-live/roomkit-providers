//! The bridge between Python's event loops and the threads the calls run on.
//!
//! A call returns an asyncio future of the loop that made it. Its work runs
//! on the bridge's own runtime, and what it gives is handed back by one
//! thread, the only one of the bridge that ever takes the interpreter's
//! lock: it settles the future on its loop and lets go of every Python
//! reference the call held, while it holds the lock. No worker thread holds
//! a Python reference it could drop without the lock, so nothing here makes
//! the interpreter keep a list of references to release later, which is
//! guarded by a lock a forked child could find taken.
//!
//! That thread hands every call that ended while it waited for the lock
//! under one hold of it. A panic while one call is handed fails that call
//! and no other: the thread goes on. Should the thread end all the same, a
//! call made afterwards is refused at once, since nothing would ever
//! complete it.
//!
//! A call is stopped by dropping its work, which drops its request and
//! closes its connection: when the Python future it completes is cancelled,
//! and when the interpreter exits (every call runs under the bridge's
//! token, which the exit hook cancels before it waits). A call whose loop
//! was closed is not stopped: it runs to its end, and what it gives is
//! dropped.
//!
//! The runtime is small and fixed: the work of a call is waiting on a
//! socket and reading what comes, which a few threads do for hundreds of
//! calls. Its blocking threads serve the lookups of names, and nothing
//! else.

use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Condvar, LazyLock, Mutex, MutexGuard, OnceLock, PoisonError};
use std::task::{Context, Poll};

use pyo3::IntoPyObjectExt;
use pyo3::exceptions::PyRuntimeError;
use pyo3::intern;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::PyCFunction;
use tokio::runtime::{Builder, Runtime};
use tokio_util::sync::CancellationToken;

use super::errors::Failure;
use super::fork::{self, Kept};

/// The threads that run the calls: each waits on sockets, reads what comes
/// and writes it as JSON. Four serve two hundred streams at once faster
/// than a thread a core did with a pool of blocking threads beside, and
/// two serve them a third slower.
const WORKERS: usize = 4;

/// The most threads that look names up at once: a lookup blocks its thread,
/// and the ones past these wait their turn.
const LOOKUPS: usize = 4;

/// The most completions handed to Python under one hold of its lock.
const HANDED_AT_ONCE: usize = 256;

#[derive(Debug, Default)]
struct Activity {
    closing: bool,
    count: usize,
}

#[derive(Debug, Default)]
struct Bridge {
    activity: Mutex<Activity>,
    idle: Condvar,
    cancellation: CancellationToken,
}

static BRIDGE: LazyLock<Bridge> = LazyLock::new(Bridge::default);

fn activity() -> MutexGuard<'static, Activity> {
    BRIDGE
        .activity
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// What a call is refused with once the interpreter is exiting.
fn shutting_down() -> PyErr {
    PyRuntimeError::new_err("the provider bridge is shutting down")
}

/// What a call is refused with when the bridge has no thread to run it on.
fn no_threads() -> PyErr {
    PyRuntimeError::new_err("the provider bridge could not start its threads")
}

/// What a call is refused with once the thread that hands calls to Python
/// has ended: nothing would complete the call.
fn no_handing() -> PyErr {
    PyRuntimeError::new_err(
        "the provider bridge lost the thread that hands its calls to Python: no call can \
         complete any more in this process",
    )
}

/// The runtime the calls run on, built at the first call.
fn runtime() -> PyResult<&'static Runtime> {
    static RUNTIME: OnceLock<Option<Runtime>> = OnceLock::new();
    let built = RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .worker_threads(WORKERS)
            .max_blocking_threads(LOOKUPS)
            .thread_name("roomkit-providers")
            .enable_all()
            .build()
            .ok()
    });
    built.as_ref().ok_or_else(no_threads)
}

/// One piece of work under way: the exit hook waits until none is left.
#[derive(Debug)]
struct Ticket;

impl Ticket {
    fn enter() -> PyResult<Self> {
        fork::ensure_unforked()?;
        let mut state = activity();
        if state.closing {
            return Err(shutting_down());
        }
        // Before the count or the runtime changes: a child forked after
        // either one copies this mark and refuses native work.
        fork::mark_started();
        state.count = state.count.saturating_add(1);
        Ok(Self)
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        let mut state = activity();
        state.count = state.count.saturating_sub(1);
        if state.count == 0 {
            BRIDGE.idle.notify_all();
        }
    }
}

/// A piece of work whose panic is caught where it is polled: it ends as
/// work that failed, and the thread that ran it goes on.
struct Caught<F>(Pin<Box<F>>);

impl<F: Future> Future for Caught<F> {
    type Output = Option<F::Output>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let work = &mut self.get_mut().0;
        // The work is not polled again once it panicked.
        match std::panic::catch_unwind(AssertUnwindSafe(|| work.as_mut().poll(cx))) {
            Ok(Poll::Ready(given)) => Poll::Ready(Some(given)),
            Ok(Poll::Pending) => Poll::Pending,
            Err(_) => Poll::Ready(None),
        }
    }
}

/// `work`, its panic caught: `None` once it panicked.
pub(super) fn caught<F: Future>(work: F) -> impl Future<Output = Option<F::Output>> {
    Caught(Box::pin(work))
}

/// What a call gave, made the Python value or the exception it is under
/// the interpreter's lock.
type Outcome = Box<dyn FnOnce(Python<'_>) -> PyResult<Py<PyAny>> + Send>;

/// The end of one call, on its way to Python: the future to settle, on its
/// loop, with what the call gave. `None` for a call that was stopped, which
/// has nothing to give: its references are let go of, and nothing more.
struct Completion {
    event_loop: Py<PyAny>,
    future: Py<PyAny>,
    outcome: Option<Outcome>,
    /// The label of the provider the call was made on.
    label: String,
    /// Released last, once Python holds what the call gave.
    ticket: Ticket,
}

/// Settles `future` on `event_loop` with what `outcome` makes, under the
/// interpreter's lock.
fn settle(py: Python<'_>, event_loop: &Py<PyAny>, future: &Py<PyAny>, outcome: Outcome) {
    #[cfg(feature = "testing")]
    testing::panic_if_asked();
    let (value, error) = match outcome(py) {
        Ok(value) => (value, py.None()),
        Err(error) => (py.None(), error.into_value(py).into_any()),
    };
    if let Some(settle) = SETTLE.get(py) {
        let settling = (settle, future, value, error);
        // A loop that was closed takes nothing: what the call gave is
        // dropped.
        drop(
            event_loop
                .bind(py)
                .call_method1(intern!(py, "call_soon_threadsafe"), settling),
        );
    }
}

impl Completion {
    /// Settles the call's future on its loop, under the interpreter's lock,
    /// and lets go of the call's references there. A panic on the way is
    /// the call's failure: the call fails as a provider's work that
    /// panicked does, and the thread that hands the calls goes on.
    fn hand(self, py: Python<'_>) {
        let Self {
            event_loop,
            future,
            outcome,
            label,
            ticket,
        } = self;
        if let Some(outcome) = outcome {
            let handed = std::panic::catch_unwind(AssertUnwindSafe(|| {
                settle(py, &event_loop, &future, outcome);
            }));
            if handed.is_err() {
                let failed: Outcome = Box::new(move |py| Err(Failure::panicked(&label).raised(py)));
                // Should this panic too, the call is left unsettled: there
                // is nothing more to try.
                drop(std::panic::catch_unwind(AssertUnwindSafe(|| {
                    settle(py, &event_loop, &future, failed);
                })));
            }
        }
        drop((event_loop, future));
        drop(ticket);
    }
}

/// Whether the thread that hands completions to Python is running.
static HANDING: AtomicBool = AtomicBool::new(false);

/// Says the handing thread ended, however it ended.
struct Handing;

impl Drop for Handing {
    fn drop(&mut self) {
        HANDING.store(false, Ordering::Release);
    }
}

/// The one thread that hands completions to Python.
fn hand_over(completions: &Receiver<Completion>) {
    let _handing = Handing;
    while let Ok(first) = completions.recv() {
        Python::attach(|py| {
            first.hand(py);
            // What ended meanwhile, under the same hold of the lock.
            for _ in 1..HANDED_AT_ONCE {
                match completions.try_recv() {
                    Ok(next) => next.hand(py),
                    Err(_) => break,
                }
            }
        });
        #[cfg(feature = "testing")]
        if testing::ends_now() {
            return;
        }
    }
}

/// Where a worker leaves the end of a call, for the thread that hands it
/// to Python.
fn completions() -> PyResult<&'static Sender<Completion>> {
    static COMPLETIONS: OnceLock<Option<Sender<Completion>>> = OnceLock::new();
    let started = COMPLETIONS.get_or_init(|| {
        let (sender, receiver) = mpsc::channel();
        let handing = std::thread::Builder::new().name("roomkit-providers-python".to_owned());
        HANDING.store(true, Ordering::Release);
        let spawned = handing.spawn(move || hand_over(&receiver));
        if spawned.is_err() {
            HANDING.store(false, Ordering::Release);
        }
        spawned.ok().map(|_| sender)
    });
    let completions = started.as_ref().ok_or_else(no_threads)?;
    // A call made once the thread has ended would wait for good.
    if HANDING.load(Ordering::Acquire) {
        Ok(completions)
    } else {
        Err(no_handing())
    }
}

/// The function that settles a call's future on its loop.
static SETTLE: PyOnceLock<Py<PyCFunction>> = PyOnceLock::new();

/// Settles `future` with `value`, or with `error` when there is one, unless
/// it is already done: a future that was cancelled takes nothing.
#[pyfunction]
fn _settle(future: &Bound<'_, PyAny>, value: Py<PyAny>, error: Py<PyAny>) -> PyResult<()> {
    let py = future.py();
    if future.call_method0(intern!(py, "done"))?.is_truthy()? {
        return Ok(());
    }
    if error.is_none(py) {
        future.call_method1(intern!(py, "set_result"), (value,))?;
    } else {
        future.call_method1(intern!(py, "set_exception"), (error,))?;
    }
    Ok(())
}

/// Stops a call when the future it completes is cancelled.
#[pyclass(name = "_Stops", frozen, module = "roomkit_providers._native")]
struct Stops {
    stopped: Kept<CancellationToken>,
}

#[pymethods]
impl Stops {
    fn __call__(&self, future: &Bound<'_, PyAny>) -> PyResult<()> {
        let cancelled = future.call_method0(intern!(future.py(), "cancelled"))?;
        if cancelled.is_truthy()?
            && let Some(stopped) = self.stopped.get()
        {
            stopped.cancel();
        }
        Ok(())
    }
}

/// The awaitable of one call: `work` on the bridge's runtime, stopped when
/// the awaitable is cancelled and when the interpreter exits. Stopping it
/// drops `work`, and with it the request it holds. Work that panics fails
/// as a provider's failure.
pub(super) fn call<'py, F, T>(py: Python<'py>, label: &str, work: F) -> PyResult<Bound<'py, PyAny>>
where
    F: Future<Output = Result<T, Failure>> + Send + 'static,
    T: for<'a> IntoPyObject<'a> + Send + 'static,
{
    let ticket = Ticket::enter()?;
    let (runtime, completions) = (runtime()?, completions()?);
    let asyncio = py.import(intern!(py, "asyncio"))?;
    let event_loop = asyncio.call_method0(intern!(py, "get_running_loop"))?;
    let future = event_loop.call_method0(intern!(py, "create_future"))?;
    let stopped = CancellationToken::new();
    let stops = Stops {
        stopped: Kept::new(stopped.clone()),
    };
    future.call_method1(intern!(py, "add_done_callback"), (stops,))?;
    let exiting = BRIDGE.cancellation.child_token();
    let (event_loop, held) = (event_loop.unbind(), future.clone().unbind());
    let label = label.to_owned();
    drop(runtime.spawn(async move {
        let ended = stopped
            .run_until_cancelled(exiting.run_until_cancelled(caught(work)))
            .await;
        let outcome: Option<Outcome> = match ended {
            // The awaitable was cancelled: nothing awaits what came.
            None => None,
            // The exit stops what the work waits on, and the work may fail
            // of that before it is seen stopped: it failed of the exit.
            Some(None) => Some(Box::new(|_| Err(shutting_down()))),
            Some(Some(Some(Err(_)))) if exiting.is_cancelled() => {
                Some(Box::new(|_| Err(shutting_down())))
            }
            Some(Some(None)) => {
                let label = label.clone();
                Some(Box::new(
                    move |py| Err(Failure::panicked(&label).raised(py)),
                ))
            }
            Some(Some(Some(Err(failure)))) => Some(Box::new(|py| Err(failure.raised(py)))),
            Some(Some(Some(Ok(given)))) => Some(Box::new(|py| given.into_py_any(py))),
        };
        let completion = Completion {
            event_loop,
            future: held,
            outcome,
            label,
            ticket,
        };
        // The thread that hands completions over lives as long as the
        // process. Should it have ended, this worker hands its own call,
        // under the interpreter's lock as that thread would: the call is
        // never left unsettled, nor its references let go of off the lock.
        if let Err(mpsc::SendError(unhanded)) = completions.send(completion) {
            Python::attach(|py| unhanded.hand(py));
        }
    }));
    Ok(future)
}

/// Runs `work` on the bridge's runtime with no awaitable to complete: the
/// reading of a stream between two crossings. It runs until it ends or the
/// interpreter exits, which drops it.
pub(super) fn spawn<F>(work: F) -> PyResult<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let ticket = Ticket::enter()?;
    let exiting = BRIDGE.cancellation.child_token();
    drop(runtime()?.spawn(async move {
        // Stopped or ended, the work is over either way.
        let _: Option<()> = exiting.run_until_cancelled(work).await;
        drop(ticket);
    }));
    Ok(())
}

/// Private diagnostic: the calls and the readings of streams under way.
///
/// A forked child runs none: the count it copied belongs to its parent.
#[pyfunction]
fn _activity() -> usize {
    if fork::is_forked() {
        return 0;
    }
    activity().count
}

/// Private diagnostic: whether the thread that hands calls to Python is
/// running. False before the first call, which starts it.
#[pyfunction]
fn _handing() -> bool {
    HANDING.load(Ordering::Acquire)
}

/// What the tests of the bridge ask of it, in a module built for them: a
/// panic while a call is handed to Python, and the end of the thread that
/// hands them.
#[cfg(feature = "testing")]
mod testing {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use pyo3::prelude::*;

    /// How many of the next calls panic as they are handed to Python.
    static PANICS: AtomicUsize = AtomicUsize::new(0);

    /// Whether the handing thread ends once it has handed what it holds.
    static ENDS: AtomicBool = AtomicBool::new(false);

    /// Panics if a test asked for a panic of the next call handed.
    #[expect(clippy::panic, reason = "the defect the tests of the bridge script")]
    pub(super) fn panic_if_asked() {
        let mut left = PANICS.load(Ordering::Acquire);
        while left > 0 {
            let fewer = left.saturating_sub(1);
            match PANICS.compare_exchange(left, fewer, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => panic!("the bridge panics while it hands a call to Python"),
                Err(now) => left = now,
            }
        }
    }

    /// Whether a test asked the handing thread to end.
    pub(super) fn ends_now() -> bool {
        ENDS.load(Ordering::Acquire)
    }

    /// The next `count` calls panic as they are handed to Python.
    #[pyfunction]
    fn _handing_panics(count: usize) {
        PANICS.store(count, Ordering::Release);
    }

    /// The thread that hands calls to Python ends once it has handed the
    /// next call that ends.
    #[pyfunction]
    fn _handing_ends() {
        ENDS.store(true, Ordering::Release);
    }

    pub(super) fn install(module: &Bound<'_, PyModule>) -> PyResult<()> {
        module.add_function(wrap_pyfunction!(_handing_panics, module)?)?;
        module.add_function(wrap_pyfunction!(_handing_ends, module)?)
    }
}

/// Private diagnostic: the threads the bridge runs its calls on, and the
/// most it starts to look names up.
#[pyfunction]
fn _threads() -> (usize, usize) {
    (WORKERS, LOOKUPS)
}

/// The exit hook: stops every call, then waits until each has ended, the
/// interpreter's lock released meanwhile, since handing a call's end to
/// Python takes it. Returns the count of what was under way.
///
/// A forked child returns at once: no thread there would ever finish the
/// parent's calls, and its copied locks may be held by threads it lacks.
#[pyfunction]
fn _shutdown(py: Python<'_>) -> usize {
    if fork::is_forked() {
        return 0;
    }
    let count = {
        let mut state = activity();
        state.closing = true;
        state.count
    };
    BRIDGE.cancellation.cancel();
    py.detach(|| {
        let mut state = activity();
        while state.count != 0 {
            state = BRIDGE
                .idle
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    });
    count
}

pub(super) fn install(module: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = module.py();
    module.add_function(wrap_pyfunction!(_activity, module)?)?;
    module.add_function(wrap_pyfunction!(_threads, module)?)?;
    module.add_function(wrap_pyfunction!(_handing, module)?)?;
    #[cfg(feature = "testing")]
    testing::install(module)?;
    module.add_function(wrap_pyfunction!(_shutdown, module)?)?;
    module.add_class::<Stops>()?;
    let settle = wrap_pyfunction!(_settle, module)?;
    drop(SETTLE.set(py, settle.unbind()));
    Ok(())
}
