//! The tool names a vendor's own endpoint accepts (RFC §6.7): checked when
//! a request declares its tools, so that a name the vendor refuses fails
//! before anything is sent, naming the tool and the rule, as Python's
//! providers check them. Behind another base URL the server decides.

use roomkit_ai::{AiTool, ProviderError, ProviderErrorOptions};

/// The longest name `OpenAI`'s rule takes.
const LONGEST: usize = 128;

/// A rule on the tool names a vendor accepts, as Python's `ToolNameRule`.
pub(crate) struct NameRule {
    vendor: &'static str,
    /// The names the vendor accepts, written as Python's pattern is.
    pattern: &'static str,
    accepted: fn(&str) -> bool,
}

/// `OpenAI`'s rule.
pub(crate) const OPENAI: NameRule = NameRule {
    vendor: "openai",
    pattern: "[A-Za-z0-9_-]{1,128}",
    accepted: letters_digits_dashes,
};

/// DeepSeek's rule, which is `OpenAI`'s (measured by Python on 2026-10-02).
pub(crate) const DEEPSEEK: NameRule = NameRule {
    vendor: "deepseek",
    pattern: "[A-Za-z0-9_-]{1,128}",
    accepted: letters_digits_dashes,
};

/// Kimi's rule, from its API reference: a letter or `_` first.
pub(crate) const KIMI: NameRule = NameRule {
    vendor: "kimi",
    pattern: "[a-zA-Z_][a-zA-Z0-9_-]{0,127}",
    accepted: kimi_name,
};

fn letters_digits_dashes(name: &str) -> bool {
    let sign = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-');
    !name.is_empty() && name.len() <= LONGEST && name.chars().all(sign)
}

fn kimi_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') && letters_digits_dashes(name)
}

impl NameRule {
    /// The first of `tools` whose name the rule refuses, as the error
    /// Python raises for it, which names `provider`.
    ///
    /// # Errors
    ///
    /// A failure before any request, naming the tool and the rule.
    pub(crate) fn check(&self, tools: &[AiTool], provider: &str) -> Result<(), ProviderError> {
        let Some(tool) = tools.iter().find(|tool| !(self.accepted)(&tool.name)) else {
            return Ok(());
        };
        // A tool's name holds no quote and nothing Python's `repr` escapes.
        let message = format!(
            "tool '{}': {} accepts tool names matching {}",
            tool.name, self.vendor, self.pattern
        );
        let options = ProviderErrorOptions {
            provider: provider.to_owned(),
            context_overflow: Some(false),
            before_request: true,
            ..ProviderErrorOptions::default()
        };
        Err(ProviderError::new(message, options))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str) -> AiTool {
        AiTool::new(name, "Reads a file").unwrap()
    }

    #[test]
    fn a_name_openai_refuses_is_named_with_the_rule() {
        let refused = |name: &str| OPENAI.check(&[tool(name)], "openai");
        let (long, longest) = ("d".repeat(129), "c".repeat(128));
        for name in ["files.read", "files:read", long.as_str()] {
            let error = refused(name).unwrap_err();
            assert!(error.before_request && !error.retryable, "{name}");
            assert_eq!(error.context_overflow, Some(false), "{name}");
        }
        assert_eq!(
            refused("files.read").unwrap_err().message,
            "tool 'files.read': openai accepts tool names matching [A-Za-z0-9_-]{1,128}"
        );
        for name in [longest.as_str(), "files_read-2"] {
            assert_eq!(refused(name), Ok(()), "{name}");
        }
    }

    // Python: tests/test_providers/test_minimax_kimi.py::test_kimi_s_tool_names_are_checked_before_the_request
    #[test]
    fn kimi_s_tool_names_follow_its_documented_rule() {
        let accepted = |name: &str| KIMI.check(&[tool(name)], "kimi").is_ok();
        assert_eq!(
            [
                accepted(&"a".repeat(128)),
                accepted("_lookup-2"),
                accepted(&"a".repeat(129)),
                accepted("1abc"),
                accepted("a.b"),
            ],
            [true, true, false, false, false]
        );
    }
}
