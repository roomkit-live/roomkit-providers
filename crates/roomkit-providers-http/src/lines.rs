//! The lines of a streamed answer, read from the bytes as they arrive: a
//! stream of JSON lines, as Ollama writes one, split as Python's HTTP client
//! splits it (httpx's `aiter_lines`, which follows `str.splitlines`).
//!
//! A line ends at `\r\n`, at `\n` or at `\r`, wherever the reads cut: a
//! `\r` that ends one read and a `\n` that starts the next are one ending.
//! It ends too at the other characters `str.splitlines` breaks at: the
//! vertical tab, the form feed, the file, group and record separators, the
//! next-line character and the line and paragraph separators. A blank line
//! is a line, and so is the last one, which no ending follows; a stream
//! that ends on an ending gives nothing after it. A line is read as UTF-8,
//! a byte that is none replaced, as Python's client decodes the text.
//!
//! What was read before the reading fails is handed on with the failure,
//! wherever the reads cut. Two bounds keep a broken or hostile server from
//! growing the memory: a line ([`LINE_LIMIT`]) and the whole stream, each
//! failing the turn once passed.

use crate::SseError;
pub use crate::sse::LINE_LIMIT;

/// The characters past ASCII a line ends at, as UTF-8: the next-line
/// character, the line separator and the paragraph separator.
const WIDE_ENDINGS: [&[u8]; 3] = [b"\xC2\x85", b"\xE2\x80\xA8", b"\xE2\x80\xA9"];

/// Whether `byte` alone ends a line: the line feed, the carriage return,
/// the vertical tab, the form feed and the three separators.
const fn ends_a_line(byte: u8) -> bool {
    matches!(byte, b'\n' | b'\r' | 0x0B | 0x0C | 0x1C | 0x1D | 0x1E)
}

/// The lines of one streamed answer, as its bytes arrive.
#[derive(Debug)]
pub struct Lines {
    /// The bytes of the line being read.
    line: Vec<u8>,
    /// Whether the last byte read was a `\r` that ended a line: a `\n`
    /// right after it belongs to the same ending.
    after_carriage_return: bool,
    /// How many bytes the answer sent so far.
    read: usize,
    /// The most bytes the answer may send.
    limit: usize,
    /// Why the reading stopped, once it did.
    failed: Option<SseError>,
}

impl Lines {
    /// The lines of an answer of at most `limit` bytes.
    #[must_use]
    pub const fn new(limit: usize) -> Self {
        Self {
            line: Vec::new(),
            after_carriage_return: false,
            read: 0,
            limit,
            failed: None,
        }
    }

    /// Reads `chunk`, the answer's next bytes: each line it ends, in order,
    /// then why the reading stops, when it does. What was read before a
    /// failure is handed on with it. Once it failed, the reader reads
    /// nothing more and says the same failure again.
    ///
    /// The failures: [`SseError::AnswerTooLong`] past the stream's bound,
    /// [`SseError::StreamLineTooLong`] for a line past [`LINE_LIMIT`].
    pub fn feed(&mut self, chunk: &[u8]) -> (Vec<String>, Option<SseError>) {
        let mut ended = Vec::new();
        if self.failed.is_none() {
            self.failed = self.read_chunk(chunk, &mut ended).err();
        }
        (ended, self.failed.clone())
    }

    fn read_chunk(&mut self, chunk: &[u8], ended: &mut Vec<String>) -> Result<(), SseError> {
        let left = self.limit.saturating_sub(self.read);
        let within = chunk.get(..left).unwrap_or(chunk);
        self.read = self.read.saturating_add(chunk.len());
        self.read_bytes(within, ended)?;
        if within.len() < chunk.len() {
            return Err(SseError::AnswerTooLong { limit: self.limit });
        }
        Ok(())
    }

    fn read_bytes(&mut self, bytes: &[u8], ended: &mut Vec<String>) -> Result<(), SseError> {
        for &byte in bytes {
            // The `\n` of a `\r\n` ending, which may start the next read.
            if std::mem::take(&mut self.after_carriage_return) && byte == b'\n' {
                continue;
            }
            if ends_a_line(byte) {
                self.after_carriage_return = byte == b'\r';
                ended.push(self.take_line());
                continue;
            }
            self.grow_line(byte)?;
            if let Some(ending) = self.wide_ending() {
                self.line.truncate(self.line.len().saturating_sub(ending));
                ended.push(self.take_line());
            }
        }
        Ok(())
    }

    /// The length of the wide ending the line being read ends with, if any.
    fn wide_ending(&self) -> Option<usize> {
        WIDE_ENDINGS
            .iter()
            .find(|ending| self.line.ends_with(ending))
            .map(|ending| ending.len())
    }

    fn grow_line(&mut self, byte: u8) -> Result<(), SseError> {
        if self.line.len() >= LINE_LIMIT {
            return Err(SseError::StreamLineTooLong { limit: LINE_LIMIT });
        }
        self.line.push(byte);
        Ok(())
    }

    fn take_line(&mut self) -> String {
        let line = std::mem::take(&mut self.line);
        String::from_utf8_lossy(&line).into_owned()
    }

    /// The last line, which no ending followed, if the answer sent one.
    ///
    /// # Errors
    ///
    /// The failure the reading stopped on, if it did.
    pub fn finish(&mut self) -> Result<Option<String>, SseError> {
        if let Some(failed) = &self.failed {
            return Err(failed.clone());
        }
        if self.line.is_empty() {
            return Ok(None);
        }
        Ok(Some(self.take_line()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lines `chunks` give, the last one included.
    fn read(chunks: &[&[u8]]) -> Vec<String> {
        let mut lines = Lines::new(1 << 20);
        let mut read: Vec<String> = Vec::new();
        for chunk in chunks {
            let (ended, failed) = lines.feed(chunk);
            assert_eq!(failed, None);
            read.extend(ended);
        }
        read.extend(lines.finish().unwrap());
        read
    }

    // Python: "a\nb\r\nc\rd".splitlines() and httpx's LineDecoder
    #[test]
    fn a_line_ends_as_python_s_splitlines_ends_it() {
        assert_eq!(read(&[b"a\nb\r\nc\rd"]), ["a", "b", "c", "d"]);
        assert_eq!(read(&[b"a\n\nb\n"]), ["a", "", "b"]);
        assert_eq!(
            read(&[b"a\x0bb\x0cc\x1cd\x1de\x1ef"]),
            ["a", "b", "c", "d", "e", "f"]
        );
        let wide = "a\u{85}b\u{2028}c\u{2029}d";
        assert_eq!(read(&[wide.as_bytes()]), ["a", "b", "c", "d"]);
        assert_eq!(read(&[b"\n\n"]), ["", ""]);
        assert_eq!(read(&[b""]), Vec::<String>::new());
    }

    #[test]
    fn an_ending_cut_by_the_reads_ends_one_line() {
        assert_eq!(read(&[b"a\r", b"\nb"]), ["a", "b"]);
        assert_eq!(read(&[b"a\r", b"b"]), ["a", "b"]);
        assert_eq!(read(&[b"a\xE2\x80", b"\xA8b"]), ["a", "b"]);
        let one_byte_at_a_time: Vec<&[u8]> = b"ab\r\ncd\n".chunks(1).collect();
        assert_eq!(read(&one_byte_at_a_time), ["ab", "cd"]);
    }

    #[test]
    fn a_byte_that_is_no_utf8_is_replaced() {
        assert_eq!(read(&[b"a\xFFb\n"]), ["a\u{FFFD}b"]);
    }

    #[test]
    fn the_bounds_fail_the_reading_after_what_came() {
        let mut lines = Lines::new(8);
        let (ended, failed) = lines.feed(b"abc\ndefghij\n");
        assert_eq!(ended, ["abc"]);
        assert_eq!(failed, Some(SseError::AnswerTooLong { limit: 8 }));
        assert!(lines.finish().is_err());
        let mut lines = Lines::new(usize::MAX);
        let long = vec![b'x'; LINE_LIMIT + 1];
        let (_, failed) = lines.feed(&long);
        let limit = LINE_LIMIT;
        assert_eq!(failed, Some(SseError::StreamLineTooLong { limit }));
    }
}
