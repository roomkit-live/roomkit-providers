//! The one way a failure leaves the provider: its text without a secret of
//! the configuration, on one line, without a character a terminal acts on,
//! within its bound.
//!
//! Inside the provider a failure is a [`Failure`], which keeps apart the
//! provider's own words and what a server, a reader or the contract said.
//! Only [`Exit::leave`] makes the contract's `ProviderError` of it, and
//! there what was said is cleaned, once; the provider's own words are not
//! searched for a secret.

use std::error::Error;
use std::fmt;

use roomkit_providers_llm::{
    ProviderError, ProviderErrorOptions, ResponseSchemaError, ResponseSchemaErrorOptions, repr,
};

use super::{Built, Failure, MESSAGE_LIMIT, Said};

/// The fewest characters of a secret: a shorter text of the configuration
/// is not removed from a failure, for it would be found in ordinary words.
pub(crate) const SECRET_MIN: usize = 8;

/// What stands where a secret stood. It is shorter than any secret, so it
/// holds none, and each removal shortens the text.
pub(crate) const MARK: &str = "[***]";

/// Whether `character` shows nothing: a soft hyphen, a zero-width or
/// format character, a control of the direction of a text, a tag character.
/// A failure's text drops each, so that a secret one of them splits is
/// still found, and nothing a viewer acts on is left.
fn shows_nothing(character: char) -> bool {
    matches!(character, '\u{ad}' | '\u{61c}' | '\u{180e}' | '\u{feff}')
        || ('\u{200b}'..='\u{200f}').contains(&character)
        || ('\u{202a}'..='\u{202e}').contains(&character)
        || ('\u{2060}'..='\u{206f}').contains(&character)
        || ('\u{fff9}'..='\u{fffb}').contains(&character)
        || ('\u{e0000}'..='\u{e007f}').contains(&character)
}

/// `text` without the characters that show nothing.
fn shown(text: &str) -> String {
    text.chars().filter(|c| !shows_nothing(*c)).collect()
}

/// `text` on one line: without the characters that show nothing, each
/// control character and each run of white space one space, none at its
/// ends.
pub(crate) fn one_line(text: &str) -> String {
    let shown = shown(text);
    let spaced = shown.chars().map(|c| if c.is_control() { ' ' } else { c });
    let spaced: String = spaced.collect();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text` as the code that builds a failure may write it: as it is, as a
/// JSON string writes it (an error object or a call's id written as JSON),
/// as a reader quotes a text it refuses, and as the contract quotes a value,
/// Python's `repr`, whose quote depends on the text around: with a single
/// quote left as it is, and escaped.
fn written_forms(text: &str) -> Vec<String> {
    let inside = |quoted: String| {
        let mut inside = quoted.chars();
        inside.next();
        inside.next_back();
        inside.as_str().to_owned()
    };
    let json = serde_json::to_string(text).unwrap_or_default();
    let beside_a_double_quote = inside(repr(&format!("{text}\"").into()));
    let escaped = beside_a_double_quote.strip_suffix('"');
    vec![
        text.to_owned(),
        inside(json),
        inside(format!("{text:?}")),
        inside(repr(&text.into())),
        escaped.unwrap_or_default().to_owned(),
    ]
}

/// The secrets of a configuration: what no failure's text may repeat.
#[derive(Clone, Default)]
pub(crate) struct Secrets {
    /// The forms a text holds only before it is put on one line.
    before: Vec<String>,
    /// The forms a text on one line holds.
    after: Vec<String>,
}

impl fmt::Debug for Secrets {
    /// How many forms: never one of them.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secrets({})", self.after.len())
    }
}

impl Secrets {
    /// The secrets among `texts`: each of [`SECRET_MIN`] characters or
    /// more, in every form a failure's text may hold it in.
    pub(crate) fn of(texts: impl IntoIterator<Item = String>) -> Self {
        let (mut before, mut after) = (Vec::new(), Vec::new());
        let keep = |held: &mut Vec<String>, form: String| {
            if form.chars().count() >= SECRET_MIN && !held.contains(&form) {
                held.push(form);
            }
        };
        let secrets = texts.into_iter().map(|text| shown(&text));
        for secret in secrets.filter(|secret| secret.chars().count() >= SECRET_MIN) {
            for form in written_forms(&secret) {
                let line = one_line(&form);
                if line != form {
                    keep(&mut before, form);
                }
                keep(&mut after, line);
            }
        }
        // The longest first: a secret that holds another goes whole.
        before.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
        after.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
        Self { before, after }
    }

    /// `text` with none of `secrets` in it. Removing one may bring the
    /// pieces of another together, so the text is searched until none is
    /// found; it gets shorter each time, which ends the search.
    fn removed(text: &str, secrets: &[String]) -> String {
        let mut text = text.to_owned();
        while let Some(secret) = secrets.iter().find(|secret| text.contains(secret.as_str())) {
            text = text.replace(secret.as_str(), MARK);
        }
        text
    }

    /// Whether `text` needs no cleaning: it holds no secret, no control
    /// character and no character that shows nothing.
    fn is_free(&self, text: &str) -> bool {
        let cleaned = Self::removed(&Self::removed(text, &self.before), &self.after);
        !text.chars().any(|c| c.is_control() || shows_nothing(c)) && cleaned == text
    }

    /// What a server, a reader or the contract said, as a failure carries
    /// it: without a secret, on one line, within `limit` characters. The
    /// characters that show nothing go first, so that none hides a secret.
    /// The secrets are then looked for twice: before the text is put on
    /// one line, in the forms that step would lose, and after it, in every
    /// form, a secret sent back with other white space included.
    pub(crate) fn clean(&self, text: &str, limit: usize) -> String {
        let text = Self::removed(&shown(text), &self.before);
        let line = Self::removed(&one_line(&text), &self.after);
        line.chars().take(limit).collect()
    }
}

/// Where a failure leaves the provider.
#[derive(Clone, Debug)]
pub(crate) struct Exit {
    /// The provider's name, which each failure carries.
    pub(crate) provider: &'static str,
    pub(crate) secrets: Secrets,
}

impl Exit {
    /// `failure` as the contract's error: the only place one is made.
    pub(crate) fn leave(&self, failure: &Failure) -> ProviderError {
        let (said, built) = match &failure.0 {
            Built::Elsewhere(error) => return self.cleaned(error),
            Built::Here { said, .. } => (said, &failure.0),
        };
        let mut message = String::new();
        for part in said {
            match part {
                Said::Own(text) => message.push_str(text),
                Said::Told(text, limit) => message.push_str(&self.secrets.clean(text, *limit)),
            }
        }
        let Built::Here {
            retryable,
            status,
            context_overflow,
            before_request,
            ..
        } = built
        else {
            return self.cleaned(&ProviderError::new(
                message,
                ProviderErrorOptions::default(),
            ));
        };
        let options = ProviderErrorOptions {
            retryable: *retryable,
            provider: self.provider.to_owned(),
            status_code: *status,
            context_overflow: *context_overflow,
            before_request: *before_request,
        };
        let bounded: String = message.chars().take(MESSAGE_LIMIT).collect();
        ProviderError::new(bounded, options)
    }

    /// An error built outside this module, by the contract or by a reader:
    /// its whole text is cleaned, for its own words and what it quotes
    /// cannot be told apart. It leaves as it is when nothing in it needs
    /// cleaning, its source too; else rebuilt around the cleaned text, a
    /// response schema's failure with its reason, any other with no
    /// source.
    fn cleaned(&self, error: &ProviderError) -> ProviderError {
        let message = self.secrets.clean(&error.message, MESSAGE_LIMIT);
        let (mut cause, mut free) = (error.source(), true);
        while let Some(found) = cause {
            free &= self.secrets.is_free(&format!("{found} {found:?}"));
            cause = found.source();
        }
        if free && message == error.message {
            return error.clone();
        }
        let schema = error.source();
        let schema = schema.and_then(|cause| cause.downcast_ref::<ResponseSchemaError>());
        if let Some(schema) = schema {
            let options = ResponseSchemaErrorOptions {
                provider: error.provider.clone(),
            };
            return ResponseSchemaError::new(message, schema.reason, options).into();
        }
        let options = ProviderErrorOptions {
            retryable: error.retryable,
            provider: error.provider.clone(),
            status_code: error.status_code,
            context_overflow: error.context_overflow,
            before_request: error.before_request,
        };
        ProviderError::new(message, options)
    }

    /// An item of a stream that went through the contract's check: the
    /// failure it carries leaves, and so does one the check built.
    pub(crate) fn leave_carried(&self, error: ProviderError) -> ProviderError {
        let carried = error.source();
        match carried.and_then(|cause| cause.downcast_ref::<Carried>()) {
            Some(carried) => self.leave(&carried.0),
            None => self.leave(&Failure::from(error)),
        }
    }
}

/// A failure on its way out through a stream of the contract's errors: the
/// source of an error that says nothing, which [`Exit::leave_carried`]
/// unwraps.
#[derive(Debug)]
struct Carried(Failure);

impl fmt::Display for Carried {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a failure on its way out of the provider")
    }
}

impl Error for Carried {}

/// `failure` as an item of a stream of the contract's errors, to leave
/// through [`Exit::leave_carried`]. It says nothing by itself.
pub(crate) fn carried(failure: Failure) -> ProviderError {
    ProviderError::new(String::new(), ProviderErrorOptions::default()).with_source(Carried(failure))
}

/// The exit of a test's provider, whose key is the placeholder every test
/// configures.
#[cfg(test)]
pub(crate) fn test_exit() -> Exit {
    Exit {
        provider: "openai",
        secrets: Secrets::of(["placeholder-not-a-key".to_owned()]),
    }
}
