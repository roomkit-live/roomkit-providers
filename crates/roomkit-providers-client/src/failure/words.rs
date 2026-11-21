//! Whether the words of a failure that came with no status name a
//! transient one: Python's `_names_transient`.

use super::retryable_status;

/// Whether `message`, which came with no status, names a transient
/// failure, as Python's `_names_transient` reads it: `rate[ _-]?limit(ed|s)?`,
/// `too many requests` or `overloaded`, in any case, or a retried status
/// (`[45]\d\d`), each a whole word, a word's letters including `_`.
#[must_use]
pub fn names_transient(message: &str) -> bool {
    let chars: Vec<char> = message.chars().collect();
    let in_word = |at: usize| {
        let found = chars.get(at);
        found.is_some_and(|c| c.is_alphanumeric() || *c == '_')
    };
    let whole = |start: usize, end: usize| {
        let before = start.checked_sub(1).is_some_and(in_word);
        !before && !in_word(end)
    };
    (0..chars.len()).any(|start| {
        let words = transient_words_at(&chars, start).into_iter();
        let status = status_at(&chars, start);
        words.clone().any(|end| whole(start, end))
            || status.is_some_and(|(end, status)| whole(start, end) && retryable_status(status))
    })
}

/// Where each of the transient words that start at `start` in `chars`
/// ends: each way `rate[ _-]?limit(ed|s)?` reads there, `too many
/// requests`, `overloaded`.
fn transient_words_at(chars: &[char], start: usize) -> Vec<usize> {
    let whole_words = ["too many requests", "overloaded"].into_iter();
    let mut ends: Vec<usize> = whole_words
        .filter_map(|words| spelled_at(chars, start, words))
        .collect();
    let Some(rate) = spelled_at(chars, start, "rate") else {
        return ends;
    };
    for separator in ["", " ", "_", "-"] {
        let after = spelled_at(chars, rate, separator);
        let Some(limit) = after.and_then(|after| spelled_at(chars, after, "limit")) else {
            continue;
        };
        ends.push(limit);
        let suffixes = ["ed", "s"].into_iter();
        ends.extend(suffixes.filter_map(|suffix| spelled_at(chars, limit, suffix)));
    }
    ends
}

/// Where `text` ends when `chars` spell it at `at`, in any case.
fn spelled_at(chars: &[char], at: usize, text: &str) -> Option<usize> {
    let mut end = at;
    for expected in text.chars() {
        let found = chars.get(end)?;
        if !found.eq_ignore_ascii_case(&expected) {
            return None;
        }
        end += 1;
    }
    Some(end)
}

/// The status a number of three digits starting `4` or `5` at `start`
/// says, and where it ends.
fn status_at(chars: &[char], start: usize) -> Option<(usize, u16)> {
    let end = start.checked_add(3)?;
    let digits = chars.get(start..end)?;
    if !matches!(digits.first(), Some('4' | '5')) {
        return None;
    }
    let number: String = digits.iter().collect();
    number.parse().ok().map(|status| (end, status))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Python: providers/ai/base.py _names_transient: its words read as
    // Python's regular expressions read them, `_` a letter of a word and
    // one separator at most.
    #[test]
    fn a_message_names_a_transient_failure_as_python_reads_it() {
        let messages = [
            "rate_limit_exceeded",
            "overloaded_error",
            "error_503",
            "rate  limit",
            "Rate limit reached",
            "HTTP 503",
            "ratelimited: retry",
            "Too many requests",
            "rate-limits",
            "HTTP 404",
        ];
        assert_eq!(
            messages.map(names_transient),
            [
                false, false, false, false, true, true, true, true, true, false
            ]
        );
    }

    // Each of the words names a transient failure, and so does each status
    // that is retried, a 4xx as a 5xx: a status that is not is none.
    #[test]
    fn each_transient_word_and_each_retried_status_is_read() {
        for transient in [
            "the model is overloaded",
            "too many requests, try later",
            "rate limited",
            "HTTP 429",
            "status 408.",
            "a 529 came back",
        ] {
            assert!(names_transient(transient), "{transient}");
        }
        for other in ["HTTP 401", "error 4290", "v429", "overloaded_model"] {
            assert!(!names_transient(other), "{other}");
        }
    }
}
