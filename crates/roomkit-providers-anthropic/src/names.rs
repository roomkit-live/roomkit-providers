//! The tool names Anthropic's own endpoint accepts (RFC §6.7): checked when
//! a request declares its tools, so that a name the vendor refuses fails
//! before anything is sent, naming the tool and the rule, as Python's
//! provider checks them (`ANTHROPIC_TOOL_NAMES`).

use roomkit_ai::{AiTool, ProviderError, ProviderErrorOptions};
use roomkit_core::rules::python_repr;

/// The names Anthropic's endpoint accepts, written as Python's pattern is.
const RULE: &str = "[A-Za-z0-9_-]{1,128}";

/// The longest name the rule takes.
const LONGEST: usize = 128;

/// The first of `tools` whose name Anthropic's endpoint refuses, as the
/// error Python raises for it.
///
/// # Errors
///
/// A failure before any request, naming the tool and the rule.
pub(crate) fn check(tools: &[AiTool], provider: &str) -> Result<(), ProviderError> {
    let Some(tool) = tools.iter().find(|tool| !accepted(&tool.name)) else {
        return Ok(());
    };
    // The name is quoted as Python's `repr` quotes it.
    let name = python_repr(&tool.name);
    let message = format!("tool {name}: anthropic accepts tool names matching {RULE}");
    let options = ProviderErrorOptions {
        provider: provider.to_owned(),
        context_overflow: Some(false),
        before_request: true,
        ..ProviderErrorOptions::default()
    };
    Err(ProviderError::new(message, options))
}

/// Whether Anthropic's endpoint accepts `name` for a tool.
fn accepted(name: &str) -> bool {
    let sign = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-');
    !name.is_empty() && name.len() <= LONGEST && name.chars().all(sign)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_anthropic_refuses_is_named_with_the_rule() {
        let refused =
            |name: &str| check(&[AiTool::new(name, "Reads a file").unwrap()], "anthropic");
        let (long, longest) = ("d".repeat(129), "c".repeat(128));
        for name in ["files.read", "files:read", long.as_str()] {
            let error = refused(name).unwrap_err();
            assert!(error.before_request && !error.retryable, "{name}");
            assert_eq!(error.context_overflow, Some(false), "{name}");
            assert_eq!(error.provider, "anthropic", "{name}");
        }
        assert_eq!(
            refused("files.read").unwrap_err().message,
            "tool 'files.read': anthropic accepts tool names matching [A-Za-z0-9_-]{1,128}"
        );
        for name in [longest.as_str(), "files_read-2"] {
            assert_eq!(refused(name), Ok(()), "{name}");
        }
    }
}
