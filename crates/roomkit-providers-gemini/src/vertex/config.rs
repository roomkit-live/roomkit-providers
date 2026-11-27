//! What a Gemini provider on Vertex AI is configured with: Python's
//! `GeminiVertexConfig`, its JSON form read and written as
//! `model_dump(mode="json")` writes it, its billing labels held to Google's
//! rules as Python holds them.

use std::fmt;
use std::time::Duration;

use roomkit_providers_client::ObjectOnly;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};

use crate::config::MODEL;

/// How a secret is written in a configuration's JSON form.
const MASK: &str = "**********";

/// The most labels a request carries.
const LABELS_AT_MOST: usize = 64;

/// The most characters of a label's key, or of its value.
const LABEL_LENGTH: usize = 63;

/// Gemini on Vertex AI, in a project and a region, and who calls it:
/// Python's `GeminiVertexConfig`, built with [`GeminiVertexConfig::new`]
/// and struct update.
///
/// Its `Debug` never prints the key or the service-account key.
#[derive(Clone)]
pub struct GeminiVertexConfig {
    /// Unused on Vertex: the identity is the service account's, never a
    /// key on the request.
    pub api_key: Option<String>,
    /// The model asked; `gemini-3.8-flash` by default.
    pub model: String,
    /// The token bound of an answer, which a request's own outranks.
    pub max_tokens: u32,
    /// The thinking level of a Gemini 3 model, which a request's effort
    /// outranks; `None` leaves the model's.
    pub thinking_level: Option<String>,
    /// The longest wait for the first read of an answer and between two.
    pub timeout: Duration,
    /// The longest wait for a connection.
    pub connect_timeout: Duration,
    /// The Google Cloud project that hosts the Vertex AI API.
    pub project: String,
    /// The Vertex region the requests are processed in, `global`, or a
    /// multi-region (`us`, `eu`): required, so that where the data goes is
    /// a choice.
    pub location: String,
    /// A service-account key file's contents, to call as that account.
    pub service_account_json: Option<String>,
    /// The email of a service account to borrow, with the identity above.
    pub impersonate_service_account: Option<String>,
    /// The billing labels every request carries, in order, by Google's
    /// rules: at most 64; a key of 1 to 63 characters that starts with a
    /// lowercase letter, a value of at most 63; lowercase letters of any
    /// script, digits, `_` and `-` only.
    pub labels: Option<Vec<(String, String)>>,
}

impl GeminiVertexConfig {
    /// The configuration calling Gemini in `project`, in `location`, with
    /// Python's defaults.
    pub fn new(project: impl Into<String>, location: impl Into<String>) -> Self {
        Self {
            api_key: None,
            model: MODEL.to_owned(),
            max_tokens: 1024,
            thinking_level: None,
            timeout: Duration::from_secs(60),
            connect_timeout: Duration::from_secs(5),
            project: project.into(),
            location: location.into(),
            service_account_json: None,
            impersonate_service_account: None,
            labels: None,
        }
    }

    /// The labels, held to Google's rules.
    ///
    /// # Errors
    ///
    /// What is wrong with them, naming the key, never a value.
    pub(crate) fn checked_labels(&self) -> Result<(), String> {
        match &self.labels {
            Some(labels) => checked(labels),
            None => Ok(()),
        }
    }
}

/// A lowercase letter, or a letter of a script without case: what Google
/// calls lowercase letters, international characters allowed.
fn label_letter(character: char) -> bool {
    matches!(
        character.general_category(),
        GeneralCategory::LowercaseLetter | GeneralCategory::OtherLetter
    )
}

/// A character of a label's key or value.
fn label_character(character: char) -> bool {
    character == '_'
        || character == '-'
        || label_letter(character)
        || character.general_category() == GeneralCategory::DecimalNumber
}

/// `labels` held to Google's rules, as Python's `_check_labels` holds them.
fn checked(labels: &[(String, String)]) -> Result<(), String> {
    if labels.len() > LABELS_AT_MOST {
        return Err(format!(
            "labels: at most {LABELS_AT_MOST} labels per request, got {}",
            labels.len()
        ));
    }
    for (key, value) in labels {
        let length = key.chars().count();
        if !(1..=LABEL_LENGTH).contains(&length) {
            return Err(format!(
                "label key {key:?}: 1 to {LABEL_LENGTH} characters, got {length}"
            ));
        }
        if !key.chars().next().is_some_and(label_letter) {
            return Err(format!(
                "label key {key:?} must start with a lowercase letter"
            ));
        }
        if !key.chars().all(label_character) {
            return Err(format!(
                "label key {key:?}: only lowercase letters, digits, '_' and '-' are allowed"
            ));
        }
        let length = value.chars().count();
        if length > LABEL_LENGTH {
            return Err(format!(
                "label {key:?}: value is {length} characters, at most {LABEL_LENGTH}"
            ));
        }
        if !value.chars().all(label_character) {
            return Err(format!(
                "label {key:?}: its value may only contain lowercase letters, digits, '_' and '-'"
            ));
        }
    }
    Ok(())
}

impl fmt::Debug for GeminiVertexConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hidden = |secret: &Option<String>| secret.as_ref().map(|_| "hidden");
        f.debug_struct("GeminiVertexConfig")
            .field("api_key", &hidden(&self.api_key))
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .field("thinking_level", &self.thinking_level)
            .field("timeout", &self.timeout)
            .field("connect_timeout", &self.connect_timeout)
            .field("project", &self.project)
            .field("location", &self.location)
            .field("service_account_json", &hidden(&self.service_account_json))
            .field(
                "impersonate_service_account",
                &self.impersonate_service_account,
            )
            .field("labels", &self.labels)
            .finish()
    }
}

fn defaults() -> GeminiVertexConfig {
    GeminiVertexConfig::new("", "")
}
fn model() -> String {
    defaults().model
}
fn max_tokens() -> u32 {
    defaults().max_tokens
}
fn timeout() -> Duration {
    defaults().timeout
}
fn connect_timeout() -> Duration {
    defaults().connect_timeout
}

/// Reads a duration written in seconds, from zero up.
fn seconds<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
    let written = f64::deserialize(deserializer)?;
    Duration::try_from_secs_f64(written).map_err(|_| {
        D::Error::custom(format!(
            "a duration in seconds, from zero up, is expected: got {written}"
        ))
    })
}

/// Reads labels, in their order, held to Google's rules.
fn labels<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<(String, String)>>, D::Error> {
    let Some(written) = Option::<Map<String, Value>>::deserialize(deserializer)? else {
        return Ok(None);
    };
    let read = written.into_iter().map(|(key, value)| match value {
        Value::String(value) => Ok((key, value)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => {
            Err(D::Error::custom("a label's value is a text"))
        }
    });
    let read: Vec<(String, String)> = read.collect::<Result<_, _>>()?;
    checked(&read).map_err(D::Error::custom)?;
    Ok(Some(read))
}

/// The configuration as it is read: a member Python does not have is
/// ignored, as Python ignores it.
#[derive(Deserialize)]
#[serde(expecting = "a Gemini on Vertex AI configuration, as a JSON object")]
struct Form {
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default = "model")]
    model: String,
    #[serde(default = "max_tokens")]
    max_tokens: u32,
    #[serde(default)]
    thinking_level: Option<String>,
    #[serde(default = "timeout", deserialize_with = "seconds")]
    timeout: Duration,
    #[serde(default = "connect_timeout", deserialize_with = "seconds")]
    connect_timeout: Duration,
    project: String,
    location: String,
    #[serde(default)]
    service_account_json: Option<String>,
    #[serde(default)]
    impersonate_service_account: Option<String>,
    #[serde(default, deserialize_with = "labels")]
    labels: Option<Vec<(String, String)>>,
}

impl<'de> Deserialize<'de> for GeminiVertexConfig {
    /// From a JSON object only, as Python refuses a list or a number.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let form = Form::deserialize(ObjectOnly(deserializer))?;
        Ok(Self {
            api_key: form.api_key,
            model: form.model,
            max_tokens: form.max_tokens,
            thinking_level: form.thinking_level,
            timeout: form.timeout,
            connect_timeout: form.connect_timeout,
            project: form.project,
            location: form.location,
            service_account_json: form.service_account_json,
            impersonate_service_account: form.impersonate_service_account,
            labels: form.labels,
        })
    }
}

/// A secret as Python writes a `SecretStr`: masked, empty when empty.
fn masked(secret: Option<&String>) -> Option<&'static str> {
    secret.map(|secret| if secret.is_empty() { "" } else { MASK })
}

/// The configuration as it is written, its secrets masked.
#[derive(Serialize)]
struct Written<'c> {
    api_key: Option<&'static str>,
    model: &'c str,
    max_tokens: u32,
    thinking_level: Option<&'c str>,
    timeout: f64,
    connect_timeout: f64,
    project: &'c str,
    location: &'c str,
    service_account_json: Option<&'static str>,
    impersonate_service_account: Option<&'c str>,
    labels: Option<Map<String, Value>>,
}

impl Serialize for GeminiVertexConfig {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let labels = self.labels.as_ref().map(|labels| {
            let pairs = labels.iter();
            let pairs = pairs.map(|(key, value)| (key.clone(), Value::from(value.as_str())));
            pairs.collect()
        });
        Written {
            api_key: masked(self.api_key.as_ref()),
            model: &self.model,
            max_tokens: self.max_tokens,
            thinking_level: self.thinking_level.as_deref(),
            timeout: self.timeout.as_secs_f64(),
            connect_timeout: self.connect_timeout.as_secs_f64(),
            project: &self.project,
            location: &self.location,
            service_account_json: masked(self.service_account_json.as_ref()),
            impersonate_service_account: self.impersonate_service_account.as_deref(),
            labels,
        }
        .serialize(serializer)
    }
}
