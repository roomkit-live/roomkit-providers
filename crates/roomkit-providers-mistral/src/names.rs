//! The tool names Mistral accepts (RFC §6.7): checked when a request to
//! Mistral's own endpoint declares its tools, so that a name the vendor
//! refuses fails before anything is sent, naming the tool and the rule, as
//! Python's provider checks them (`MISTRAL_TOOL_NAMES`). A dot is taken, a
//! colon is not. Behind another server URL the server decides its names.

use roomkit_ai::{AiTool, ProviderError, ProviderErrorOptions};
use roomkit_core::rules::python_repr;

/// The rule as Python writes it in its refusal.
const RULE: &str = "[A-Za-z0-9_.-]+";

/// Refuses the first of `tools` whose name Mistral does not take.
pub(crate) fn check(tools: &[AiTool], provider: &str) -> Result<(), ProviderError> {
    let Some(tool) = tools.iter().find(|tool| !accepted(&tool.name)) else {
        return Ok(());
    };
    // The name is quoted as Python's `repr` quotes it.
    let name = python_repr(&tool.name);
    let message = format!("tool {name}: mistral accepts tool names matching {RULE}");
    let options = ProviderErrorOptions {
        provider: provider.to_owned(),
        context_overflow: Some(false),
        before_request: true,
        ..ProviderErrorOptions::default()
    };
    Err(ProviderError::new(message, options))
}

fn accepted(name: &str) -> bool {
    let sign = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-');
    !name.is_empty() && name.chars().all(sign)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_mistral_refuses_is_named_with_the_rule() {
        let refused = |name: &str| check(&[AiTool::new(name, "Reads a file").unwrap()], "mistral");
        for name in ["orders:lookup", "files:read"] {
            let error = refused(name).unwrap_err();
            assert!(error.before_request && !error.retryable, "{name}");
            assert_eq!(error.context_overflow, Some(false), "{name}");
        }
        for name in ["files.read", "9_lookup", "-x", "_x-2"] {
            assert_eq!(refused(name), Ok(()), "{name}");
        }
    }
}
