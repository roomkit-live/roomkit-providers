//! The defaults of `AnthropicConfig`: those of `RoomKit`'s Python
//! `AnthropicConfig`.

use std::time::Duration;

use roomkit_providers_anthropic::AnthropicConfig;

// Source: tests/api_defaults.rs::anthropic_defaults_match_python
#[test]
fn the_defaults_are_python_s() {
    let config = AnthropicConfig::new("placeholder-not-a-key", "model");
    // roomkit.providers.anthropic.AnthropicConfig.__init__(max_tokens) = 1024
    assert_eq!(config.max_tokens, 1024);
    // roomkit.providers.anthropic.AnthropicConfig.__init__(timeout) = 60.0
    assert_eq!(config.timeout, Duration::from_secs(60));
    // roomkit.providers.anthropic.AnthropicConfig.__init__(connect_timeout) = 5.0
    assert_eq!(config.connect_timeout, Duration::from_secs(5));
    // roomkit.providers.anthropic.AnthropicConfig.__init__(max_retries) = 0
    assert_eq!(config.max_retries, 0);
    // roomkit.providers.anthropic.AnthropicConfig.__init__(base_url) = None
    assert_eq!(config.base_url, None);
    // roomkit.providers.anthropic.AnthropicConfig.__init__(extra_headers) = None
    assert_eq!(config.extra_headers, None);
    // roomkit.providers.anthropic.AnthropicConfig.__init__(enable_prompt_caching) = True
    assert!(config.enable_prompt_caching);
    // `None` is a switch left unset, which the profile of the model decides
    // where Python's `model_post_init` does.
    // roomkit.providers.anthropic.AnthropicConfig.__init__(use_adaptive_thinking) = False
    assert_eq!(config.use_adaptive_thinking, None);
    // roomkit.providers.anthropic.AnthropicConfig.__init__(supports_custom_temperature) = True
    assert_eq!(config.supports_custom_temperature, None);
    // Written, the switches are decided as Python's defaults are for a
    // model that is no modern Claude.
    let written = serde_json::to_value(&config).unwrap_or_default();
    assert_eq!(written["use_adaptive_thinking"], false);
    assert_eq!(written["supports_custom_temperature"], true);
}
