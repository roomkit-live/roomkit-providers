//! The arguments a call holds so far, read as their fragments come, so
//! that asking whether they are a whole JSON document costs nothing that
//! grows with them.
//!
//! The question is Python's `not arguments_cut(held)`, as the fold of a
//! stream's calls asks it: the text, without the white space around it, is
//! empty or reads as JSON. Asking the parser each time would read the
//! whole text at every fragment. Here each fragment is read once as it
//! comes, and the parser is given a text only when the document it holds
//! has ended, once for that document: a document still open is known not
//! to read without being parsed, and one that ended and is followed by
//! more is known not to read either. A number written at the top, which
//! never says it ended, is judged from its shape and its size; only one
//! whose size is at the very edge of what a float holds is given to the
//! parser, as a number of the same size written short.

use serde_json::Value;

/// Where the reading stands inside an object or an array.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Text {
    /// Between tokens.
    Out,
    /// Inside a string.
    In,
    /// Inside a string, after a backslash.
    Escaped,
}

/// The power of ten a float cannot reach: a number below `10^308` is one
/// the parser reads, a number from `10^309` is past what it reads, and
/// between the two its digits decide.
const EDGE: i128 = 309;

/// Which part of a number the last character wrote.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Part {
    /// A minus sign: a digit must follow.
    #[default]
    Minus,
    /// A zero before the point: no digit may follow.
    Zero,
    /// Digits before the point.
    Int,
    /// The point: a digit must follow.
    Point,
    /// Digits after the point.
    Fraction,
    /// The `e`: a sign or a digit must follow.
    Exponent,
    /// The exponent's sign: a digit must follow.
    Sign,
    /// The exponent's digits.
    ExponentDigits,
}

/// A JSON number written at the top of the arguments, as far as it came.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Number {
    part: Part,
    /// Whether the digits before the point are a lone zero.
    zero_int: bool,
    /// How many digits stand before the point.
    int_digits: u64,
    /// How many zeros follow the point before any other digit, while the
    /// number is still zero.
    leading_zeros: u64,
    /// Whether a digit other than zero came before the exponent.
    nonzero: bool,
    exponent_negative: bool,
    /// The exponent's digits as a number, kept at the largest once past it.
    exponent: u64,
}

impl Number {
    /// The number `first` starts, if it starts one.
    fn starting(first: char) -> Option<Self> {
        let mut number = Self::default();
        (first == '-' || number.push(first)).then_some(number)
    }

    /// Adds `character`; `false` when no JSON number goes on with it.
    fn push(&mut self, character: char) -> bool {
        let digit = character.to_digit(10);
        let exponent = matches!(character, 'e' | 'E');
        self.part = match (self.part, digit) {
            (Part::Minus, Some(0)) => Part::Zero,
            (Part::Minus | Part::Int, Some(_)) => Part::Int,
            (Part::Zero | Part::Int, None) if character == '.' => Part::Point,
            (Part::Point | Part::Fraction, Some(_)) => Part::Fraction,
            (Part::Zero | Part::Int | Part::Fraction, None) if exponent => Part::Exponent,
            (Part::Exponent, None) if matches!(character, '+' | '-') => Part::Sign,
            (Part::Exponent | Part::Sign | Part::ExponentDigits, Some(_)) => Part::ExponentDigits,
            _ => return false,
        };
        self.count(character, digit.map_or(0, u64::from));
        true
    }

    /// Counts the character just added, for the number's size.
    fn count(&mut self, character: char, digit: u64) {
        match self.part {
            Part::Zero => self.zero_int = true,
            Part::Int => {
                self.int_digits = self.int_digits.saturating_add(1);
                self.nonzero = true;
            }
            Part::Fraction if digit != 0 => self.nonzero = true,
            Part::Fraction if !self.nonzero => {
                self.leading_zeros = self.leading_zeros.saturating_add(1);
            }
            Part::Sign => self.exponent_negative = character == '-',
            Part::ExponentDigits => {
                self.exponent = self.exponent.saturating_mul(10).saturating_add(digit);
            }
            Part::Minus | Part::Point | Part::Fraction | Part::Exponent => {}
        }
    }

    /// The power of ten the number is below, and no lower: 1 for a number
    /// from 1 to 10, 0 from 0.1 to 1. For a number that is not zero.
    fn magnitude(&self) -> i128 {
        let before_the_exponent = if self.zero_int {
            -i128::from(self.leading_zeros)
        } else {
            i128::from(self.int_digits)
        };
        let exponent = i128::from(self.exponent);
        let exponent = if self.exponent_negative {
            -exponent
        } else {
            exponent
        };
        before_the_exponent.saturating_add(exponent)
    }

    /// Whether `written`, the number as far as it came, reads as JSON: it
    /// ends on a digit, and is no larger than a float holds, which the
    /// parser refuses. A number at the edge is asked of the parser, as the
    /// number its first 309 digits make: no later digit changes whether a
    /// float holds it.
    fn is_whole(&self, written: &str, parsed: &mut usize) -> bool {
        let ends_on_a_digit = matches!(
            self.part,
            Part::Zero | Part::Int | Part::Fraction | Part::ExponentDigits
        );
        if !ends_on_a_digit {
            return false;
        }
        if !self.nonzero {
            return true;
        }
        let magnitude = self.magnitude();
        if magnitude != EDGE {
            return magnitude < EDGE;
        }
        let mantissa = written.bytes().take_while(|b| !matches!(b, b'e' | b'E'));
        let digits = mantissa
            .filter(u8::is_ascii_digit)
            .skip_while(|b| *b == b'0');
        let digits: String = digits.take(309).map(char::from).collect();
        let short = format!("0.{digits}e{EDGE}");
        *parsed = parsed.saturating_add(short.len());
        serde_json::from_str::<Value>(&short).is_ok()
    }
}

/// What the text read so far is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    /// Nothing but white space.
    #[default]
    Blank,
    /// Inside an object or an array, this deep.
    Open { depth: usize, text: Text },
    /// Inside a string at the top.
    Quoted { escaped: bool },
    /// A number at the top, which more may follow.
    Number(Number),
    /// A literal at the top, as far as it is written: `at` of `word`.
    Literal { word: &'static str, at: usize },
    /// A document that ended at `end`, with nothing but white space after
    /// it: whether it reads, once the parser was asked.
    Ended { end: usize, reads: Option<bool> },
    /// No document, whatever comes next.
    Never,
}

/// The reading of the arguments a call holds.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Held {
    state: State,
    /// Where the document starts, past the white space before it.
    start: usize,
    /// How many bytes were read.
    read: usize,
}

impl Held {
    /// Reads `fragment`, which follows what was read.
    pub(super) fn push(&mut self, fragment: &str) {
        if self.state != State::Never {
            for (at, character) in fragment.char_indices() {
                self.state = self.after(character, self.read.saturating_add(at));
            }
        }
        self.read = self.read.saturating_add(fragment.len());
    }

    /// The state once `character`, at `offset` of the text, is read.
    fn after(&mut self, character: char, offset: usize) -> State {
        let white = character.is_whitespace();
        let past = offset.saturating_add(character.len_utf8());
        match self.state {
            State::Blank if white => State::Blank,
            State::Blank => {
                self.start = offset;
                opening(character)
            }
            State::Open { depth, text } => inside(depth, text, character, past),
            State::Quoted { escaped: true } => State::Quoted { escaped: false },
            State::Quoted { escaped: false } => match character {
                '\\' => State::Quoted { escaped: true },
                '"' => ended(past),
                _ => self.state,
            },
            State::Number(_) if white => ended(offset),
            State::Number(mut number) => {
                if number.push(character) {
                    State::Number(number)
                } else {
                    State::Never
                }
            }
            State::Literal { word, at } if white => {
                let reads = Some(at == word.len());
                State::Ended { end: offset, reads }
            }
            State::Literal { word, at } => {
                let rest = word.get(at..).unwrap_or_default();
                if rest.starts_with(character) {
                    let at = at.saturating_add(character.len_utf8());
                    State::Literal { word, at }
                } else {
                    State::Never
                }
            }
            State::Ended { .. } if white => self.state,
            State::Ended { .. } | State::Never => State::Never,
        }
    }

    /// Whether nothing but white space was read.
    pub(super) const fn is_blank(&self) -> bool {
        matches!(self.state, State::Blank)
    }

    /// Whether `text`, all that was read, is whole: empty but for white
    /// space, or a JSON document with white space around it. `parsed`
    /// counts the bytes given to the parser.
    pub(super) fn is_whole(&mut self, text: &str, parsed: &mut usize) -> bool {
        let start = self.start;
        match &mut self.state {
            State::Blank => true,
            State::Open { .. } | State::Quoted { .. } | State::Never => false,
            State::Literal { word, at } => *at == word.len(),
            State::Number(number) => {
                let written = text.get(start..).unwrap_or_default();
                number.is_whole(written, parsed)
            }
            State::Ended { end, reads } => *reads.get_or_insert_with(|| {
                let document = text.get(start..*end).unwrap_or_default();
                *parsed = parsed.saturating_add(document.len());
                serde_json::from_str::<Value>(document).is_ok()
            }),
        }
    }
}

/// A document that ended at `end`, not asked of the parser yet.
const fn ended(end: usize) -> State {
    State::Ended { end, reads: None }
}

/// What the first character of a document opens.
fn opening(first: char) -> State {
    match first {
        '{' | '[' => State::Open {
            depth: 1,
            text: Text::Out,
        },
        '"' => State::Quoted { escaped: false },
        't' => State::Literal {
            word: "true",
            at: 1,
        },
        'f' => State::Literal {
            word: "false",
            at: 1,
        },
        'n' => State::Literal {
            word: "null",
            at: 1,
        },
        _ => Number::starting(first).map_or(State::Never, State::Number),
    }
}

/// The state after `character` inside an object or an array `depth` deep,
/// which ends at `past` when the character closes it.
fn inside(depth: usize, text: Text, character: char, past: usize) -> State {
    let (depth, text) = match (text, character) {
        (Text::Escaped, _) | (Text::Out, '"') => (depth, Text::In),
        (Text::In, '\\') => (depth, Text::Escaped),
        (Text::In, '"') => (depth, Text::Out),
        (Text::Out, '{' | '[') => (depth.saturating_add(1), Text::Out),
        (Text::Out, '}' | ']') if depth <= 1 => return ended(past),
        (Text::Out, '}' | ']') => (depth.saturating_sub(1), Text::Out),
        (Text::In | Text::Out, _) => (depth, text),
    };
    State::Open { depth, text }
}

#[cfg(test)]
mod tests;
