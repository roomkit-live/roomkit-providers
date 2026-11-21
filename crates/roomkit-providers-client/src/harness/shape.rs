//! A fixture of recorded cases held to its shape, whatever the provider it
//! was captured on: every member of a case is read, strictly, the requests
//! and the results with the contract's types, and each case is required to
//! say of itself what a replay rests on.
//!
//! A body read in several reads is a list of chunks, each the bytes of one
//! read: a JSON text where the bytes are UTF-8, else an object whose `hex`
//! member holds them in hexadecimal, as a read that ends inside a character
//! is. A case that fails says under `compare` what a replay reproduces of
//! the failure and whose words its message holds, and a case that pins a
//! behaviour of Python that is not settled says so under `questioned`: a
//! possible defect, or a difference the maintainer decides.
//!
//! A provider crate reads its fixture with [`Fixture::read`], states what
//! is its own in a [`Shape`], and runs each check in a test of its own
//! beside the checks of its configuration and of its catalogue.

mod checks;

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

/// What a fixture of one provider states of itself, beside what every
/// fixture holds.
#[derive(Clone, Copy, Debug)]
pub struct Shape<'s> {
    /// The label of the provider, which a `ProviderError` carries.
    pub provider: &'s str,
    /// The member of the fixture that holds the version of the vendor's
    /// SDK it was captured with.
    pub sdk_version: &'s str,
    /// What the note on the environment names: the variables of the
    /// vendor's SDK that are removed before a case runs.
    pub environment: &'s str,
    /// The notes the fixture must hold.
    pub notes: &'s [&'s str],
    /// The members of a case that are the provider's own: any other member
    /// the shape does not read is refused.
    pub own_members: &'s [&'s str],
    /// The key of every configuration.
    pub key: &'s str,
    /// The keys a request may carry for itself.
    pub request_keys: &'s [&'s str],
    /// The cases whose request the contract does not read, being stricter
    /// than Python's on what a request holds.
    pub unread: &'s [&'s str],
    /// The classes of the vendor's SDK a call may raise as they are.
    pub sdk_errors: &'s [&'s str],
    /// What the SDK says of a failure, word for word.
    pub sdk_words: &'s [&'s str],
    /// The classes of the SDK a failure whose words are the SDK's is
    /// raised from.
    pub sdk_causes: &'s [&'s str],
    /// How the SDK's own sentences start, where it raises an exception of
    /// no class of its own.
    pub sdk_sentences: &'s [&'s str],
    /// Whether the SDK raises an error a stream tells with Python's `repr`
    /// of the event's data, and with the data itself when it is no JSON.
    pub raises_an_event_s_data: bool,
    /// The longest wait an SDK takes before another try, in seconds.
    pub longest_wait: f64,
}

/// A fixture, every member of its cases read.
#[derive(Debug, Deserialize)]
pub struct Fixture {
    roomkit_version: String,
    roomkit_commit: Option<String>,
    roomkit_dirty: Option<bool>,
    pydantic_version: String,
    httpx2_version: String,
    python_version: String,
    clock: String,
    /// What the fixture says of how each of its parts is written.
    pub notes: BTreeMap<String, String>,
    questioned: Vec<String>,
    /// The cases, in the order the fixture writes them.
    pub cases: Vec<Case>,
    /// The members of the fixture that are a provider's own.
    #[serde(flatten)]
    pub others: BTreeMap<String, Value>,
}

/// One case, every member read.
#[derive(Debug, Deserialize)]
pub struct Case {
    /// The case's name, one of a kind in its fixture.
    pub name: String,
    /// What the case shows of its provider.
    pub kinds: Vec<String>,
    /// The call of the provider the case makes.
    pub call: String,
    #[serde(rename = "note")]
    _note: Option<String>,
    /// What the vendor's SDK decided of the case, when it did.
    pub sdk: Option<String>,
    /// The behaviour of Python the case pins that is not settled.
    pub questioned: Option<String>,
    /// The configuration of the provider.
    pub config: Value,
    /// What the provider read of its model's catalogue entry.
    pub catalogue: Option<Catalogue>,
    /// Whether the provider says its model takes images.
    pub supports_vision: Option<bool>,
    /// The request, in the contract's JSON form.
    pub request: Value,
    /// What the request carries under its key, which its JSON form masks.
    pub request_api_key: Option<Value>,
    /// The HTTP exchanges Python had.
    pub exchanges: Vec<Exchange>,
    /// What Python returned.
    pub expected: Expected,
    /// How a replay compares the failure of the case.
    pub compare: Option<Compare>,
    /// The ids Python minted for a call its server gave none.
    #[serde(default)]
    pub minted_call_ids: Vec<String>,
    /// The members that are the provider's own.
    #[serde(flatten)]
    pub own: BTreeMap<String, Value>,
}

/// What the provider read of its model's catalogue entry.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalogue {
    /// The model the entry is of.
    pub model: String,
    /// Whether the catalogue knows the model.
    pub known: bool,
    /// The tags of the entry among those the provider reads.
    pub tags: Vec<String>,
    /// The least reasoning effort the entry declares.
    pub reasoning_floor: Option<String>,
}

/// One HTTP exchange: what Python sent, what the fake answered, and the
/// wait the SDK took before its next try.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exchange {
    /// What Python sent.
    pub request: Sent,
    /// What the fake answered.
    pub response: Answered,
    /// The wait the SDK took before its next try.
    pub wait: Option<Wait>,
}

/// What Python sent.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sent {
    /// The method of the request.
    pub method: String,
    /// The whole URL it went to.
    pub url: String,
    /// The headers the fixture keeps, one that was not sent `None`.
    pub headers: BTreeMap<String, Option<String>>,
    /// The timeouts of the request, in seconds.
    pub timeouts: BTreeMap<String, f64>,
    /// The body, as JSON.
    pub body: Option<Value>,
}

/// What the fake answered: a status and headers with one body, or a failure
/// in place of an answer.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answered {
    /// The status of the answer.
    pub status: Option<u16>,
    /// The headers of the answer.
    pub headers: Option<BTreeMap<String, String>>,
    /// A body that is JSON.
    pub json: Option<Value>,
    /// A body that is a text.
    pub text: Option<String>,
    /// A body read in several reads.
    pub chunks: Option<Vec<Chunk>>,
    /// How the reads end.
    pub then: Option<String>,
    /// The failure in place of an answer.
    pub failure: Option<String>,
}

/// The bytes of one read: their text where they are UTF-8, else in
/// hexadecimal.
#[derive(Debug, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum Chunk {
    /// Bytes that are UTF-8, as the text they write.
    Text(String),
    /// Bytes that are none, in hexadecimal.
    Bytes {
        /// The bytes, two digits each.
        hex: String,
    },
}

impl Chunk {
    /// The bytes read, `None` for a `hex` member that is no hexadecimal.
    #[must_use]
    pub fn bytes(&self) -> Option<Vec<u8>> {
        let hex = match self {
            Self::Text(text) => return Some(text.clone().into_bytes()),
            Self::Bytes { hex } => hex.as_bytes(),
        };
        let digits = hex.chunks(2).map(|pair| std::str::from_utf8(pair).ok());
        digits
            .map(|pair| u8::from_str_radix(pair.filter(|pair| pair.len() == 2)?, 16).ok())
            .collect()
    }
}

/// The wait the SDK took before its next try.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wait {
    /// How long, in seconds.
    pub seconds: f64,
    /// Where it came from: the answer's headers, or the SDK's backoff.
    pub from: String,
}

/// What Python returned.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expected {
    /// The response of `generate`.
    pub response: Option<Value>,
    /// The text of `generate_stream`.
    pub text: Option<Vec<String>>,
    /// The events of `generate_structured_stream`.
    pub events: Option<Vec<Value>>,
    /// The models of `list_models`.
    pub models: Option<Vec<Value>>,
    /// The error Python raised.
    pub error: Option<Failure>,
}

/// The error Python raised: its class, what it says, and the attributes of
/// a `ProviderError`, which another class does not have.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Failure {
    /// The class of the error.
    pub class: String,
    /// Its first argument.
    pub message: String,
    /// What it reads as.
    pub text: String,
    /// The class of the error it was raised from.
    pub raised_from: Option<String>,
    /// Whether a new request may succeed.
    pub retryable: Option<bool>,
    /// The provider that failed.
    pub provider: Option<String>,
    /// The status the vendor answered.
    pub status_code: Option<u16>,
    /// Whether the request was past the model's context.
    pub context_overflow: Option<bool>,
    /// Whether the provider refused before any request.
    pub before_request: Option<bool>,
    /// Why a response schema failed.
    pub reason: Option<String>,
}

/// How a replay compares a failure: what it reproduces as written, and
/// whose words the message holds.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compare {
    /// What a replay reproduces as written.
    pub exactly: Vec<String>,
    /// Whose words the message holds.
    pub message: String,
}

/// How many failures a fixture holds, and of which kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Failures {
    /// The cases that fail.
    pub all: usize,
    /// Those whose message a replay compares.
    pub compared: usize,
    /// Those of a response schema.
    pub of_a_schema: usize,
}

#[cfg(test)]
mod tests;
