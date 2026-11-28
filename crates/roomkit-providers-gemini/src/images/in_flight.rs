//! An image attempt while its interaction is in flight, and what it tells
//! its callback when it is dropped then.

use roomkit_ai::image::{ImageAttempt, ImageAttemptStatus, ImageProgressCallback};

/// An attempt while its interaction is in flight: an attempt dropped then,
/// the call cancelled, tells its callback it is unknown, as Python's
/// provider does from its `CancelledError` (RFC §25.2): the vendor may
/// still bill it. The callback is told on the runtime the attempt ran on,
/// which a dropped future cannot await.
pub(crate) struct InFlight {
    attempt: ImageAttempt,
    callback: Option<ImageProgressCallback>,
}

/// What a cancelled attempt says, as Python's.
const CANCELLED: &str = "Cancelled locally; the provider may still bill this request";

impl InFlight {
    pub(crate) fn new(attempt: ImageAttempt, callback: Option<&ImageProgressCallback>) -> Self {
        Self {
            attempt,
            callback: callback.cloned(),
        }
    }

    /// The attempt, its interaction settled: nothing to tell on a drop.
    pub(crate) fn settled(mut self) -> ImageAttempt {
        self.callback = None;
        std::mem::take(&mut self.attempt)
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        let Some(callback) = self.callback.take() else {
            return;
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let mut attempt = std::mem::take(&mut self.attempt);
        attempt.status = ImageAttemptStatus::Unknown;
        attempt.error = Some(CANCELLED.to_owned());
        // The callback's failure has no caller left to fail.
        drop(runtime.spawn(async move { callback(attempt).await }));
    }
}
