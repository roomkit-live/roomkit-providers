//! Azure: Python's `roomkit.providers.azure`, its provider over the Chat
//! Completions API of an Azure resource's deployments, and its
//! configuration.

use std::fmt;
use std::time::Duration;

use roomkit::model::Metadata;
use roomkit_ai::ProviderError;
use roomkit_providers_client::{Exit, ObjectOnly};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::config::form::{MASK, seconds};
use crate::vendor::{Vendor, vendor_provider};

mod endpoint;

pub(crate) use endpoint::resource_base;

/// The API version a configuration that states none asks.
const API_VERSION: &str = "2024-12-01-preview";

/// An Azure resource's deployment and how it is called: Python's
/// `AzureAIConfig`, built with [`AzureAiConfig::new`] and struct update.
/// It is no `OpenAIConfig`: an endpoint and an API version in place of a
/// base URL, the key in Azure's `api-key` header, no default headers and no
/// thinking field.
///
/// Its `Debug` never prints the API key, nor the endpoint's user
/// information, path or query, which may carry credentials.
#[derive(Clone)]
pub struct AzureAiConfig {
    /// The resource's key, sent as its `api-key` header.
    pub api_key: String,
    /// The resource's endpoint, as Azure hands it out: its root, the v1
    /// API's URL or a deployment's target URI, the provider keeping the
    /// root alone (Python's `normalize_azure_endpoint`).
    pub azure_endpoint: String,
    /// The API version every request asks.
    pub api_version: String,
    /// The deployment asked, a name its operator chose.
    pub model: String,
    /// The token bound.
    pub max_tokens: u32,
    /// The longest wait between two reads of an answer.
    pub timeout: Duration,
    /// The longest wait for a connection.
    pub connect_timeout: Duration,
    /// How many times a failed request is tried again.
    pub max_retries: u32,
    /// Whether a streamed answer asks for its usage
    /// (`stream_options.include_usage`).
    pub include_stream_usage: bool,
    /// Whether the bound goes as `max_completion_tokens` rather than
    /// `max_tokens`, which newer models of Azure require.
    pub use_max_completion_tokens: bool,
    /// Whether the turn's temperature is sent: a reasoning deployment takes
    /// only its default.
    pub supports_custom_temperature: bool,
    /// The reasoning depth; the turn's own outranks it.
    pub reasoning_effort: Option<String>,
    /// Members merged into every request's body, the turn's own winning.
    pub extra_body: Option<Metadata>,
    /// Whether the deployment honours a `json_schema` response format (RFC
    /// §6.7): `None` leaves it to the provider's default.
    pub supports_response_schema: Option<bool>,
    /// Whether the deployment takes a `json_schema` response format beside
    /// function tools (RFC §6.7): `None` leaves it to the provider, which
    /// sends it.
    pub supports_response_schema_with_tools: Option<bool>,
    /// Whether the deployment takes a reasoning setting beside function
    /// tools (RFC §6.7): `None` and `false` leave it out, a deployment's
    /// name saying nothing of its model, `None` warning of it once.
    pub supports_reasoning_effort_with_tools: Option<bool>,
}

impl AzureAiConfig {
    /// The vendor whose rules the provider applies.
    pub(crate) const VENDOR: Vendor = Vendor::Azure;

    /// The configuration calling the deployment `model` of the resource at
    /// `azure_endpoint` with `api_key`, with Python's defaults.
    pub fn new(
        api_key: impl Into<String>,
        azure_endpoint: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            azure_endpoint: azure_endpoint.into(),
            api_version: API_VERSION.to_owned(),
            model: model.into(),
            max_tokens: 1024,
            timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(5),
            max_retries: 0,
            include_stream_usage: true,
            use_max_completion_tokens: false,
            supports_custom_temperature: true,
            reasoning_effort: None,
            extra_body: None,
            supports_response_schema: None,
            supports_response_schema_with_tools: None,
            supports_reasoning_effort_with_tools: None,
        }
    }

    /// The provider the configuration builds: `OpenAI`'s wire at the
    /// resource's base, the deployment in the path, the API version in the
    /// query, the key in `api-key`.
    pub(crate) fn provider(self) -> Result<crate::OpenAi, ProviderError> {
        let label = Self::VENDOR.provider();
        let Some(base) = resource_base(&self.azure_endpoint) else {
            let failure = crate::failure::Failure::unbuilt("its endpoint does not read as a URL");
            return Err(Exit::unbuilt(label, &failure));
        };
        let api_version = self.api_version.clone();
        let openai = crate::OpenAiConfig {
            base_url: Some(base),
            max_tokens: Some(self.max_tokens),
            timeout: self.timeout,
            connect_timeout: self.connect_timeout,
            max_retries: self.max_retries,
            include_stream_usage: self.include_stream_usage,
            use_max_completion_tokens: Some(self.use_max_completion_tokens),
            supports_custom_temperature: Some(self.supports_custom_temperature),
            reasoning_effort: self.reasoning_effort,
            supports_reasoning_effort_with_tools: self.supports_reasoning_effort_with_tools,
            extra_body: self.extra_body,
            supports_response_schema: self.supports_response_schema,
            supports_response_schema_with_tools: self.supports_response_schema_with_tools,
            ..crate::OpenAiConfig::new(self.api_key, self.model)
        };
        crate::OpenAi::for_azure(openai, &api_version)
    }
}

impl fmt::Debug for AzureAiConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let api_key = if self.api_key.is_empty() {
            "none"
        } else {
            "hidden"
        };
        f.debug_struct("AzureAiConfig")
            .field(
                "azure_endpoint",
                &crate::endpoint::shown_url(&self.azure_endpoint),
            )
            .field("api_key", &api_key)
            .field("api_version", &self.api_version)
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .field("reasoning_effort", &self.reasoning_effort)
            .finish_non_exhaustive()
    }
}

// Python's `AzureAIConfig.model_dump(mode="json")`, read and written: its
// members in its order, the endpoint reduced to the resource's base as it is
// read, the key written masked, a member Python does not have ignored.

fn defaults() -> AzureAiConfig {
    AzureAiConfig::new("", "", "")
}
fn api_version() -> String {
    defaults().api_version
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
fn include_stream_usage() -> bool {
    defaults().include_stream_usage
}
fn supports_custom_temperature() -> bool {
    defaults().supports_custom_temperature
}

/// The configuration as it is read.
#[derive(Deserialize)]
#[serde(expecting = "an Azure configuration, as a JSON object")]
struct Form {
    api_key: String,
    azure_endpoint: String,
    #[serde(default = "api_version")]
    api_version: String,
    model: String,
    #[serde(default = "max_tokens")]
    max_tokens: u32,
    #[serde(default = "timeout", deserialize_with = "seconds")]
    timeout: Duration,
    #[serde(default = "connect_timeout", deserialize_with = "seconds")]
    connect_timeout: Duration,
    #[serde(default)]
    max_retries: u32,
    #[serde(default = "include_stream_usage")]
    include_stream_usage: bool,
    #[serde(default)]
    use_max_completion_tokens: bool,
    #[serde(default = "supports_custom_temperature")]
    supports_custom_temperature: bool,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    extra_body: Option<Metadata>,
    #[serde(default)]
    supports_response_schema: Option<bool>,
    #[serde(default)]
    supports_response_schema_with_tools: Option<bool>,
    #[serde(default)]
    supports_reasoning_effort_with_tools: Option<bool>,
}

impl<'de> Deserialize<'de> for AzureAiConfig {
    /// From a JSON object only, as Python refuses a list or a number.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let form = Form::deserialize(ObjectOnly(deserializer))?;
        let Some(azure_endpoint) = resource_base(&form.azure_endpoint) else {
            return Err(D::Error::custom("azure_endpoint does not read as a URL"));
        };
        Ok(Self {
            api_key: form.api_key,
            azure_endpoint,
            api_version: form.api_version,
            model: form.model,
            max_tokens: form.max_tokens,
            timeout: form.timeout,
            connect_timeout: form.connect_timeout,
            max_retries: form.max_retries,
            include_stream_usage: form.include_stream_usage,
            use_max_completion_tokens: form.use_max_completion_tokens,
            supports_custom_temperature: form.supports_custom_temperature,
            reasoning_effort: form.reasoning_effort,
            extra_body: form.extra_body,
            supports_response_schema: form.supports_response_schema,
            supports_response_schema_with_tools: form.supports_response_schema_with_tools,
            supports_reasoning_effort_with_tools: form.supports_reasoning_effort_with_tools,
        })
    }
}

/// The configuration as it is written.
#[derive(Serialize)]
struct Written<'c> {
    api_key: &'static str,
    azure_endpoint: &'c str,
    api_version: &'c str,
    model: &'c str,
    max_tokens: u32,
    timeout: f64,
    connect_timeout: f64,
    max_retries: u32,
    include_stream_usage: bool,
    use_max_completion_tokens: bool,
    supports_custom_temperature: bool,
    reasoning_effort: Option<&'c str>,
    extra_body: Option<&'c Metadata>,
    supports_response_schema: Option<bool>,
    supports_response_schema_with_tools: Option<bool>,
    supports_reasoning_effort_with_tools: Option<bool>,
}

impl Serialize for AzureAiConfig {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Written {
            api_key: if self.api_key.is_empty() { "" } else { MASK },
            azure_endpoint: &self.azure_endpoint,
            api_version: &self.api_version,
            model: &self.model,
            max_tokens: self.max_tokens,
            timeout: self.timeout.as_secs_f64(),
            connect_timeout: self.connect_timeout.as_secs_f64(),
            max_retries: self.max_retries,
            include_stream_usage: self.include_stream_usage,
            use_max_completion_tokens: self.use_max_completion_tokens,
            supports_custom_temperature: self.supports_custom_temperature,
            reasoning_effort: self.reasoning_effort.as_deref(),
            extra_body: self.extra_body.as_ref(),
            supports_response_schema: self.supports_response_schema,
            supports_response_schema_with_tools: self.supports_response_schema_with_tools,
            supports_reasoning_effort_with_tools: self.supports_reasoning_effort_with_tools,
        }
        .serialize(serializer)
    }
}

vendor_provider!(
    /// An Azure resource's deployment (RFC §6.7): Python's
    /// `AzureAIProvider(config)`. `OpenAI`'s wire at
    /// `{endpoint}/openai/deployments/{deployment}`, the API version in the
    /// query and the key in `api-key`; a reasoning setting on a turn with
    /// tools only where the configuration says the deployment takes it, a
    /// response schema beside tools unless it says otherwise; whether it
    /// reads images is the deployment's to answer. A call with an empty key
    /// is refused before any request.
    AzureAi,
    AzureAiConfig,
    built by its configuration
);
