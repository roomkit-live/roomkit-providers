//! A key a host lends one request, kept out of every failure.
//!
//! Beside the replay of its recorded cases, a provider is asked each
//! generation of its fixture once more, the request lending a key, by a
//! fake that refuses it with a 401 whose message repeats that key. The
//! fake refuses every request it is sent so, up to [`REFUSALS`]: a provider
//! that asks a server of its own first, one that tells it where to send the
//! request, gets the 401 there too. The failure must say what the 401 says
//! without the key, in its message as in its `Debug` form, whether the
//! provider sends that key or not: a host that lends a key must find it in
//! no failure.
//!
//! A provider whose failures say nothing of what a server answered
//! ([`Replays::SAYS_REFUSALS`]), as one whose identity asks its token
//! first, is held to the key alone.
//!
//! [`replayed_or_named`](super::replayed_or_named) asks it of every case it
//! replays, so a provider that replays its fixture is held to it without
//! writing anything.

use std::sync::Arc;

use roomkit_ai::{API_KEY_METADATA_KEY, ProviderError};

use super::fake::{Door, Fake};
use super::{Case, Replays, drained};
use crate::Pace;
use crate::failure::{MARK, SECRET_MIN};

/// The key lent to the request: a placeholder, long enough for an exit to
/// search for it, and unlike the key of any configuration.
pub(super) const LENT_KEY: &str = "placeholder-lent-to-one-request";

// An exit searches for no secret shorter than this.
const _: () = assert!(LENT_KEY.len() >= SECRET_MIN);

/// What the 401 says before the key it repeats.
const SAID: &str = "the key lent to the request is refused: ";

/// How many requests the fake refuses: past them, it answers 599.
const REFUSALS: usize = 4;

/// What the fake's 401 says: [`SAID`], then the key.
fn said_with_the_key() -> String {
    format!("{SAID}{LENT_KEY}")
}

/// Asks the provider of `case` the generation of the case again, its
/// request lending [`LENT_KEY`], of a fake behind `door` that refuses it
/// with a 401 whose message repeats the key.
///
/// `Ok(true)` when the failure says what the 401 says without the key;
/// `Ok(false)` when nothing was asked, by a provider that is not built or
/// that refuses the request before it sends it, and no failure holds the
/// key; and for a case that lists the models, which no request is lent a
/// key for.
///
/// # Errors
///
/// What is wrong: a failure that repeats the key, in its message or in its
/// `Debug` form; a failure that says nothing of the 401, which then shows
/// nothing; or no failure.
pub(super) async fn kept_out<P: Replays>(case: &Case, door: &Door) -> Result<bool, String> {
    if case.call == "list_models" {
        return Ok(false);
    }
    let (Ok(mut request), Ok(mut provider)) = (case.read_request(), P::built(&case.config)) else {
        return Ok(false);
    };
    assert_ne!(case.key(), LENT_KEY, "{}", case.name);
    let lent = (API_KEY_METADATA_KEY.to_owned(), LENT_KEY.into());
    request.metadata.extend([lent]);
    let refusals = (0..REFUSALS).map(|_| P::key_refused(&said_with_the_key()));
    let fake = Fake::behind(door, refusals.collect());
    provider.point_at(fake.address);
    // Should the provider try again, it takes no wait.
    provider.paced(Pace {
        now: Arc::new(|| 0.0),
        wait: Arc::new(|_| Box::pin(std::future::ready(()))),
    });
    let failure = match case.call.as_str() {
        "generate" => provider.generate(&request).await.err(),
        "generate_structured_stream" => {
            let events = provider.generate_structured_stream(&request);
            drained(events).await.1
        }
        "generate_stream" => drained(provider.generate_stream(&request)).await.1,
        _ => return Ok(false),
    };
    judged(
        failure.as_ref(),
        P::SAYS_REFUSALS && !fake.seen().is_empty(),
    )
}

/// How `failure` stands, the failure of a request that lent [`LENT_KEY`]
/// and that the fake refused when it was `asked`: see [`kept_out`].
fn judged(failure: Option<&ProviderError>, asked: bool) -> Result<bool, String> {
    let Some(error) = failure else {
        return Err("no failure, where the fake refuses the key lent to the request".to_owned());
    };
    let shown = format!("{error} {error:?}");
    if shown.contains(LENT_KEY) {
        return Err(format!(
            "the failure repeats the key lent to the request: {shown}"
        ));
    }
    if !asked {
        return Ok(false);
    }
    if error.message.contains(&format!("{SAID}{MARK}")) {
        Ok(true)
    } else {
        Err(format!(
            "the failure says nothing of the 401 that repeats the key lent to the request, \
             which then shows nothing: {:?}",
            error.message
        ))
    }
}

#[cfg(test)]
mod tests;
