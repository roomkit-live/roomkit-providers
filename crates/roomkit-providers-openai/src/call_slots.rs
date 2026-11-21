//! A streamed response's call fragments folded into calls, one slot per call:
//! Python's `ToolCallSlots`.
//!
//! A fragment belongs to the call its stream index names, unless it starts
//! another call: a server that sends each call whole may tag every one with
//! index 0, and folding them together would run one call made of two. Each slot
//! has an id from its opening and keeps it once a composition event names it.
//!
//! Folding a call costs what its bytes cost, not more: whether the
//! arguments held are whole is asked of [`held::Held`], which read them as
//! they came, and what a server controls of a call is bounded, the number
//! of calls ([`CALLS_LIMIT`]), a call's arguments ([`ARGUMENTS_LIMIT`]) and
//! its id and its name ([`NAME_LIMIT`]): a stream past one of them fails.

use std::collections::{HashMap, HashSet};

use roomkit_providers_core::{CallIds, StreamToolCallDelta};
use serde_json::Value;

use self::held::{Held, stripped};
use self::odd_id::same_odd;
use crate::failure::Failure;

mod held;
mod odd_id;

/// The most calls one streamed answer may hold.
pub(crate) const CALLS_LIMIT: usize = 1024;

/// The most bytes of one call's arguments: what an answer read whole may
/// weigh.
pub(crate) const ARGUMENTS_LIMIT: usize = 4 * 1024 * 1024;

/// The most bytes of a call's id or of its name, each of which every event
/// of the call's composition repeats.
pub(crate) const NAME_LIMIT: usize = 1024;

/// Why the calls of a stream do not read: what is wrong, in the provider's
/// words, and the value the server wrote, when one is to blame.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Unread {
    what: &'static str,
    value: Option<String>,
}

impl Unread {
    const fn because(what: &'static str) -> Self {
        Self { what, value: None }
    }

    /// An id that is no text.
    fn id(id: &Value) -> Self {
        Self {
            what: "a call's id is no text",
            value: Some(id.to_string()),
        }
    }
}

impl From<Unread> for Failure {
    fn from(unread: Unread) -> Self {
        match unread.value {
            Some(value) => Self::unreadable_value(unread.what, value),
            None => Self::unreadable(unread.what),
        }
    }
}

/// A call as its stream gave it: its id, its tool and the text of its
/// arguments, which is read once the stream ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RawCall {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) arguments: String,
}

impl RawCall {
    pub(crate) fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
        }
    }
}

/// A call as its fragments built it so far.
#[derive(Debug, Default)]
struct Slot {
    id: String,
    /// An id minted here can be replaced until its first composition event.
    minted: bool,
    /// The server's id when it was no text (a number, `true`): the slot
    /// holds it as Python's does, and fails the read once it is a call.
    odd_id: Option<Value>,
    name: String,
    arguments: String,
    /// What the arguments are so far, read as they came.
    held: Held,
    /// Whether a fragment carried a function, even one with no name and no
    /// arguments: that is a call, for the host to refuse.
    function: bool,
    /// Whether a composition event named the call: it keeps that id.
    announced: bool,
}

/// A fragment's id, as Python's `ToolCallSlots` reads what the server wrote.
#[derive(Clone, Copy, Debug)]
pub(crate) enum FragmentId<'a> {
    /// None: absent, `null` or empty text (Python's `None` or `""`).
    None,
    /// The server's id.
    Text(&'a str),
    /// No text, and read as false: `0`, `false`, `[]`, `{}`. No id, but no
    /// absent one either: it names no call the fragment could continue.
    False,
    /// No text, and read as true.
    True(&'a Value),
}

impl FragmentId<'_> {
    /// Whether Python reads it as true.
    fn is_true(self) -> bool {
        matches!(self, Self::Text(_) | Self::True(_))
    }
}

/// One fragment of a call, as a chunk carries it.
#[derive(Debug)]
pub(crate) struct Fragment<'a> {
    /// The stream index it names.
    pub(crate) index: u64,
    /// The server's id, as it wrote it.
    pub(crate) id: FragmentId<'a>,
    /// The tool's name, when it gave one.
    pub(crate) name: Option<&'a str>,
    /// The piece of its arguments' text.
    pub(crate) arguments: &'a str,
    /// Whether it carried a function.
    pub(crate) function: bool,
}

/// The calls of a streamed response, in the order they came.
#[derive(Debug, Default)]
pub(crate) struct CallSlots {
    slots: Vec<Slot>,
    by_index: HashMap<u64, usize>,
    taken: HashSet<String>,
    /// The ids that are no text a slot took.
    taken_odd: Vec<Value>,
    /// How many bytes were looked at to tell whether held arguments are
    /// whole: each byte read as it came, and each byte given to the JSON
    /// parser.
    pub(crate) examined: usize,
}

impl CallSlots {
    /// Folds `fragment` in, into the call its index names unless it starts
    /// another. Opened by a new JSON object under no new id or name, or the
    /// held call's own, it is a second call to the held call's tool; under an
    /// id that is no text, a call with no name, for the host to refuse.
    ///
    /// # Errors
    ///
    /// Why the read fails: an id that is no text and that no set can hold
    /// (a list, an object), where Python asks whether a call holds it; a call
    /// named under an id that is no text, as Python's composition event,
    /// which carries the id, fails as it is built.
    pub(crate) fn fold(
        &mut self,
        fragment: &Fragment<'_>,
    ) -> Result<Option<StreamToolCallDelta>, Unread> {
        let too_long = |text: Option<&str>| text.is_some_and(|text| text.len() > NAME_LIMIT);
        if too_long(fragment.name) {
            return Err(Unread::because("a call's name is longer than 1024 bytes"));
        }
        if let FragmentId::Text(id) = fragment.id
            && too_long(Some(id))
        {
            return Err(Unread::because("a call's id is longer than 1024 bytes"));
        }
        let mut name = fragment.name.map(str::to_owned);
        let mut position = self.by_index.get(&fragment.index).copied();
        let examined = &mut self.examined;
        if let Some(held) = position.and_then(|at| self.slots.get_mut(at))
            && held.starts_another_call(fragment, examined)
        {
            if name.is_none() && held.is_continued_by(fragment.id) {
                name = Some(held.name.clone());
            }
            position = None;
        }
        let at = match position {
            Some(at) => at,
            None => self.new_slot(fragment.index, name.as_deref())?,
        };
        self.fill(at, fragment, name.as_deref())
    }

    fn new_slot(&mut self, index: u64, name: Option<&str>) -> Result<usize, Unread> {
        if self.slots.len() >= CALLS_LIMIT {
            return Err(Unread::because("the answer holds more than 1024 calls"));
        }
        let id = CallIds::new().call("", name.unwrap_or("tool"));
        self.taken.insert(id.clone());
        self.slots.push(Slot {
            id,
            minted: true,
            ..Slot::default()
        });
        let at = self.slots.len().saturating_sub(1);
        self.by_index.insert(index, at);
        Ok(at)
    }

    /// Adds a fragment and emits only the first name or new argument text.
    fn fill(
        &mut self,
        at: usize,
        fragment: &Fragment<'_>,
        name: Option<&str>,
    ) -> Result<Option<StreamToolCallDelta>, Unread> {
        self.adopt_id(at, fragment.id)?;
        let Some(slot) = self.slots.get_mut(at) else {
            return Ok(None);
        };
        slot.function |= fragment.function;
        let first_name = name.is_some() && slot.name.is_empty();
        if let Some(name) = name {
            name.clone_into(&mut slot.name);
        }
        if slot
            .arguments
            .len()
            .saturating_add(fragment.arguments.len())
            > ARGUMENTS_LIMIT
        {
            return Err(Unread::because(
                "a call's arguments are longer than 4194304 bytes",
            ));
        }
        slot.arguments.push_str(fragment.arguments);
        slot.held.push(fragment.arguments, &mut self.examined);
        if slot.name.is_empty() || (!first_name && fragment.arguments.is_empty()) {
            return Ok(None);
        }
        if let Some(id) = &slot.odd_id {
            return Err(Unread::id(id));
        }
        slot.announced = true;
        Ok(Some(StreamToolCallDelta {
            id: slot.id.clone(),
            name: slot.name.clone(),
            index: u32::try_from(at).unwrap_or(u32::MAX),
            arguments_delta: fragment.arguments.to_owned(),
        }))
    }

    /// Takes an unused server id only before the slot was announced.
    fn adopt_id(&mut self, at: usize, id: FragmentId<'_>) -> Result<(), Unread> {
        let Some(slot) = self.slots.get_mut(at) else {
            return Ok(());
        };
        if !id.is_true() || slot.has_server_id() || slot.announced {
            return Ok(());
        }
        match id {
            FragmentId::Text(id) if !self.taken.contains(id) => {
                self.taken.remove(&slot.id);
                id.clone_into(&mut slot.id);
                slot.minted = false;
                self.taken.insert(id.to_owned());
            }
            FragmentId::True(id @ (Value::Array(_) | Value::Object(_))) => {
                return Err(Unread::id(id));
            }
            FragmentId::True(id) if !self.taken_odd.iter().any(|held| same_odd(held, id)) => {
                slot.odd_id = Some(id.clone());
                self.taken_odd.push(id.clone());
            }
            FragmentId::None | FragmentId::False | FragmentId::Text(_) | FragmentId::True(_) => {}
        }
        Ok(())
    }

    /// The calls once the response is over, each with its argument text: a
    /// slot that never carried a function is no call.
    ///
    /// # Errors
    ///
    /// Why the read fails: a call that took an id that is no text, which
    /// Python's call refuses as its id.
    pub(crate) fn finish(self) -> Result<Vec<RawCall>, Unread> {
        self.slots
            .into_iter()
            .filter(|slot| slot.function || !slot.name.is_empty() || !slot.arguments.is_empty())
            .map(|slot| match slot.odd_id {
                Some(id) => Err(Unread::id(&id)),
                None => Ok(RawCall::new(slot.id, slot.name, slot.arguments)),
            })
            .collect()
    }
}

impl Slot {
    /// Whether `fragment`, on this call's index, starts another call. Another
    /// server id says so. Without one: another name once this call is whole;
    /// the same name bringing arguments of its own after whole ones; or, after
    /// whole arguments, a fragment that opens another JSON object. A server may
    /// repeat a call's name on every fragment, so the same name with nothing or
    /// blanks starts nothing. Whether the arguments held are whole is asked
    /// last, and only where the answer depends on it; `examined` counts what
    /// that looks at.
    fn starts_another_call(&mut self, fragment: &Fragment<'_>, examined: &mut usize) -> bool {
        if fragment.id.is_true() && self.has_server_id() && !self.holds(fragment.id) {
            return true;
        }
        if self.name.is_empty() {
            return false;
        }
        // White space is what Python's `str.strip()` strips.
        let blank = self.held.is_blank();
        let arguments = fragment.arguments;
        let brings = match fragment.name {
            None => !blank && arguments.trim_start_matches(stripped).starts_with('{'),
            Some(name) if name != self.name => true,
            Some(_) => !blank && !arguments.chars().all(stripped),
        };
        brings && self.held.is_whole(&self.arguments, examined)
    }

    /// Whether the call took the server's id, text or not.
    fn has_server_id(&self) -> bool {
        !self.minted || self.odd_id.is_some()
    }

    /// Whether `id` is the id the call took.
    fn holds(&self, id: FragmentId<'_>) -> bool {
        match id {
            FragmentId::Text(id) => self.odd_id.is_none() && id == self.id,
            FragmentId::True(id) => self.odd_id.as_ref().is_some_and(|held| same_odd(held, id)),
            FragmentId::None | FragmentId::False => false,
        }
    }

    /// Whether a fragment under `id` that starts another call with no name
    /// is a second call to this one's tool: under no id, or this call's own
    /// (Python's `call_id in (None, "", held["id"])`); an id that is no
    /// text and reads as false names no call.
    fn is_continued_by(&self, id: FragmentId<'_>) -> bool {
        matches!(id, FragmentId::None) || self.holds(id)
    }
}

#[cfg(test)]
mod tests;
