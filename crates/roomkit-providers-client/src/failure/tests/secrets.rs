//! What an exit searches a text for: a secret of 8 characters at least, in
//! each form the code that builds a failure writes one in, through what
//! shows nothing; and what it keeps of an error built elsewhere.

use std::error::Error as _;

use roomkit_providers_llm::{
    ProviderError, ProviderErrorOptions, ResponseSchemaError, ResponseSchemaErrorOptions,
    ResponseSchemaFailure,
};

use super::{KEY, test_exit};
use crate::failure::{Exit, Failure, MESSAGE_LIMIT, SECRET_MIN, Secrets, one_line, told};

/// What `exit` lets out of what a server `said`.
fn cleaned(exit: &Exit, said: &str) -> String {
    let said = vec![told(said, MESSAGE_LIMIT)];
    exit.leave(&Failure::said(said, false, None)).message
}

// A text of the configuration is a secret from 8 characters: a shorter one
// would be found in ordinary words, and is left.
#[test]
fn a_secret_is_searched_for_from_8_characters() {
    assert_eq!(SECRET_MIN, 8);
    let secrets = ["eight-ch".to_owned(), "seven-c".to_owned()];
    let exit = Exit::new("test", Secrets::of(secrets));
    assert_eq!(cleaned(&exit, "eight-ch and seven-c"), "[***] and seven-c");
}

// A secret is found in each form the code that builds a failure may write
// it in: as JSON writes it, as a reader quotes it, and as Python's `repr`
// writes it, alone and beside a double quote.
#[test]
fn a_secret_is_found_in_each_form_a_failure_may_write_it_in() {
    let exit = Exit::new("test", Secrets::of(["place\u{7}holder's key".to_owned()]));
    for form in [
        r"place\u0007holder's key",
        r"place\u{7}holder's key",
        r"place\x07holder's key",
        r"place\x07holder\'s key",
    ] {
        let said = cleaned(&exit, &format!("refused: {form} !"));
        assert_eq!(said, "refused: [***] !", "{form}");
    }
}

// A text on one line holds no character that shows nothing, of any of the
// ranges that hold one, no control character, and no white space but one
// space between two words.
#[test]
fn a_text_on_one_line_drops_what_shows_nothing() {
    let hidden = [
        '\u{ad}',
        '\u{61c}',
        '\u{180e}',
        '\u{feff}',
        '\u{200b}',
        '\u{200f}',
        '\u{202a}',
        '\u{202e}',
        '\u{2060}',
        '\u{206f}',
        '\u{fff9}',
        '\u{fffb}',
        '\u{e0000}',
        '\u{e007f}',
    ];
    for character in hidden {
        let shown = one_line(&format!("a{character}b"));
        assert_eq!(shown, "ab", "{:x}", u32::from(character));
    }
    assert_eq!(one_line(" a\tb\u{1b}[0m\r\n c\u{200a}d "), "a b [0m c d");
}

// An error built elsewhere leaves as it is, its source too, when nothing in
// it needs cleaning. A source that holds a secret or a control character
// does not leave: the error is rebuilt around its text, with what it said
// of itself, and a response schema's failure with its reason.
#[test]
fn an_error_built_elsewhere_is_rebuilt_only_where_it_needs_cleaning() {
    let built = |cause: String| {
        let options = ProviderErrorOptions {
            retryable: true,
            provider: "elsewhere".to_owned(),
            status_code: Some(429),
            ..ProviderErrorOptions::default()
        };
        ProviderError::new("slow down", options).with_source(std::io::Error::other(cause))
    };
    let exit = test_exit();
    let kept = exit.leave(&Failure::from(built("the cause".to_owned())));
    assert_eq!(
        kept.source().map(ToString::to_string).as_deref(),
        Some("the cause")
    );
    for cause in [format!("the key {KEY}"), "a\u{1b}[31m cause".to_owned()] {
        let rebuilt = exit.leave(&Failure::from(built(cause)));
        assert!(rebuilt.source().is_none(), "{rebuilt:?}");
        assert_eq!(
            (
                rebuilt.message.as_str(),
                rebuilt.status_code,
                rebuilt.retryable,
                rebuilt.provider.as_str()
            ),
            ("slow down", Some(429), true, "elsewhere")
        );
    }
    let options = ResponseSchemaErrorOptions {
        provider: "elsewhere".to_owned(),
    };
    let cut = ResponseSchemaFailure::Truncated;
    let schema = ResponseSchemaError::new(format!("cut after {KEY}"), cut, options);
    let cleaned = exit.leave(&Failure::from(ProviderError::from(schema)));
    let source = cleaned.source();
    let schema = source.and_then(|cause| cause.downcast_ref::<ResponseSchemaError>());
    assert_eq!(
        (cleaned.message.as_str(), schema.map(|schema| schema.reason)),
        ("cut after [***]", Some(cut))
    );
}
