//! The events of a streamed answer, read from the bytes as they arrive
//! (server-sent events), without a dependency for it.
//!
//! A line ends at `\r\n`, at `\n` or at `\r`, as the format defines it,
//! wherever the reads cut: a `\r` that ends one read and a `\n` that starts
//! the next are one ending. A blank line ends an event, whose `data` lines
//! join with `\n`; a comment (`:`) and the other fields are skipped. An
//! event the answer's end cuts, with no blank line after it, is read.
//! Three bounds keep a broken or hostile server from growing the memory: a
//! line, an event's data and the whole stream, each failing the turn once
//! passed, so that a line that never ends, or events that never stop, are
//! cut.
//!
//! The reader is narrower than the format on two points:
//!
//! - a byte order mark at the start of the answer is not dropped: the line
//!   it starts is read as a field other than `data`, and skipped;
//! - a `data` line without a colon, which the format reads as an empty line
//!   of data, is skipped as another field is.

use crate::SseError;

/// The longest line a stream may send, in bytes.
pub const LINE_LIMIT: usize = 1024 * 1024;

/// The most data one event may carry, in bytes.
pub const EVENT_LIMIT: usize = 1024 * 1024;

/// The most bytes a stream may send, envelopes included. A stream weighs far
/// more than what it says: a server may wrap each token of a model's
/// reasoning in about 200 bytes of JSON, so that 4 MiB would cut a turn
/// after some 20,000 tokens. This keeps the bound on events that never stop,
/// past any answer a model writes.
pub const STREAM_LIMIT: usize = 256 * 1024 * 1024;

/// The events of one streamed answer, as its bytes arrive.
#[derive(Debug)]
pub struct Events {
    /// The bytes of the line being read.
    line: Vec<u8>,
    /// The data of the event being read, its lines joined.
    data: Option<String>,
    /// Whether the last byte read was a `\r` that ended a line: a `\n`
    /// right after it belongs to the same ending.
    after_carriage_return: bool,
    /// How many bytes the answer sent so far.
    read: usize,
    /// The most bytes the answer may send.
    limit: usize,
}

impl Events {
    /// The events of an answer of at most `limit` bytes.
    pub fn new(limit: usize) -> Self {
        Self {
            line: Vec::new(),
            data: None,
            after_carriage_return: false,
            read: 0,
            limit,
        }
    }

    /// Reads `chunk`, the answer's next bytes: the data of each event it
    /// ends, in order.
    ///
    /// # Errors
    ///
    /// [`SseError::AnswerTooLong`] past the stream's bound,
    /// [`SseError::StreamLineTooLong`] for a line past [`LINE_LIMIT`],
    /// [`SseError::StreamEventTooLong`] for an event past
    /// [`EVENT_LIMIT`], [`SseError::Malformed`] for a line that is no
    /// UTF-8.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<String>, SseError> {
        self.read = self.read.saturating_add(chunk.len());
        if self.read > self.limit {
            return Err(SseError::AnswerTooLong { limit: self.limit });
        }
        let mut ended = Vec::new();
        let mut rest = chunk;
        if self.after_carriage_return && !rest.is_empty() {
            self.after_carriage_return = false;
            rest = rest.strip_prefix(b"\n").unwrap_or(rest);
        }
        while let Some(at) = rest.iter().position(|byte| matches!(byte, b'\r' | b'\n')) {
            let (head, tail) = rest.split_at(at);
            self.grow_line(head)?;
            let carriage_return = tail.first() == Some(&b'\r');
            rest = tail.get(1..).unwrap_or_default();
            if carriage_return {
                match rest.strip_prefix(b"\n") {
                    Some(after) => rest = after,
                    // The line feed of this ending may start the next read.
                    None => self.after_carriage_return = rest.is_empty(),
                }
            }
            let line = std::mem::take(&mut self.line);
            if let Some(event) = self.take_line(&line)? {
                ended.push(event);
            }
        }
        self.grow_line(rest)?;
        Ok(ended)
    }

    /// The data of the event the answer's end cut, if any: an answer that
    /// ends without its blank line.
    ///
    /// # Errors
    ///
    /// As [`feed`](Self::feed), for the last line.
    pub fn finish(&mut self) -> Result<Option<String>, SseError> {
        let line = std::mem::take(&mut self.line);
        if line.is_empty() {
            return Ok(self.data.take());
        }
        // The last line, which no ending followed, is the last of the event
        // the answer's end cuts.
        let ended = self.take_line(&line)?;
        Ok(ended.or_else(|| self.data.take()))
    }

    fn grow_line(&mut self, bytes: &[u8]) -> Result<(), SseError> {
        if self.line.len().saturating_add(bytes.len()) > LINE_LIMIT {
            return Err(SseError::StreamLineTooLong { limit: LINE_LIMIT });
        }
        self.line.extend_from_slice(bytes);
        Ok(())
    }

    /// Takes one whole line: the event it ends, at a blank line.
    fn take_line(&mut self, line: &[u8]) -> Result<Option<String>, SseError> {
        if line.is_empty() {
            return Ok(self.data.take());
        }
        let Some(value) = line.strip_prefix(b"data:") else {
            // A comment, or a field other than data.
            return Ok(None);
        };
        let value = value.strip_prefix(b" ").unwrap_or(value);
        let value = std::str::from_utf8(value).map_err(|_| SseError::Malformed {
            reason: "an event's data is not UTF-8".to_owned(),
        })?;
        let data = self.data.get_or_insert_with(String::new);
        let joined = data.len() + value.len() + usize::from(!data.is_empty());
        if joined > EVENT_LIMIT {
            return Err(SseError::StreamEventTooLong { limit: EVENT_LIMIT });
        }
        if !data.is_empty() {
            data.push('\n');
        }
        data.push_str(value);
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_split_across_chunks_come_whole_and_in_order() {
        let mut events = Events::new(1024);
        let mut read = events.feed(b"data: {\"a\"").unwrap();
        read.extend(
            events
                .feed(b": 1}\r\n\r\n: keep-alive\n\ndata: [DONE]\n\n")
                .unwrap(),
        );
        assert_eq!(read, ["{\"a\": 1}", "[DONE]"]);
    }

    #[test]
    fn an_event_s_data_lines_join_and_other_fields_are_skipped() {
        let mut events = Events::new(1024);
        let read = events
            .feed(b"event: message\nid: 7\ndata: first\ndata:second\n\n")
            .unwrap();
        assert_eq!(read, ["first\nsecond"]);
    }

    #[test]
    fn an_event_the_answer_s_end_cuts_is_read() {
        let mut events = Events::new(1024);
        assert_eq!(events.feed(b"data: last").unwrap(), Vec::<String>::new());
        assert_eq!(events.finish().unwrap().as_deref(), Some("last"));
    }

    /// The events of `answer` sent in two chunks, cut at `at`, then ended.
    fn read_in_two(answer: &[u8], at: usize) -> Result<Vec<String>, SseError> {
        let (head, tail) = answer.split_at(at);
        let mut events = Events::new(1024);
        let mut read = events.feed(head)?;
        read.extend(events.feed(tail)?);
        read.extend(events.finish()?);
        Ok(read)
    }

    /// An answer may end on a carriage return whose line feed never came:
    /// the event before it is read, wherever the chunks are cut.
    #[test]
    fn an_event_is_read_when_the_answer_ends_on_a_lone_carriage_return() {
        for answer in [
            &b"data: last\r\n\r"[..],
            b"data: last\n\r",
            b"data: last\r",
            b"data: first\ndata: last\n\r",
        ] {
            let last = if answer.starts_with(b"data: first") {
                "first\nlast"
            } else {
                "last"
            };
            for at in 0..=answer.len() {
                assert_eq!(
                    read_in_two(answer, at),
                    Ok(vec![last.to_owned()]),
                    "{:?} cut at {at}",
                    String::from_utf8_lossy(answer)
                );
            }
        }
        let two = b"data: one\n\ndata: two\r\n\r";
        for at in 0..=two.len() {
            assert_eq!(
                read_in_two(two, at),
                Ok(vec!["one".to_owned(), "two".to_owned()]),
                "cut at {at}"
            );
        }
    }

    /// A line is decoded once it is whole: a character cut by a chunk is
    /// read, and bytes that are no UTF-8 are refused wherever the chunks
    /// are cut, at the answer's end too.
    #[test]
    fn data_that_is_no_utf_8_is_refused_whole_or_split_across_chunks() {
        let malformed = SseError::Malformed {
            reason: "an event's data is not UTF-8".to_owned(),
        };
        for answer in [
            &b"data: \xff\n\n"[..],
            b"data: caf\xc3\n\n",
            b"data: \xe2\x82\n\n",
            b"data: \xe2\x82",
        ] {
            for at in 0..=answer.len() {
                assert_eq!(
                    read_in_two(answer, at),
                    Err(malformed.clone()),
                    "{:?} cut at {at}",
                    String::from_utf8_lossy(answer)
                );
            }
        }
        let answer = "data: caf\u{e9} \u{20ac}\n\n".as_bytes();
        for at in 0..=answer.len() {
            assert_eq!(
                read_in_two(answer, at),
                Ok(vec!["caf\u{e9} \u{20ac}".to_owned()]),
                "cut at {at}"
            );
        }
    }

    /// The three endings of the format read alike, wherever one read ends
    /// and the next starts: a `\r` and the `\n` after it are one ending
    /// even in two reads.
    #[test]
    fn a_line_ends_at_a_carriage_return_a_line_feed_or_both_at_every_cut() {
        let events = "data: one\n\ndata: two\ndata: lines\n\n: comment\n\ndata: three\n\n";
        let expected = Ok(vec![
            "one".to_owned(),
            "two\nlines".to_owned(),
            "three".to_owned(),
        ]);
        for ending in ["\n", "\r", "\r\n"] {
            let answer = events.replace('\n', ending);
            for at in 0..=answer.len() {
                assert_eq!(
                    read_in_two(answer.as_bytes(), at),
                    expected,
                    "{ending:?} cut at {at}"
                );
            }
        }
        // Each ending may differ from the one before it.
        let mixed = b"data: one\r\rdata: two\n\r\ndata: three\r\n\n";
        for at in 0..=mixed.len() {
            assert_eq!(
                read_in_two(mixed, at),
                Ok(vec!["one".to_owned(), "two".to_owned(), "three".to_owned()]),
                "cut at {at}"
            );
        }
        // One byte per read.
        let mut reader = Events::new(1024);
        let mut read = Vec::new();
        for byte in b"data: a\r\n\r\ndata: b\r\r" {
            read.extend(reader.feed(&[*byte]).unwrap());
        }
        assert_eq!(read, ["a", "b"]);
        assert_eq!(reader.finish(), Ok(None));
    }

    /// An event the answer's end cuts, with no blank line after it, is
    /// read, whichever ending its last line has or lacks.
    #[test]
    fn an_event_with_no_blank_line_after_it_is_read_at_the_answer_s_end() {
        for answer in [
            &b"data: one\n\ndata: last"[..],
            b"data: one\n\ndata: last\n",
        ] {
            for at in 0..=answer.len() {
                assert_eq!(
                    read_in_two(answer, at),
                    Ok(vec!["one".to_owned(), "last".to_owned()]),
                    "cut at {at}"
                );
            }
        }
    }

    #[test]
    fn a_byte_order_mark_and_a_data_line_without_a_colon_are_skipped() {
        let mut events = Events::new(1024);
        let marked = b"\xef\xbb\xbfdata: first\ndata: second\n\n";
        assert_eq!(events.feed(marked).unwrap(), ["second"]);
        let bare = b"data: one\ndata\ndata: two\n\n";
        assert_eq!(events.feed(bare).unwrap(), ["one\ntwo"]);
    }

    #[test]
    fn the_bounds_are_a_mebibyte_for_a_line_and_an_event_and_256_for_a_stream() {
        assert_eq!(LINE_LIMIT, 1_048_576);
        assert_eq!(EVENT_LIMIT, 1_048_576);
        assert_eq!(STREAM_LIMIT, 268_435_456);
    }

    #[test]
    fn a_stream_is_read_to_its_bound_and_refused_one_byte_past_it() {
        let mut events = Events::new(16);
        assert_eq!(events.feed(b"data: 12345678\n\n").unwrap(), ["12345678"]);
        assert_eq!(
            events.feed(b"\n"),
            Err(SseError::AnswerTooLong { limit: 16 })
        );
        let mut events = Events::new(16);
        assert_eq!(
            events.feed(b"data: 123456789\n\n"),
            Err(SseError::AnswerTooLong { limit: 16 })
        );
    }

    #[test]
    fn a_line_is_read_to_its_bound_and_refused_one_byte_past_it() {
        let too_long = Err(SseError::StreamLineTooLong { limit: LINE_LIMIT });
        let line = |length: usize| format!("data:{}\n\n", "x".repeat(length - 5));
        let mut events = Events::new(usize::MAX);
        let read = events.feed(line(LINE_LIMIT).as_bytes()).unwrap();
        let lengths: Vec<usize> = read.iter().map(String::len).collect();
        assert_eq!(lengths, [LINE_LIMIT - 5]);
        let mut events = Events::new(usize::MAX);
        assert_eq!(events.feed(line(LINE_LIMIT + 1).as_bytes()), too_long);
        // The bound is the line's, however many chunks bring it.
        let mut events = Events::new(usize::MAX);
        assert_eq!(events.feed(&vec![b'x'; LINE_LIMIT]), Ok(Vec::new()));
        assert_eq!(events.feed(b"x"), too_long);
    }

    /// The line break that joins two data lines counts in the event.
    #[test]
    fn an_event_is_read_to_its_bound_and_refused_one_byte_past_it() {
        let event = |second: usize| {
            let first = "x".repeat(EVENT_LIMIT - 10);
            format!("data:{first}\ndata:{}\n\n", "x".repeat(second))
        };
        let mut events = Events::new(usize::MAX);
        let read = events.feed(event(9).as_bytes()).unwrap();
        let lengths: Vec<usize> = read.iter().map(String::len).collect();
        assert_eq!(lengths, [EVENT_LIMIT]);
        let mut events = Events::new(usize::MAX);
        assert_eq!(
            events.feed(event(10).as_bytes()),
            Err(SseError::StreamEventTooLong { limit: EVENT_LIMIT })
        );
    }

    /// A server that sends a line without end is cut at the line's bound,
    /// whatever the answer's.
    #[test]
    fn a_line_that_never_ends_is_cut_at_its_bound() {
        let mut events = Events::new(usize::MAX);
        let chunk = vec![b'x'; 64 * 1024];
        let failed = (0..64).find_map(|_| events.feed(&chunk).err());
        assert_eq!(
            failed,
            Some(SseError::StreamLineTooLong { limit: LINE_LIMIT })
        );
    }

    /// An event whose data lines never stop is cut at the event's bound.
    #[test]
    fn an_event_that_never_ends_is_cut_at_its_bound() {
        let mut events = Events::new(usize::MAX);
        let line = format!("data: {}\n", "x".repeat(1023));
        let failed = (0..2048).find_map(|_| events.feed(line.as_bytes()).err());
        assert_eq!(
            failed,
            Some(SseError::StreamEventTooLong { limit: EVENT_LIMIT })
        );
    }

    /// Events that never stop are cut at the stream's bound.
    #[test]
    fn events_that_never_stop_are_cut_at_the_stream_s_bound() {
        let mut events = Events::new(4096);
        let failed = (0..1024).find_map(|_| events.feed(b"data: {}\n\n").err());
        assert_eq!(failed, Some(SseError::AnswerTooLong { limit: 4096 }));
    }
}
