//! What the crate's tests run a future and read a stream with, without an
//! async runtime: nothing here waits on anything but its own reader, so a
//! pending future only needs polling again.

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use futures_core::Stream;

use crate::stream::next;

/// The polls after which a future still pending is taken for stuck.
const POLLS: usize = 100_000;

/// The output of `future`, polled on this thread until it is ready.
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    for _ in 0..POLLS {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
    }
    panic!("a future of a test is still pending after {POLLS} polls");
}

/// Every item of `stream`, in order.
pub(crate) fn collected<S: Stream + Unpin>(mut stream: S) -> Vec<S::Item> {
    block_on(async {
        let mut items = Vec::new();
        while let Some(item) = next(&mut stream).await {
            items.push(item);
        }
        items
    })
}
