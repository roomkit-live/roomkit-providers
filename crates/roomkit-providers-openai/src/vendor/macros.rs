//! The provider and the configuration every vendor of the `OpenAI` wire
//! has, written once: Python's derivative classes inherit them.

/// A vendor's provider over [`crate::OpenAi`]: its rules, its name, its
/// catalogue; every request and answer as `OpenAI`'s wire carries them.
macro_rules! vendor_provider {
    ($(#[$doc:meta])* $provider:ident, $config:ident) => {
        $(#[$doc])*
        #[derive(Clone, Debug)]
        pub struct $provider {
            inner: $crate::OpenAi,
        }

        impl $provider {
            /// The provider `config` configures.
            ///
            /// # Errors
            ///
            /// A failure before any request: a configuration stating
            /// `supports_reasoning_effort_with_tools`, which the vendor's
            /// own rule makes moot, or `thinking_field`, as the provider
            /// sends reasoning where its vendor reads it; and what
            /// [`OpenAi::new`]($crate::OpenAi::new) refuses.
            pub fn new(config: $config) -> Result<Self, roomkit_ai::ProviderError> {
                let vendor = config.vendor();
                let refused = |failure| {
                    let name = vendor.provider();
                    Err(roomkit_providers_client::Exit::unbuilt(name, &failure))
                };
                if config.supports_reasoning_effort_with_tools.is_some() {
                    let config = stringify!($config);
                    return refused($crate::vendor::refused_tool_turn_setting(config));
                }
                if config.thinking_field.is_some() {
                    let config = stringify!($config);
                    return refused($crate::vendor::refused_thinking_field(config));
                }
                let inner = $crate::OpenAi::for_vendor(config.into_openai(), vendor)?;
                Ok(Self { inner })
            }

            /// The name the provider's failures carry: Python's `provider`.
            #[must_use]
            pub fn label(&self) -> &'static str {
                self.inner.label()
            }

            /// The provider a replay of recorded cases drives: as
            /// [`OpenAi::replaying`]($crate::OpenAi::replaying), its
            /// requests sent to `fake`, a loopback address, a date read
            /// against `clock`, the waits kept instead of taken.
            ///
            /// # Errors
            ///
            /// A `fake` that is no loopback address.
            #[cfg(feature = "replay")]
            #[doc(hidden)]
            pub fn replaying(
                self,
                fake: std::net::SocketAddr,
                clock: f64,
            ) -> Result<(Self, $crate::ReplayWaits), roomkit_ai::ProviderError> {
                let (inner, waits) = self.inner.replaying(fake, clock)?;
                Ok((Self { inner }, waits))
            }

            /// The `OpenAI` provider it runs on, for a replay of the cases
            /// captured on its Python provider.
            #[cfg(test)]
            pub(crate) fn openai_mut(&mut self) -> &mut $crate::OpenAi {
                &mut self.inner
            }
        }

        impl roomkit_ai::AiProvider for $provider {
            fn name(&self) -> &str {
                self.inner.name()
            }

            fn model_name(&self) -> &str {
                self.inner.model_name()
            }

            fn supports_vision(&self) -> bool {
                self.inner.supports_vision()
            }

            fn available_models() -> Vec<roomkit_ai::ModelInfo> {
                $config::VENDOR.models().to_vec()
            }

            fn catalog_entry(&self) -> Option<roomkit_ai::ModelInfo> {
                self.inner.catalog_entry()
            }

            async fn list_models(
                &self,
            ) -> Result<Vec<roomkit_ai::ModelInfo>, roomkit_ai::ProviderError> {
                self.inner.list_models().await
            }

            fn supports_streaming(&self) -> bool {
                true
            }

            fn supports_structured_streaming(&self) -> bool {
                true
            }

            fn supports_response_schema(&self) -> bool {
                self.inner.supports_response_schema()
            }

            fn supports_response_schema_with_tools(&self) -> bool {
                self.inner.supports_response_schema_with_tools()
            }

            async fn generate(
                &self,
                context: &roomkit_ai::AiContext,
            ) -> Result<roomkit_ai::AiResponse, roomkit_ai::ProviderError> {
                self.inner.generate(context).await
            }

            fn generate_stream<'a>(
                &'a self,
                context: &'a roomkit_ai::AiContext,
            ) -> roomkit_ai::TextStream<'a> {
                self.inner.generate_stream(context)
            }

            fn generate_structured_stream<'a>(
                &'a self,
                context: &'a roomkit_ai::AiContext,
            ) -> roomkit_ai::ProviderStream<'a> {
                self.inner.generate_structured_stream(context)
            }
        }
    };
}

pub(crate) use vendor_provider;

/// The fields every vendor's configuration shares with
/// [`crate::OpenAiConfig`], as Python's derivative configs inherit them,
/// carried into the `OpenAI` configuration the provider runs on.
macro_rules! into_openai {
    ($config:expr) => {{
        let config = $config;
        $crate::OpenAiConfig {
            base_url: Some(config.base_url),
            max_tokens: config.max_tokens,
            timeout: config.timeout,
            connect_timeout: config.connect_timeout,
            max_retries: config.max_retries,
            include_stream_usage: config.include_stream_usage,
            use_max_completion_tokens: Some(config.use_max_completion_tokens),
            supports_custom_temperature: Some(config.supports_custom_temperature),
            reasoning_effort: config.reasoning_effort,
            default_headers: config.default_headers,
            extra_body: config.extra_body,
            supports_response_schema: config.supports_response_schema,
            supports_response_schema_with_tools: config.supports_response_schema_with_tools,
            ..$crate::OpenAiConfig::new(config.api_key, config.model)
        }
    }};
}

pub(crate) use into_openai;

/// A vendor's configuration read and written as the JSON form of its Python
/// class, a derivative of `OpenAIConfig`: `OpenAIConfig`'s members in its
/// order, the vendor's defaults for what a form leaves out, then the
/// vendor's own switches (`Option<bool>`, `null` taken). The two members
/// the vendor's own rule makes moot, `supports_reasoning_effort_with_tools`
/// and `thinking_field`, are read only as `null`, as Python refuses any other
/// value. Each rule of `OpenAiConfig`'s form holds otherwise: an object only,
/// the timeouts in seconds, the key written masked.
macro_rules! vendor_config_form {
    ($config:ident, $expecting:literal $(, $own:ident)*) => {
        const _: () = {
            use std::collections::BTreeMap;
            use std::time::Duration;

            use roomkit::model::Metadata;
            use roomkit_providers_client::ObjectOnly;
            use serde::{Deserialize, Deserializer, Serialize, Serializer};

            use $crate::config::form::{MASK, left_null, seconds};

            fn defaults() -> $config {
                $config::new("", "")
            }
            fn base_url() -> String {
                defaults().base_url
            }
            fn max_tokens() -> Option<u32> {
                defaults().max_tokens
            }
            fn timeout() -> Duration {
                defaults().timeout
            }
            fn connect_timeout() -> Duration {
                defaults().connect_timeout
            }
            fn max_retries() -> u32 {
                defaults().max_retries
            }
            fn include_stream_usage() -> bool {
                defaults().include_stream_usage
            }
            fn use_max_completion_tokens() -> bool {
                defaults().use_max_completion_tokens
            }
            fn supports_custom_temperature() -> bool {
                defaults().supports_custom_temperature
            }

            /// The configuration as it is read.
            #[derive(Deserialize)]
            #[serde(expecting = $expecting)]
            struct Form {
                api_key: String,
                #[serde(default = "base_url")]
                base_url: String,
                model: String,
                #[serde(default = "max_tokens")]
                max_tokens: Option<u32>,
                #[serde(default = "timeout", deserialize_with = "seconds")]
                timeout: Duration,
                #[serde(default = "connect_timeout", deserialize_with = "seconds")]
                connect_timeout: Duration,
                #[serde(default = "max_retries")]
                max_retries: u32,
                #[serde(default = "include_stream_usage")]
                include_stream_usage: bool,
                #[serde(default = "use_max_completion_tokens")]
                use_max_completion_tokens: bool,
                #[serde(default = "supports_custom_temperature")]
                supports_custom_temperature: bool,
                #[serde(default)]
                reasoning_effort: Option<String>,
                #[serde(default)]
                default_headers: Option<BTreeMap<String, String>>,
                #[serde(default)]
                extra_body: Option<Metadata>,
                #[serde(default)]
                supports_response_schema: Option<bool>,
                #[serde(default)]
                supports_response_schema_with_tools: Option<bool>,
                // Read for their refusal of a value, never kept.
                #[serde(
                    default,
                    rename = "supports_reasoning_effort_with_tools",
                    deserialize_with = "left_null"
                )]
                _tool_turn_setting: (),
                #[serde(default, rename = "thinking_field", deserialize_with = "left_null")]
                _thinking_field: (),
                $(
                    #[serde(default)]
                    $own: Option<bool>,
                )*
            }

            impl<'de> Deserialize<'de> for $config {
                /// From a JSON object only, as Python refuses a list or a
                /// number.
                fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                    let form = Form::deserialize(ObjectOnly(deserializer))?;
                    Ok(Self {
                        api_key: form.api_key,
                        base_url: form.base_url,
                        model: form.model,
                        max_tokens: form.max_tokens,
                        timeout: form.timeout,
                        connect_timeout: form.connect_timeout,
                        max_retries: form.max_retries,
                        include_stream_usage: form.include_stream_usage,
                        use_max_completion_tokens: form.use_max_completion_tokens,
                        supports_custom_temperature: form.supports_custom_temperature,
                        reasoning_effort: form.reasoning_effort,
                        supports_reasoning_effort_with_tools: None,
                        thinking_field: None,
                        supports_response_schema: form.supports_response_schema,
                        supports_response_schema_with_tools: form.supports_response_schema_with_tools,
                        default_headers: form.default_headers,
                        extra_body: form.extra_body,
                        $($own: form.$own,)*
                    })
                }
            }

            /// The configuration as it is written: Python's members, in
            /// Python's order.
            #[derive(Serialize)]
            struct Written<'c> {
                api_key: &'static str,
                base_url: &'c str,
                model: &'c str,
                max_tokens: Option<u32>,
                timeout: f64,
                connect_timeout: f64,
                max_retries: u32,
                include_stream_usage: bool,
                use_max_completion_tokens: bool,
                supports_custom_temperature: bool,
                reasoning_effort: Option<&'c str>,
                default_headers: Option<&'c BTreeMap<String, String>>,
                extra_body: Option<&'c Metadata>,
                supports_response_schema: Option<bool>,
                supports_response_schema_with_tools: Option<bool>,
                supports_reasoning_effort_with_tools: Option<bool>,
                thinking_field: Option<&'c str>,
                $($own: Option<bool>,)*
            }

            impl Serialize for $config {
                fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                    Written {
                        api_key: if self.api_key.is_empty() { "" } else { MASK },
                        base_url: &self.base_url,
                        model: &self.model,
                        max_tokens: self.max_tokens,
                        timeout: self.timeout.as_secs_f64(),
                        connect_timeout: self.connect_timeout.as_secs_f64(),
                        max_retries: self.max_retries,
                        include_stream_usage: self.include_stream_usage,
                        use_max_completion_tokens: self.use_max_completion_tokens,
                        supports_custom_temperature: self.supports_custom_temperature,
                        reasoning_effort: self.reasoning_effort.as_deref(),
                        default_headers: self.default_headers.as_ref(),
                        extra_body: self.extra_body.as_ref(),
                        supports_response_schema: self.supports_response_schema,
                        supports_response_schema_with_tools: self.supports_response_schema_with_tools,
                        supports_reasoning_effort_with_tools: self.supports_reasoning_effort_with_tools,
                        thinking_field: self.thinking_field.as_deref(),
                        $($own: self.$own,)*
                    }
                    .serialize(serializer)
                }
            }
        };
    };
}

pub(crate) use vendor_config_form;
