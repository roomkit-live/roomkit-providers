//! The tool names Gemini accepts (RFC §6.7): checked when a request declares
//! its tools, so that a name the vendor refuses fails before anything is
//! sent, naming the tool and the rule, as Python's provider checks them
//! (`GEMINI_TOOL_NAMES`). A dot and a colon are taken, a leading digit is
//! not.

use roomkit_ai::{AiTool, ProviderError, ProviderErrorOptions};
use roomkit_core::rules::python_repr;

/// The rule as Python writes it in its refusal.
const RULE: &str = "[A-Za-z_][A-Za-z0-9_.:-]{0,127}";

/// The longest name the rule takes.
const LONGEST: usize = 128;

/// Refuses the first of `tools` whose name Gemini does not take.
pub(crate) fn check(tools: &[AiTool], provider: &str) -> Result<(), ProviderError> {
    let Some(tool) = tools.iter().find(|tool| !accepted(&tool.name)) else {
        return Ok(());
    };
    // The name is quoted as Python's `repr` quotes it.
    let name = python_repr(&tool.name);
    let message = format!("tool {name}: gemini accepts tool names matching {RULE}");
    let options = ProviderErrorOptions {
        provider: provider.to_owned(),
        context_overflow: Some(false),
        before_request: true,
        ..ProviderErrorOptions::default()
    };
    Err(ProviderError::new(message, options))
}

fn accepted(name: &str) -> bool {
    let first = |c: char| c.is_ascii_alphabetic() || c == '_';
    let sign = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-');
    name.len() <= LONGEST && name.starts_with(first) && name.chars().all(sign)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_gemini_refuses_is_named_with_the_rule() {
        let refused = |name: &str| check(&[AiTool::new(name, "Reads a file").unwrap()], "gemini");
        let (long, longest) = ("d".repeat(129), "c".repeat(128));
        for name in ["9_lookup", "-lookup", long.as_str()] {
            let error = refused(name).unwrap_err();
            assert!(error.before_request && !error.retryable, "{name}");
            assert_eq!(error.context_overflow, Some(false), "{name}");
        }
        for name in [longest.as_str(), "files.read", "files:read", "_x-2"] {
            assert_eq!(refused(name), Ok(()), "{name}");
        }
    }
}
