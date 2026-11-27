//! The arguments a call holds so far, read as their fragments come, so
//! that asking whether they are a whole JSON document costs nothing that
//! grows with them.
//!
//! The question is Python's `not arguments_cut(held)`, as the fold of a
//! stream's calls asks it: the text is nothing but what `str.strip()`
//! strips, or it reads as JSON, which takes a space, a tab and a line break
//! around a document and no other white space. Asking the parser each time
//! would read the whole text at every fragment. Here each fragment is read
//! once as it comes, and the parser is given a text only when the document
//! it holds has ended, once for that document: a document still open is
//! known not to read without being parsed, and one that ended and is
//! followed by more is known not to read either. A number written at the
//! top, which never says it ended, is judged from its shape and its size;
//! only one whose size is at the very edge of what a float holds is given
//! to the parser, as a number of the same size written short, from its
//! first digits, which are kept as they come.
//!
//! So a question costs what came since the last one: asked again of the
//! same text, it is answered from what is known.

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

/// How many of a number's digits, from its first that is not zero, decide
/// whether a float holds it: no later digit changes that.
const DECIDING: usize = 309;

/// Whether Python's `str.strip()` strips `character`: white space, and the
/// four separators of information, U+001C to U+001F.
pub(super) fn stripped(character: char) -> bool {
    character.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&character)
}

/// Whether `character` is white space to JSON: what may stand around a
/// document.
const fn json_space(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\r')
}

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

    /// Whether the number, as far as it came, reads as JSON: it ends on a
    /// digit, and is no larger than a float holds, which the parser
    /// refuses. `None` for a number at the edge, whose digits decide.
    fn reads(&self) -> Option<bool> {
        let ends_on_a_digit = matches!(
            self.part,
            Part::Zero | Part::Int | Part::Fraction | Part::ExponentDigits
        );
        if !ends_on_a_digit {
            return Some(false);
        }
        if !self.nonzero {
            return Some(true);
        }
        let magnitude = self.magnitude();
        (magnitude != EDGE).then_some(magnitude < EDGE)
    }

    /// Whether the last character wrote a digit before the exponent.
    const fn wrote_a_digit(&self) -> bool {
        matches!(self.part, Part::Zero | Part::Int | Part::Fraction)
    }
}

/// What the text read so far is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    /// Nothing but what Python strips.
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
    /// A document that ended at `end`, with nothing but JSON's white space
    /// after it: whether it reads, once the parser was asked.
    Ended { end: usize, reads: Option<bool> },
    /// No document, whatever comes next.
    Never,
}

/// The reading of the arguments a call holds.
#[derive(Clone, Debug, Default)]
pub(super) struct Held {
    state: State,
    /// Whether white space that is not JSON's was read before any
    /// document: no document reads after it.
    odd_space: bool,
    /// Where the document starts, past the white space before it.
    start: usize,
    /// How many bytes were read.
    read: usize,
    /// The first digits of a number at the top, from its first that is
    /// not zero: as many as decide whether a float holds it.
    digits: String,
    /// What the parser said of a number at the edge that has this many of
    /// `digits`.
    edge: Option<(usize, bool)>,
}

impl Held {
    /// Reads `fragment`, which follows what was read. `examined` counts
    /// the bytes looked at.
    pub(super) fn push(&mut self, fragment: &str, examined: &mut usize) {
        if self.state != State::Never {
            *examined = examined.saturating_add(fragment.len());
            for (at, character) in fragment.char_indices() {
                self.state = self.after(character, self.read.saturating_add(at));
            }
        }
        self.read = self.read.saturating_add(fragment.len());
    }

    /// The state once `character`, at `offset` of the text, is read.
    fn after(&mut self, character: char, offset: usize) -> State {
        let white = json_space(character);
        let past = offset.saturating_add(character.len_utf8());
        match self.state {
            State::Blank if stripped(character) => {
                self.odd_space |= !white;
                State::Blank
            }
            State::Blank if self.odd_space => State::Never,
            State::Blank => {
                self.start = offset;
                let state = opening(character);
                self.keep(state, character);
                state
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
                    let state = State::Number(number);
                    self.keep(state, character);
                    state
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

    /// Keeps `digit`, which the number `state` holds was just given, when
    /// it is one of the digits that decide its size: the zeros before its
    /// first other digit do not, nor a digit past [`DECIDING`].
    fn keep(&mut self, state: State, digit: char) {
        let State::Number(number) = state else {
            return;
        };
        let leading_zero = self.digits.is_empty() && digit == '0';
        if number.wrote_a_digit() && !leading_zero && self.digits.len() < DECIDING {
            self.digits.push(digit);
        }
    }

    /// Whether nothing but what Python strips was read.
    pub(super) const fn is_blank(&self) -> bool {
        matches!(self.state, State::Blank)
    }

    /// Whether `text`, all that was read, is whole: nothing but what Python
    /// strips, or a JSON document with JSON's white space around it.
    /// `examined` counts the bytes looked at to say so, which are those
    /// given to the parser: none when the answer is known.
    pub(super) fn is_whole(&mut self, text: &str, examined: &mut usize) -> bool {
        match self.state {
            State::Blank => true,
            State::Open { .. } | State::Quoted { .. } | State::Never => false,
            State::Literal { word, at } => at == word.len(),
            State::Number(number) => match number.reads() {
                Some(reads) => reads,
                None => self.at_the_edge(examined),
            },
            State::Ended {
                reads: Some(reads), ..
            } => reads,
            State::Ended { end, reads: None } => {
                let document = text.get(self.start..end).unwrap_or_default();
                *examined = examined.saturating_add(document.len());
                let reads = serde_json::from_str::<Value>(document).is_ok();
                self.state = State::Ended {
                    end,
                    reads: Some(reads),
                };
                reads
            }
        }
    }

    /// Whether a number at the edge reads: what the parser says of the
    /// number its first digits make, written short, asked once for those
    /// digits.
    fn at_the_edge(&mut self, examined: &mut usize) -> bool {
        let digits = self.digits.len();
        if let Some((_, reads)) = self.edge.filter(|(asked, _)| *asked == digits) {
            return reads;
        }
        let short = format!("0.{}e{EDGE}", self.digits);
        *examined = examined.saturating_add(short.len());
        let reads = serde_json::from_str::<Value>(&short).is_ok();
        self.edge = Some((digits, reads));
        reads
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
