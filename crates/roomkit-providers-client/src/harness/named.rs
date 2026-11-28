//! The cases a provider answers otherwise than Python: each with the
//! reason, how its replay then differs from Python's, and exactly what the
//! provider does instead. The replay asserts both: it differs in what the
//! entry states and in nothing else, and the provider sends as many
//! requests and gives what the entry states.
//!
//! A provider crate lists its own, as [`Entry`] values in the three lists
//! of a [`Named`]. Here is what an entry is made of, how a replay is held
//! to it, and the data file a replay outside the crate reads the entries
//! from.

use std::collections::BTreeMap;

use roomkit_ai::{FinishReason, ProviderError, StreamDone, StreamEvent};

use roomkit_ai::image::ImageError;

use super::{Fixture, Outcome, Replayed, Replays, text_of};

/// What a failure says.
#[derive(Clone, Copy, Debug)]
pub enum Says {
    /// These words, the provider's own, and no other.
    Exactly(&'static str),
    /// A reader's words, which hold these.
    Holding(&'static str),
}

/// What the provider does where it does not do as Python.
#[derive(Clone, Copy, Debug)]
pub enum Does {
    /// It is not built, and says why.
    Unbuilt(&'static str),
    /// The call fails: with this status, retried or not, saying whether
    /// the context overflowed, in these words.
    Fails(Option<u16>, bool, Option<bool>, Says),
    /// The call is answered, whole or as a stream that ends without a
    /// failure: it says this text, stops for this reason, a word of the
    /// contract, and counts this usage.
    Answers(&'static str, FinishReason, &'static str),
    /// Nothing is asked: the contract, stricter than Python's on what a
    /// request holds, does not read the case's request, and says why.
    Unread(Says),
    /// The call fails before its request to the provider's endpoint, final
    /// and with no status, in these words: a host's source of tokens could
    /// give none, as an identity whose token was refused.
    FailsBefore(Says),
    /// It is built where Python's is not, and asks without the header that
    /// carries its key: the fake, which the case gives no answer, answers
    /// its 599.
    AsksWithoutAKey,
}

/// What the provider does of a named case: how many requests it sends, and
/// what the call gives.
#[derive(Clone, Copy, Debug)]
pub struct Instead {
    /// How many requests the provider sends.
    pub requests: usize,
    /// What the call gives.
    pub does: Does,
}

/// A failure after one request, final or not, in the provider's words.
#[must_use]
pub const fn fails(status: Option<u16>, retryable: bool, says: &'static str) -> Instead {
    let does = Does::Fails(status, retryable, None, Says::Exactly(says));
    Instead { requests: 1, does }
}

/// A named case as its entry states it: how its replay differs from
/// Python's, each difference by the words it starts with, and what the
/// provider does instead.
#[derive(Clone, Copy, Debug)]
pub(super) struct Stated {
    differs: &'static [&'static str],
    instead: Instead,
}

impl Stated {
    /// Whether the replay is what the entry states: `differing`, the
    /// differences found from Python, are the ones stated, each of them
    /// and no other, and `replayed` did what is stated.
    ///
    /// # Errors
    ///
    /// What is not as stated.
    pub(super) fn holds<P: Replays>(
        &self,
        differing: &[String],
        replayed: &Replayed,
    ) -> Result<(), String> {
        let as_stated = differing.len() == self.differs.len()
            && differing
                .iter()
                .zip(self.differs)
                .all(|(found, stated)| found.starts_with(stated));
        if !as_stated {
            return Err(format!(
                "it differs from Python otherwise than its entry states, {:?}: {differing:?}",
                self.differs
            ));
        }
        if !self.instead.is_done::<P>(replayed) {
            let (unbuilt, outcome) = (&replayed.unbuilt, &replayed.outcome);
            let unread = &replayed.unread;
            return Err(format!(
                "not {:?}: {unread:?} {unbuilt:?} {outcome:?}",
                self.instead
            ));
        }
        Ok(())
    }
}

/// The done event of `events`.
fn done_of(events: &[StreamEvent]) -> Option<&StreamDone> {
    events.iter().find_map(|event| match event {
        StreamEvent::Done(done) => Some(done),
        StreamEvent::TextDelta(_)
        | StreamEvent::ToolCall(_)
        | StreamEvent::ToolCallDelta(_)
        | StreamEvent::ThinkingDelta(_)
        | StreamEvent::Usage(_) => None,
    })
}

impl Instead {
    /// Whether `replayed` did exactly what this says.
    fn is_done<P: Replays>(self, replayed: &Replayed) -> bool {
        if replayed.seen.len() != self.requests {
            return false;
        }
        let said = |said: &str, says: Says| match says {
            Says::Exactly(words) => said == words,
            Says::Holding(words) => said.contains(words),
        };
        let failed = |error: &ProviderError, expected: Does| {
            let Does::Fails(status, retryable, overflow, says) = expected else {
                return false;
            };
            said(&error.message, says)
                && error.status_code == status
                && error.retryable == retryable
                && error.context_overflow == overflow
                && !error.before_request
        };
        if let Does::Unread(says) = self.does {
            let unread = replayed.unread.as_deref();
            return unread.is_some_and(|unread| said(unread, says)) && replayed.outcome.is_none();
        }
        match (self.does, &replayed.unbuilt, &replayed.outcome) {
            (Does::Unbuilt(words), Some(error), None) => {
                error.before_request && !error.retryable && error.message == words
            }
            (
                Does::Fails(..),
                None,
                Some(
                    Outcome::Response(Err(error))
                    | Outcome::Models(Err(error))
                    | Outcome::Events(_, Some(error))
                    | Outcome::Text(_, Some(error))
                    | Outcome::Images(Err(ImageError::Provider(error)), _),
                ),
            ) => failed(error, self.does),
            (
                Does::FailsBefore(says),
                None,
                Some(
                    Outcome::Response(Err(error))
                    | Outcome::Models(Err(error))
                    | Outcome::Events(_, Some(error))
                    | Outcome::Text(_, Some(error)),
                ),
            ) => {
                error.before_request
                    && !error.retryable
                    && error.status_code.is_none()
                    && said(&error.message, says)
            }
            (Does::AsksWithoutAKey, None, Some(Outcome::Response(Err(error)))) => {
                let asked = replayed.seen.first();
                let no_key = asked.is_some_and(|seen| !seen.headers.contains_key(P::KEY_HEADER));
                let nothing = Says::Exactly("the provider answered HTTP 599: no message");
                no_key && failed(error, Does::Fails(Some(599), true, None, nothing))
            }
            (Does::Answers(text, finish, usage), None, Some(Outcome::Response(Ok(response)))) => {
                response.content == text
                    && response.finish_reason == Some(finish)
                    && serde_json::to_string(&response.usage).unwrap() == usage
            }
            (Does::Answers(text, finish, usage), None, Some(Outcome::Events(events, None))) => {
                let done = done_of(events);
                let stopped = done.and_then(|done| done.finish_reason);
                let counted = done.map(|done| serde_json::to_string(&done.usage).unwrap());
                text_of(events) == text
                    && stopped == Some(finish)
                    && counted.as_deref() == Some(usage)
            }
            _ => false,
        }
    }
}

/// A named case: its name in the fixture, why the provider answers it
/// otherwise than Python, how its replay then differs from Python's, each
/// difference by the words it starts with, and what the provider does
/// instead.
pub type Entry = (&'static str, &'static str, &'static [&'static str], Instead);

/// The cases a provider answers otherwise than Python, each in the list
/// its case belongs to.
#[derive(Clone, Copy, Debug)]
pub struct Named {
    /// The cases the fixture questions: a behaviour of Python that is not
    /// settled, which the provider is decided otherwise.
    pub questioned_otherwise: &'static [Entry],
    /// The cases whose failure no provider reproduces: an SDK's own, or a
    /// raw Python exception.
    pub only_python: &'static [Entry],
    /// The cases the provider answers otherwise by a rule of its own, where
    /// the fixture questions nothing and Python fails with a
    /// `ProviderError` or answers.
    pub by_its_own_rule: &'static [Entry],
}

impl Named {
    /// The three lists, each with the name the data file gives it.
    fn lists(&self) -> [(&'static str, &'static [Entry]); 3] {
        [
            ("questioned_otherwise", self.questioned_otherwise),
            ("only_python", self.only_python),
            ("by_its_own_rule", self.by_its_own_rule),
        ]
    }

    /// The named cases, by name: what the entry of each states.
    pub(super) fn stated(&self) -> BTreeMap<&'static str, Stated> {
        let named = self.lists().into_iter().flat_map(|(_, entries)| entries);
        let stated = |(name, _, differs, instead): &Entry| {
            let (differs, instead) = (*differs, *instead);
            (*name, Stated { differs, instead })
        };
        named.map(stated).collect()
    }

    /// The named cases as a data file, for a replay outside the provider's
    /// crate: `notes`, which say what the file is and how it is rewritten,
    /// then each case with the list it stands in, its reason, the
    /// differences from Python its replay shows and what the provider does
    /// instead.
    #[must_use]
    pub fn as_data(&self, notes: &str) -> String {
        let mut cases = Vec::new();
        for (list, entries) in self.lists() {
            for (name, reason, differs, instead) in entries {
                cases.push(serde_json::json!({
                    "name": name,
                    "list": list,
                    "reason": reason,
                    "differs": differs,
                    "requests": instead.requests,
                    "does": does_as_data(instead.does),
                }));
            }
        }
        let file = serde_json::json!({"notes": notes, "cases": cases});
        serde_json::to_string_pretty(&file).unwrap() + "\n"
    }

    /// Fails unless each named case is a case of `fixture`, in the list its
    /// case belongs to: questioned there, failing with what no provider
    /// reproduces, or neither; unless it says why; and unless no case is
    /// named twice.
    pub fn set_apart(&self, fixture: &Fixture) {
        let raw = |name: &str| {
            let compare = fixture.case(name).compare.as_ref();
            compare.is_some_and(|compare| compare.exactly.is_empty())
        };
        for (name, reason, differs, _) in self.questioned_otherwise {
            assert!(fixture.case(name).questioned.is_some(), "{name}");
            assert!(reason.ends_with('.') && !differs.is_empty(), "{name}");
        }
        for (name, reason, differs, _) in self.only_python {
            assert!(raw(name), "{name}");
            assert!(reason.ends_with('.') && !differs.is_empty(), "{name}");
        }
        for (name, reason, differs, _) in self.by_its_own_rule {
            assert!(
                fixture.case(name).questioned.is_none() && !raw(name),
                "{name}"
            );
            assert!(reason.ends_with('.') && !differs.is_empty(), "{name}");
        }
        let listed =
            self.questioned_otherwise.len() + self.only_python.len() + self.by_its_own_rule.len();
        assert_eq!(self.stated().len(), listed);
    }
}

/// What a failure says, as the data file writes it.
fn says_as_data(says: Says) -> serde_json::Value {
    match says {
        Says::Exactly(words) => serde_json::json!({"exactly": words}),
        Says::Holding(words) => serde_json::json!({"holding": words}),
    }
}

/// What the provider does of a named case, as the data file writes it.
fn does_as_data(does: Does) -> serde_json::Value {
    match does {
        Does::Unbuilt(says) => serde_json::json!({"kind": "unbuilt", "says": says}),
        Does::Fails(status_code, retryable, context_overflow, says) => serde_json::json!({
            "kind": "fails",
            "status_code": status_code,
            "retryable": retryable,
            "context_overflow": context_overflow,
            "says": says_as_data(says),
        }),
        Does::Answers(text, finish_reason, usage) => serde_json::json!({
            "kind": "answers",
            "text": text,
            "finish_reason": finish_reason,
            "usage": serde_json::from_str::<serde_json::Value>(usage).unwrap(),
        }),
        Does::FailsBefore(says) => {
            serde_json::json!({"kind": "fails_before", "says": says_as_data(says)})
        }
        Does::Unread(says) => serde_json::json!({"kind": "unread", "says": says_as_data(says)}),
        Does::AsksWithoutAKey => serde_json::json!({"kind": "asks_without_a_key"}),
    }
}
