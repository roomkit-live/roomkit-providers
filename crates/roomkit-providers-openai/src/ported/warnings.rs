//! The warnings the provider logs while a test runs, for a test that checks
//! one is logged, or that none is.
//!
//! One subscriber is the test binary's global default, installed by the
//! first capture: it keeps the lines of the thread that captures, while its
//! guard lives. A subscriber set for one thread would miss events: the
//! interest a callsite caches is computed again whenever a subscriber comes
//! or goes, and races the same callsite firing in another test's thread.

use std::cell::RefCell;
use std::fmt::{Debug, Write as _};
use std::sync::{Arc, Mutex, Once, PoisonError};

use tracing::field::{Field, Visit};
use tracing::level_filters::LevelFilter;
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Event, Level, Metadata, Subscriber};

thread_local! {
    /// Where the warnings of this thread go while it captures.
    static CAPTURING: RefCell<Option<Warnings>> = const { RefCell::new(None) };
}

/// The warnings and the errors logged on this thread while the guard lives:
/// each as its fields, `name=value`, a space after each.
#[derive(Clone)]
pub(crate) struct Warnings {
    lines: Arc<Mutex<Vec<String>>>,
}

/// Captures this thread's warnings while it lives.
pub(crate) struct Capturing(());

impl Warnings {
    /// Starts capturing on this thread, until the guard is dropped.
    pub(crate) fn capture() -> (Self, Capturing) {
        static INSTALLED: Once = Once::new();
        INSTALLED.call_once(|| {
            let installed = tracing::subscriber::set_global_default(Sink);
            assert!(installed.is_ok(), "another subscriber is the default");
        });
        let warnings = Self {
            lines: Arc::default(),
        };
        CAPTURING.with(|capturing| *capturing.borrow_mut() = Some(warnings.clone()));
        (warnings, Capturing(()))
    }

    /// The lines that contain `text`.
    pub(crate) fn containing(&self, text: &str) -> Vec<String> {
        let lines = self.lines.lock().unwrap_or_else(PoisonError::into_inner);
        let found = lines.iter().filter(|line| line.contains(text));
        found.cloned().collect()
    }
}

impl Drop for Capturing {
    fn drop(&mut self) {
        CAPTURING.with(|capturing| capturing.borrow_mut().take());
    }
}

/// One event written out, field by field.
struct Line<'line>(&'line mut String);

impl Visit for Line<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        write!(self.0, "{}={value} ", field.name()).ok();
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        write!(self.0, "{}={value:?} ", field.name()).ok();
    }
}

/// The binary's subscriber: every warning and error, kept by the thread
/// that logs it when it captures.
struct Sink;

impl Subscriber for Sink {
    fn register_callsite(&self, callsite: &'static Metadata<'static>) -> Interest {
        if *callsite.level() <= Level::WARN {
            Interest::always()
        } else {
            Interest::never()
        }
    }

    fn max_level_hint(&self) -> Option<LevelFilter> {
        Some(LevelFilter::WARN)
    }

    fn enabled(&self, callsite: &Metadata<'_>) -> bool {
        *callsite.level() <= Level::WARN
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        CAPTURING.with(|capturing| {
            let Some(warnings) = &*capturing.borrow() else {
                return;
            };
            let mut line = String::new();
            event.record(&mut Line(&mut line));
            let mut lines = warnings
                .lines
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            lines.push(line);
        });
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}
