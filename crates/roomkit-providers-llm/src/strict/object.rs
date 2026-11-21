//! A model read from a JSON object, and from nothing else.

use serde::Deserializer;
use serde::de::Visitor;

/// A source that reads an object whatever its reader asks for.
///
/// A derived struct asks its source for a struct, which a JSON source also
/// answers with a list, each item read as the field at its place. Asked
/// through this one, the source is asked for an object, and refuses a list
/// as it refuses a number. The source is read once: nothing is held back.
///
/// For a provider crate too: the reader of its configuration.
#[derive(Debug)]
pub struct ObjectOnly<D>(pub D);

impl<'de, D: Deserializer<'de>> Deserializer<'de> for ObjectOnly<D> {
    type Error = D::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.0.deserialize_map(visitor)
    }

    fn is_human_readable(&self) -> bool {
        self.0.is_human_readable()
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

/// Implements `Deserialize` for each model as its form, read from an object
/// only. A form holds the model's fields as Python's model takes them, a
/// field left out taking its default, and converts into the model.
macro_rules! read_as_object {
    ($($form:ty => $model:ty),+ $(,)?) => {
        $(
            impl<'de> serde::Deserialize<'de> for $model {
                fn deserialize<D: serde::Deserializer<'de>>(
                    deserializer: D,
                ) -> Result<Self, D::Error> {
                    let object = $crate::strict::ObjectOnly(deserializer);
                    <$form as serde::Deserialize>::deserialize(object).map(Self::from)
                }
            }
        )+
    };
}

pub(crate) use read_as_object;
