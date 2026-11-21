//! A union of models told apart by their `type`, read from a JSON object.

use std::fmt;
use std::marker::PhantomData;

use serde::Deserializer;
use serde::de::value::{MapAccessDeserializer, MapDeserializer};
use serde::de::{Error, MapAccess, Visitor};
use serde_json::Value;

/// The member that says which model an object is.
const TYPE: &str = "type";

/// The members of an object held until its `type` is found, as written.
type Held = Vec<(String, Value)>;

/// A union of models told apart by their `type`.
pub(crate) trait Tagged: Sized {
    /// What the union is, for an error that expected one.
    const EXPECTING: &'static str;

    /// The `type` of each of its models.
    const TYPES: &'static [&'static str];

    /// The model `tag` names, read from `rest`, its object without its
    /// `type`; `None` for a `type` that names none of its models.
    fn read<'de, D: Deserializer<'de>>(tag: &str, rest: D) -> Option<Result<Self, D::Error>>;
}

/// Reads a union's model from an object, its `type` a string.
///
/// A model Python wrote has its `type` first: the rest of the object is then
/// read as it comes. An object whose `type` comes later is held until it is
/// found, then read, each member as it was written: a member written twice
/// reads as it does when the `type` comes first. Anything but an object is
/// refused, and so is a `type` that is no string, where a derived union
/// takes a list for the object and, from some sources, a number for the
/// `type`.
pub(crate) fn read_tagged<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Tagged,
{
    deserializer.deserialize_map(TaggedVisitor(PhantomData))
}

struct TaggedVisitor<T>(PhantomData<fn() -> T>);

impl<'de, T: Tagged> Visitor<'de> for TaggedVisitor<T> {
    type Value = T;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(T::EXPECTING)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<T, A::Error> {
        let Some(first) = map.next_key::<String>()? else {
            return Err(A::Error::missing_field(TYPE));
        };
        if first == TYPE {
            let tag: String = map.next_value()?;
            return T::read(&tag, MapAccessDeserializer::new(map))
                .unwrap_or_else(|| Err(unknown::<T, A::Error>(&tag)));
        }
        let (tag, held) = held_until_typed(first, map)?;
        let rest = MapDeserializer::<_, serde_json::Error>::new(held.into_iter());
        match T::read(&tag, rest) {
            Some(read) => read.map_err(A::Error::custom),
            None => Err(unknown::<T, A::Error>(&tag)),
        }
    }
}

/// The `type` and the other members of an object whose `type` does not come
/// first, `first` the name already read. The members are kept as written,
/// in order and a repeated one each time, so that the model reads them as
/// it reads the rest of an object whose `type` comes first.
fn held_until_typed<'de, A: MapAccess<'de>>(
    first: String,
    mut map: A,
) -> Result<(String, Held), A::Error> {
    let mut held = Held::new();
    let mut tag = None;
    let mut name = Some(first);
    while let Some(member) = name {
        if member == TYPE && tag.is_none() {
            tag = Some(map.next_value::<String>()?);
        } else {
            held.push((member, map.next_value()?));
        }
        name = map.next_key()?;
    }
    let tag = tag.ok_or_else(|| A::Error::missing_field(TYPE))?;
    Ok((tag, held))
}

/// The refusal of a `type` that names none of the union's models.
fn unknown<T: Tagged, E: Error>(tag: &str) -> E {
    E::unknown_variant(tag, T::TYPES)
}
