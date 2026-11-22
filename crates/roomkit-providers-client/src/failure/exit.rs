//! The one way a failure leaves a provider: its text without a secret of
//! the configuration, on one line, without a character a terminal acts on,
//! within its bound.
//!
//! Inside a provider a failure is a [`Failure`], which keeps apart the
//! provider's own words and what a server, a reader or the contract said.
//! Only [`Exit::leave`] makes the contract's `ProviderError` of it, and
//! there what was said is cleaned, once; the provider's own words are not
//! searched for a secret.
//!
//! A provider gets its exit from its client
//! ([`Client::exit`](crate::Client::exit)), which adds to the secrets of
//! the configuration those it alone knows. One that could not be built has
//! no client: its failure leaves through [`Exit::unbuilt`], in the
//! provider's own words alone.
//!
//! The failure of a generation leaves with its request
//! ([`Exit::leave_for`], [`leaving`](crate::stream::leaving)): the key a
//! host lends that request is a secret too, which the exit reads from the
//! request itself.

use std::borrow::Cow;
use std::error::Error;
use std::fmt;
use std::ops::Range;

use roomkit_ai::{
    AiContext, ProviderError, ProviderErrorOptions, ResponseSchemaError, ResponseSchemaErrorOptions,
};
use roomkit_core::rules::python_repr;

use super::{Built, Failure, MESSAGE_LIMIT, Piece};

/// The fewest characters of a secret: a shorter text of the configuration
/// is not removed from a failure, for it would be found in ordinary words.
pub const SECRET_MIN: usize = 8;

/// What stands where a secret stood. It is shorter than any secret, so it
/// holds none, and each removal shortens the text.
pub const MARK: &str = "[***]";

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
#[must_use]
pub fn one_line(text: &str) -> String {
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
    let beside_a_double_quote = inside(python_repr(&format!("{text}\"")));
    let escaped = beside_a_double_quote.strip_suffix('"');
    vec![
        text.to_owned(),
        inside(json),
        inside(format!("{text:?}")),
        inside(python_repr(text)),
        escaped.unwrap_or_default().to_owned(),
    ]
}

/// The byte the start of `text` writes as a URL does, a percent sign and two
/// hexadecimal digits in either case, and what follows it.
fn byte_of_a_url(text: &str) -> Option<(u8, &str)> {
    let mut digits = text.strip_prefix('%')?.chars();
    let high = digits.next()?.to_digit(16)?;
    let low = digits.next()?.to_digit(16)?;
    Some((u8::try_from(high * 16 + low).ok()?, digits.as_str()))
}

/// What follows `c` at the start of `text`, written as it is and written as
/// a URL may write it, each of its bytes a percent sign and two digits.
fn after(text: &str, c: char) -> [Option<&str>; 2] {
    let mut bytes = [0; 4];
    let mut as_a_url_writes_it = c.encode_utf8(&mut bytes).bytes();
    let escaped = as_a_url_writes_it.try_fold(text, |rest, byte| {
        let (written, rest) = byte_of_a_url(rest)?;
        (written == byte).then_some(rest)
    });
    [text.strip_prefix(c), escaped]
}

/// How many bytes at the start of `text` write `secret` as a URL may: each
/// character as it is or escaped, whichever characters the writer chose to
/// escape. A percent sign of the secret reads both ways, so every reading
/// is followed, and the longest one counts.
fn written_as_a_url(text: &str, secret: &str) -> Option<usize> {
    let mut readings = vec![text];
    for c in secret.chars() {
        let mut next: Vec<&str> = Vec::new();
        for rest in readings.iter().flat_map(|rest| after(rest, c)).flatten() {
            if !next.iter().any(|known| known.len() == rest.len()) {
                next.push(rest);
            }
        }
        if next.is_empty() {
            return None;
        }
        readings = next;
    }
    let shortest_rest = readings.iter().map(|rest| rest.len()).min()?;
    Some(text.len() - shortest_rest)
}

/// Where `text` first holds `secret`, read from its start: as it is, or, in
/// a text with a percent sign (`escapes`), as a URL writes it. Only a text
/// with a percent sign holds a secret that other way, and only where the
/// secret's first character is, as it is or escaped.
fn held(text: &str, secret: &str, escapes: bool) -> Option<Range<usize>> {
    if !escapes {
        let start = text.find(secret)?;
        return Some(start..start + secret.len());
    }
    let first = secret.chars().next()?;
    let may_start = |c: char| c == first || c == '%';
    let starts = text.char_indices().filter(|(_, c)| may_start(*c));
    starts.map(|(start, _)| start).find_map(|start| {
        let rest = text.get(start..)?;
        let length = if rest.starts_with(secret) {
            secret.len()
        } else {
            written_as_a_url(rest, secret)?
        };
        Some(start..start + length)
    })
}

/// `text` with the mark in every place it holds `secret` at, or `None` for
/// a text that holds it nowhere. The text is read once, from its start to
/// its end, however many places there are: a server chooses how often its
/// answer repeats a key.
fn without(text: &str, secret: &str) -> Option<String> {
    if secret.is_empty() {
        return None;
    }
    // Asked once for the whole text, not once for each place.
    let escapes = text.contains('%');
    let mut cleaned = String::new();
    let mut rest = text;
    let mut found = false;
    while let Some(place) = held(rest, secret, escapes) {
        // A place starts and ends where characters do. Were it not so,
        // nothing of the text is shown.
        let (Some(kept), Some(after)) = (rest.get(..place.start), rest.get(place.end..)) else {
            return Some(MARK.to_owned());
        };
        cleaned.push_str(kept);
        cleaned.push_str(MARK);
        rest = after;
        found = true;
    }
    found.then(|| cleaned + rest)
}

/// The secrets of a configuration: what no failure's text may repeat.
#[derive(Clone, Default)]
pub struct Secrets {
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
    /// more, in every form the code that builds a failure may write it in.
    /// A text is searched for each form as it is and as a URL writes it.
    pub fn of(texts: impl IntoIterator<Item = String>) -> Self {
        Self::default().with(texts)
    }

    /// These secrets and those among `texts`: for a secret one request
    /// brings, beside the configuration's.
    #[must_use]
    pub fn with(&self, texts: impl IntoIterator<Item = String>) -> Self {
        let (mut before, mut after) = (self.before.clone(), self.after.clone());
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

    /// `text` with none of `secrets` in it, as it is or as a URL writes it.
    /// Each pass marks every place one secret is held at, in one reading of
    /// the text. Removing one may bring the pieces of another together, so
    /// the passes go on until none is found; the text gets shorter each
    /// time, which ends the search.
    fn removed(text: &str, secrets: &[String]) -> String {
        let mut text = text.to_owned();
        while let Some(cleaned) = secrets.iter().find_map(|secret| without(&text, secret)) {
            text = cleaned;
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
    #[must_use]
    pub fn clean(&self, text: &str, limit: usize) -> String {
        let text = Self::removed(&shown(text), &self.before);
        let line = Self::removed(&one_line(&text), &self.after);
        line.chars().take(limit).collect()
    }
}

/// Where a failure leaves a provider.
///
/// A provider makes none itself: its client does
/// ([`Client::exit`](crate::Client::exit)), with the secrets it alone
/// knows.
///
/// ```compile_fail,E0624
/// use roomkit_providers_client::{Exit, Secrets};
///
/// let lacking_the_client_s_secrets = Exit::new("vendor", Secrets::default());
/// ```
#[derive(Clone, Debug)]
pub struct Exit {
    /// The provider's label, which each failure carries.
    provider: &'static str,
    secrets: Secrets,
}

impl Exit {
    /// The exit of the provider labelled `provider`, whose failures repeat
    /// none of `secrets`: what a client makes, and a test that has none.
    pub(crate) const fn new(provider: &'static str, secrets: Secrets) -> Self {
        Self { provider, secrets }
    }

    /// `failure`, the failure of a provider labelled `provider` that could
    /// not be built, as the contract's error. Such a provider has no client
    /// yet, so no secret is known to clean a text of: the provider's own
    /// words leave as they are, and the mark stands where the failure
    /// quotes a server or a reader. A failure the contract built leaves as
    /// it is, on one line and bounded.
    ///
    /// Nothing else leaves this way: the failure of a request leaves
    /// through the exit the provider's client makes
    /// ([`Client::exit`](crate::Client::exit)).
    #[must_use]
    pub fn unbuilt(provider: &'static str, failure: &Failure) -> ProviderError {
        let knowing_no_secret = Self::new(provider, Secrets::default());
        knowing_no_secret.leave(&failure.in_its_own_words())
    }

    /// This exit for the generation that answers `request`: its failures
    /// do not repeat the key `request` brings for itself either, read as
    /// the contract reads it (`request_api_key`) and searched for in every
    /// form a secret of the configuration is. The exit itself when
    /// `request` brings none.
    fn answering(&self, request: &AiContext) -> Cow<'_, Self> {
        match crate::key::request_api_key(request) {
            Some(key) => Cow::Owned(Self {
                provider: self.provider,
                secrets: self.secrets.with([key.to_owned()]),
            }),
            None => Cow::Borrowed(self),
        }
    }

    /// `failure`, the failure of the generation that answers `request`, as
    /// the contract's error: as [`leave`](Self::leave), without the key
    /// `request` brings for itself either, whether the provider sends that
    /// key or not.
    #[must_use]
    pub fn leave_for(&self, request: &AiContext, failure: &Failure) -> ProviderError {
        self.answering(request).leave(failure)
    }

    /// `failure` as the contract's error: the only place one is made.
    ///
    /// It takes no request, so it is for what no request brings a key to:
    /// the model listing, which is asked with the configuration's key. The
    /// failure of a generation leaves with its request:
    /// [`leave_for`](Self::leave_for), and
    /// [`leaving`](crate::stream::leaving) for a stream.
    #[must_use]
    pub fn leave(&self, failure: &Failure) -> ProviderError {
        let (said, built) = match &failure.0 {
            Built::Elsewhere(error) => return self.cleaned(error),
            Built::Here { said, .. } => (said, &failure.0),
        };
        let mut message = String::new();
        for part in said {
            match &part.0 {
                Piece::Own(text) => message.push_str(text),
                Piece::Told(text, limit) => message.push_str(&self.secrets.clean(text, *limit)),
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

    /// An item of the stream of the generation that answers `request`,
    /// once it went through the contract's check: the failure it carries
    /// leaves with `request`, and so does one the check built.
    pub(crate) fn leave_carried(&self, request: &AiContext, error: ProviderError) -> ProviderError {
        let carried = error.source();
        match carried.and_then(|cause| cause.downcast_ref::<Carried>()) {
            Some(carried) => self.leave_for(request, &carried.0),
            None => self.leave_for(request, &Failure::from(error)),
        }
    }
}

/// A failure on its way out through a stream of the contract's errors: the
/// source of an error that says nothing, which the exit unwraps where the
/// stream leaves.
#[derive(Debug)]
struct Carried(Failure);

impl fmt::Display for Carried {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a failure on its way out of the provider")
    }
}

impl Error for Carried {}

/// `failure` as an item of a stream of the contract's errors, to leave
/// where its stream does ([`leaving`](crate::stream::leaving)). It says
/// nothing by itself.
#[must_use]
pub fn carried(failure: Failure) -> ProviderError {
    ProviderError::new(String::new(), ProviderErrorOptions::default()).with_source(Carried(failure))
}
