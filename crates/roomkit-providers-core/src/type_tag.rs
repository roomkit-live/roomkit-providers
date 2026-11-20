//! The `type` a tagged model is read with: its one literal.

/// Declares what a model's `type` reads as: its one literal, a JSON string,
/// which is also what a missing `type` stands for, as each Python model
/// defaults it. Any other value is refused.
macro_rules! type_tag {
    ($name:ident, $literal:literal) => {
        #[derive(Default)]
        struct $name;

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let expecting = concat!("the type `", $literal, "`");
                $crate::strict::read_name(deserializer, expecting, |name| {
                    (name == $literal).then_some(Self)
                })
            }
        }
    };
}

pub(crate) use type_tag;
