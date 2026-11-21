//! What an Anthropic provider is configured with: Python's
//! `AnthropicConfig`.

mod form;

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use crate::endpoint::{ANTHROPIC_BASE_URL, is_anthropic_endpoint, shown_url};

/// The path of the Messages API, which a base URL pasted whole ends with.
const MESSAGES_PATH: &str = "/v1/messages";

/// The endpoint a provider calls and how: Python's `AnthropicConfig(*,
/// api_key, model, max_tokens=1024, timeout=60.0, connect_timeout=5.0,
/// max_retries=0, base_url=None, extra_headers=None,
/// enable_prompt_caching=True, use_adaptive_thinking=False,
/// supports_custom_temperature=True)`, built with [`AnthropicConfig::new`]
/// and struct update.
///
/// ```
/// use std::time::Duration;
///
/// use roomkit_providers_anthropic::AnthropicConfig;
///
/// let config = AnthropicConfig {
///     base_url: Some("http://localhost:8080".to_owned()),
///     timeout: Duration::from_secs(120),
///     ..AnthropicConfig::new("placeholder-not-a-key", "local-model")
/// };
/// assert_eq!(config.max_tokens, 1024);
/// // Neither the key nor what follows the host of the base URL is shown.
/// let shown = format!("{config:?}");
/// assert!(shown.contains("http://localhost:8080") && !shown.contains("placeholder"));
/// ```
///
/// # JSON form
///
/// The configuration reads and writes the JSON that Python's
/// `AnthropicConfig` gives with `model_dump(mode="json")`: the same members,
/// in the same order, the timeouts in seconds. As in Python, the key is
/// written masked, `**********`, an empty one as an empty text: a
/// configuration written then read back holds the mask, not the key.
///
/// The base URL is written as Python keeps it: without the white space
/// around it, the slashes that end it and a `/v1/messages` it ends with,
/// which the provider adds itself. A configuration holds the base URL it
/// was given, read or built, and the provider reads it as it is written.
///
/// The two switches Python profiles are written as Python writes them,
/// decided: `true` or `false`. A member left out of what is read leaves the
/// switch unset, and a `true` or a `false` states it.
///
/// Input is stricter than Python's. Each of these is refused here and taken
/// by Python:
///
/// - a value pydantic would convert: `"30"` for a number, `2048.0` for an
///   integer, `1` for a boolean;
/// - a `max_tokens` or a `max_retries` outside 32 bits without a sign;
/// - a `timeout` or a `connect_timeout` below zero, or past what a
///   [`Duration`] holds; one is kept to the nanosecond.
///
/// `extra_headers` are written sorted by name, where Python keeps the order
/// they were given in. A member written twice in the JSON text is refused,
/// where Python keeps the last: this comes from the JSON parser.
///
/// The form is not for showing: like Python's, it writes an extra header's
/// value and the credentials a base URL carries whole. Only the key is
/// masked. The `Debug` form hides all three.
///
/// # Debug
///
/// The `Debug` form never prints the API key, nor the base URL's user
/// information, path or query, which may carry credentials, nor an extra
/// header's value. Of the base URL it shows the origin the HTTP client
/// dials for it, read as the client's own URL parser reads it.
#[derive(Clone, PartialEq)]
pub struct AnthropicConfig {
    /// The key a request carries in its `x-api-key` header, unless the
    /// request brings its own.
    pub api_key: String,
    /// The model asked.
    pub model: String,
    /// The most tokens a generation writes, unless its request says.
    pub max_tokens: u32,
    /// How long a request waits for the endpoint between two reads
    /// (Python's `timeout`, in seconds there).
    pub timeout: Duration,
    /// How long a request waits for its connection (Python's
    /// `connect_timeout`, in seconds there).
    pub connect_timeout: Duration,
    /// How many times a request is tried again on its own. 0 by default: a
    /// host's retry policy is the layer that retries, with its fallback.
    pub max_retries: u32,
    /// The endpoint's base URL; `None` calls Anthropic's own. One that ends
    /// with `/v1/messages` is read without it: the provider adds the path.
    pub base_url: Option<String>,
    /// Headers every request carries. One replaces the provider's own of
    /// the same name, its key included.
    pub extra_headers: Option<BTreeMap<String, String>>,
    /// Whether the stable prefix of a request is marked for Anthropic's
    /// prompt caching: its tools, its system prompt and its last two
    /// messages. Off for a proxy that refuses the marks.
    pub enable_prompt_caching: bool,
    /// Whether a turn that thinks asks for adaptive thinking rather than a
    /// budget, which Anthropic's modern models refuse. `None`, a switch
    /// left unset, is Python's profile: on Anthropic's own endpoint a
    /// modern Claude model thinks adaptively, any other on a budget.
    pub use_adaptive_thinking: Option<bool>,
    /// Whether the model takes a temperature: one that refuses it is sent
    /// none. `None`, a switch left unset, is Python's profile: on
    /// Anthropic's own endpoint a modern Claude model takes none, any other
    /// does.
    pub supports_custom_temperature: Option<bool>,
}

impl AnthropicConfig {
    /// The configuration calling Anthropic's own endpoint with `api_key`
    /// for `model`, with Python's defaults.
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            max_tokens: 1024,
            timeout: Duration::from_secs(60),
            connect_timeout: Duration::from_secs(5),
            max_retries: 0,
            base_url: None,
            extra_headers: None,
            enable_prompt_caching: true,
            use_adaptive_thinking: None,
            supports_custom_temperature: None,
        }
    }

    /// The base URL as Python's configuration keeps it
    /// (`_drop_the_path_the_sdk_appends`): without the white space around
    /// it, the slashes that end it, and the path of the Messages API it
    /// ends with, in any case. What is left when that leaves nothing is the
    /// base URL without its white space.
    pub(crate) fn kept_base_url(&self) -> Option<String> {
        self.base_url.as_deref().map(kept)
    }

    /// Whether a turn that thinks asks for adaptive thinking: the switch
    /// when set, else Python's profile of a modern model on Anthropic's own
    /// endpoint (`AnthropicConfig.model_post_init`).
    pub(crate) fn thinks_adaptively(&self) -> bool {
        self.use_adaptive_thinking
            .unwrap_or_else(|| self.profiled())
    }

    /// Whether a temperature is sent: the switch when set, else unless
    /// Python's profile says the model takes none.
    pub(crate) fn takes_temperature(&self) -> bool {
        self.supports_custom_temperature
            .unwrap_or_else(|| !self.profiled())
    }

    /// Whether the model is a modern Claude on Anthropic's own endpoint.
    fn profiled(&self) -> bool {
        is_anthropic_endpoint(self.kept_base_url().as_deref()) && is_modern_claude(&self.model)
    }
}

/// `base_url` as Python's configuration keeps it.
fn kept(base_url: &str) -> String {
    // What Python's `str.strip()` strips: the white space Rust knows, and
    // the four separators of information, U+001C to U+001F.
    let space = |c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c);
    let stripped = base_url.trim_matches(space);
    let trimmed = stripped.trim_end_matches('/');
    let before_the_path = trimmed.len().checked_sub(MESSAGES_PATH.len());
    let pasted = before_the_path.and_then(|at| {
        let (base, path) = trimmed.split_at_checked(at)?;
        path.eq_ignore_ascii_case(MESSAGES_PATH).then_some(base)
    });
    let trimmed = pasted.unwrap_or(trimmed);
    if trimmed.is_empty() {
        stripped.to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Whether `model` is a Claude model past the 4.6 generation, which takes
/// adaptive thinking and no temperature: Python's `_is_modern_claude`. The
/// old models are the listed ones, so that a model released after this
/// crate is a modern one.
fn is_modern_claude(model: &str) -> bool {
    let Some(name) = model.strip_prefix("claude-") else {
        return false;
    };
    if ["2", "3", "instant"]
        .iter()
        .any(|old| name.starts_with(old))
    {
        return false;
    }
    let families = ["opus-4-", "sonnet-4-", "haiku-4-"];
    let version = families.iter().find_map(|family| name.strip_prefix(family));
    !version.is_some_and(is_old_version)
}

/// Whether `version`, what follows `claude-<family>-4-` in a model's name,
/// is one of the 4.6 generation or before: a digit from 0 to 6 that ends a
/// word, or the eight digits of a dated snapshot.
fn is_old_version(version: &str) -> bool {
    let mut characters = version.chars();
    let first = characters.next();
    let second = characters.next();
    let continues = second.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let minor = first.is_some_and(|c| ('0'..='6').contains(&c)) && !continues;
    let dated = version.chars().take(8).filter(char::is_ascii_digit).count() == 8;
    minor || dated
}

impl fmt::Debug for AnthropicConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let api_key = if self.api_key.is_empty() {
            "none"
        } else {
            "hidden"
        };
        let base_url = self.kept_base_url();
        let base_url = base_url.as_deref().unwrap_or(ANTHROPIC_BASE_URL);
        let headers: Option<Vec<&String>> = self
            .extra_headers
            .as_ref()
            .map(|headers| headers.keys().collect());
        f.debug_struct("AnthropicConfig")
            .field("base_url", &shown_url(base_url))
            .field("api_key", &api_key)
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .field("timeout", &self.timeout)
            .field("connect_timeout", &self.connect_timeout)
            .field("max_retries", &self.max_retries)
            .field("enable_prompt_caching", &self.enable_prompt_caching)
            .field("use_adaptive_thinking", &self.use_adaptive_thinking)
            .field(
                "supports_custom_temperature",
                &self.supports_custom_temperature,
            )
            .field("extra_headers", &headers)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "placeholder-not-a-key";

    // Source: src/tests.rs::official_profiles_preserve_legacy_custom_and_explicit_settings
    // Python: tests/test_providers/test_anthropic.py::test_config_preserves_legacy_custom_and_explicit_settings
    #[test]
    fn a_modern_model_is_profiled_on_anthropic_s_endpoint_unless_the_switch_is_set() {
        let profile =
            |config: &AnthropicConfig| (config.thinks_adaptively(), config.takes_temperature());
        for (model, adaptive, temperature) in [
            ("claude-sonnet-4-6", false, true),
            ("claude-sonnet-4-20250514", false, true),
            ("claude-opus-5-5", true, false),
            ("claude-future-8", true, false),
            ("custom-model", false, true),
        ] {
            let config = AnthropicConfig::new(KEY, model);
            assert_eq!(profile(&config), (adaptive, temperature), "{model}");
            let custom = AnthropicConfig {
                base_url: Some("https://gateway.example".to_owned()),
                ..config.clone()
            };
            assert_eq!(profile(&custom), (false, true), "{model}");
            let explicit = AnthropicConfig {
                use_adaptive_thinking: Some(false),
                supports_custom_temperature: Some(true),
                ..config
            };
            assert_eq!(profile(&explicit), (false, true), "{model}");
        }
    }

    // Python: tests/test_providers/test_anthropic.py::test_the_appended_path_is_dropped
    #[test]
    fn a_base_url_is_kept_without_the_path_the_provider_adds() {
        for (given, expected) in [
            (
                " https://host/anthropic/v1/messages/ ",
                "https://host/anthropic",
            ),
            ("https://host/V1/Messages", "https://host"),
            ("https://host/v1", "https://host/v1"),
            (
                "https://host/v1/messages/v1/messages",
                "https://host/v1/messages",
            ),
            ("/v1/messages", "/v1/messages"),
            ("https://", "https:"),
            (" ", ""),
            ("/", "/"),
            (
                "https://host/caf\u{e9}/v1/messages",
                "https://host/caf\u{e9}",
            ),
        ] {
            assert_eq!(kept(given), expected, "{given:?}");
        }
    }

    #[test]
    fn the_debug_form_hides_the_key_the_url_s_credentials_and_a_header_s_value() {
        let config = AnthropicConfig {
            base_url: Some(
                "https://user:placeholder-in-the-url@llm.example:8443/placeholder-path?k=placeholder"
                    .to_owned(),
            ),
            extra_headers: Some(BTreeMap::from([(
                "Authorization".to_owned(),
                "Bearer placeholder-in-a-header".to_owned(),
            )])),
            ..AnthropicConfig::new(KEY, "local-model")
        };
        let shown = format!("{config:?}");
        assert!(!shown.contains("placeholder"), "{shown}");
        assert!(shown.contains("https://llm.example:8443"), "{shown}");
        assert!(shown.contains("api_key: \"hidden\""), "{shown}");
        assert!(shown.contains("Authorization"), "{shown}");
        let keyless = AnthropicConfig::new("", "local-model");
        assert!(format!("{keyless:?}").contains("api_key: \"none\""));
    }
}
