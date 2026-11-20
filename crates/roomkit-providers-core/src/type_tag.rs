//! The `type` a tagged model is read with: its one literal.

/// Declares what a model's `type` reads as: its one literal, which is also
/// what a missing `type` stands for, as each Python model defaults it. Any
/// other value is refused.
macro_rules! type_tag {
    ($name:ident, $literal:literal) => {
        #[derive(Default, serde::Deserialize)]
        enum $name {
            #[default]
            #[serde(rename = $literal)]
            Literal,
        }
    };
}

pub(crate) use type_tag;
