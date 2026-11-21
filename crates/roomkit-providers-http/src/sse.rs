//! The events of a streamed answer, read from the bytes as they arrive
//! (server-sent events), without a dependency for it.
//!
//! A line ends at `\r\n`, at `\n` or at `\r`, as the format defines it,
//! wherever the reads cut: a `\r` that ends one read and a `\n` that starts
//! the next are one ending. A blank line ends an event, whose `data` lines
//! join with `\n`; a comment (`:`) and the other fields are skipped. An
//! event the answer's end cuts, with no blank line after it, is read.
//! What was read before the reading fails is handed on with the failure,
//! wherever the reads cut.
//! Three bounds keep a broken or hostile server from growing the memory: a
//! line, an event's data and the whole stream, each failing the turn once
//! passed, so that a line that never ends, or events that never stop, are
//! cut.
//!
//! A byte order mark that starts the answer is dropped, as the format
//! says: it is no part of the first line. The reader is narrower than the
//! format on one point: a `data` line without a colon, which the format
//! reads as an empty line of data, is skipped as another field is.

use crate::SseError;

/// The longest line a stream may send, in bytes.
pub const LINE_LIMIT: usize = 1024 * 1024;

/// The byte order mark an answer may start with.
const BYTE_ORDER_MARK: &[u8] = b"\xEF\xBB\xBF";

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
    /// Whether the answer's first line was taken: a byte order mark starts
    /// no other.
    started: bool,
    /// The data of the event being read, its lines joined.
    data: Option<String>,
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

impl Events {
    /// The events of an answer of at most `limit` bytes.
    pub fn new(limit: usize) -> Self {
        Self {
            line: Vec::new(),
            started: false,
            data: None,
            after_carriage_return: false,
            read: 0,
            limit,
            failed: None,
        }
    }

    /// Reads `chunk`, the answer's next bytes: the data of each event it
    /// ends, in order, then why the reading stops, when it does. What was
    /// read before a failure is handed on with it, however the answer was
    /// cut into chunks: the stream's bound is counted byte by byte, and a
    /// line that fails keeps the events ended before it. Once it failed,
    /// the reader reads nothing more and says the same failure again.
    ///
    /// The failures: [`SseError::AnswerTooLong`] past the stream's bound,
    /// [`SseError::StreamLineTooLong`] for a line past [`LINE_LIMIT`],
    /// [`SseError::StreamEventTooLong`] for an event past
    /// [`EVENT_LIMIT`], [`SseError::Malformed`] for a line that is no
    /// UTF-8.
    pub fn feed(&mut self, chunk: &[u8]) -> (Vec<String>, Option<SseError>) {
        let mut ended = Vec::new();
        if self.failed.is_none() {
            self.failed = self.read_chunk(chunk, &mut ended).err();
        }
        (ended, self.failed.clone())
    }

    /// Reads the bytes of `chunk` the stream's bound still allows, adding
    /// to `ended` the events they end, and fails past the bound.
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
        let mut rest = bytes;
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
        self.grow_line(rest)
    }

    /// The data of the event the answer's end cut, if any: an answer that
    /// ends without its blank line.
    ///
    /// # Errors
    ///
    /// As [`feed`](Self::feed), for the last line; and the failure the
    /// reading stopped on, if it did.
    pub fn finish(&mut self) -> Result<Option<String>, SseError> {
        if let Some(failed) = &self.failed {
            return Err(failed.clone());
        }
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

    /// Takes one whole line: the event it ends, at a blank line. The byte
    /// order mark that starts the answer's first line is no part of it.
    fn take_line(&mut self, line: &[u8]) -> Result<Option<String>, SseError> {
        let line = if std::mem::replace(&mut self.started, true) {
            line
        } else {
            line.strip_prefix(BYTE_ORDER_MARK).unwrap_or(line)
        };
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

    impl Events {
        /// The events `chunk` ends, or the failure that stops the reading.
        fn fed(&mut self, chunk: &[u8]) -> Result<Vec<String>, SseError> {
            let (ended, failed) = self.feed(chunk);
            failed.map_or(Ok(ended), Err)
        }
    }

    #[test]
    fn events_split_across_chunks_come_whole_and_in_order() {
        let mut events = Events::new(1024);
        let mut read = events.fed(b"data: {\"a\"").unwrap();
        read.extend(
            events
                .fed(b": 1}\r\n\r\n: keep-alive\n\ndata: [DONE]\n\n")
                .unwrap(),
        );
        assert_eq!(read, ["{\"a\": 1}", "[DONE]"]);
    }

    #[test]
    fn an_event_s_data_lines_join_and_other_fields_are_skipped() {
        let mut events = Events::new(1024);
        let read = events
            .fed(b"event: message\nid: 7\ndata: first\ndata:second\n\n")
            .unwrap();
        assert_eq!(read, ["first\nsecond"]);
    }

    #[test]
    fn an_event_the_answer_s_end_cuts_is_read() {
        let mut events = Events::new(1024);
        assert_eq!(events.fed(b"data: last").unwrap(), Vec::<String>::new());
        assert_eq!(events.finish().unwrap().as_deref(), Some("last"));
    }

    // A byte order mark that starts the answer is no part of its first
    // line, wherever the reads cut it; one anywhere else is what it was, a
    // character of its line.
    #[test]
    fn a_byte_order_mark_that_starts_the_answer_is_dropped() {
        let answer = "\u{feff}data: first\n\ndata: second\n\n".as_bytes();
        for at in 0..=answer.len() {
            let (head, tail) = answer.split_at(at);
            let mut events = Events::new(1024);
            let mut read = events.fed(head).unwrap();
            read.extend(events.fed(tail).unwrap());
            assert_eq!(read, ["first", "second"], "{at}");
        }
        // The mark alone, and the mark before an event the answer's end
        // cuts.
        let mut events = Events::new(1024);
        assert_eq!(
            events.fed("\u{feff}\n".as_bytes()).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(events.finish().unwrap(), None);
        let mut events = Events::new(1024);
        assert_eq!(
            events.fed("\u{feff}data: last".as_bytes()).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(events.finish().unwrap().as_deref(), Some("last"));
        // A mark that starts a later line makes it another field than data.
        let mut events = Events::new(1024);
        let later = "data: first\n\n\u{feff}data: second\n\ndata: third\n\n";
        assert_eq!(events.fed(later.as_bytes()).unwrap(), ["first", "third"]);
        // Two marks: the first alone is dropped.
        let mut events = Events::new(1024);
        let twice = "\u{feff}\u{feff}data: first\n\ndata: second\n\n";
        assert_eq!(events.fed(twice.as_bytes()).unwrap(), ["second"]);
    }

    /// The events `answer` gives before it fails and the failure, read in
    /// two chunks cut at `at` by a reader bounded at `limit`.
    fn read_until_it_fails(
        answer: &[u8],
        at: usize,
        limit: usize,
    ) -> (Vec<String>, Option<SseError>) {
        let (head, tail) = answer.split_at(at);
        let mut events = Events::new(limit);
        let mut read = Vec::new();
        for chunk in [head, tail] {
            let (ended, failed) = events.feed(chunk);
            read.extend(ended);
            if failed.is_some() {
                return (read, failed);
            }
        }
        match events.finish() {
            Ok(last) => (read.into_iter().chain(last).collect(), None),
            Err(error) => (read, Some(error)),
        }
    }

    /// What was read before a failure is handed on, then the failure,
    /// wherever the chunks are cut: an event and the line that fails may
    /// come in one read.
    #[test]
    fn the_events_before_a_failure_are_read_at_every_cut() {
        let malformed = SseError::Malformed {
            reason: "an event's data is not UTF-8".to_owned(),
        };
        let both = || vec!["one".to_owned(), "two".to_owned()];
        let no_utf8 = b"data: one\n\ndata: two\r\n\r\ndata: \xff\xfe\n\ndata: late\n\n";
        for at in 0..=no_utf8.len() {
            let read = read_until_it_fails(no_utf8, at, 1024);
            assert_eq!(read, (both(), Some(malformed.clone())), "{at}");
        }
        // The stream's bound is counted byte by byte: the events that end
        // within it are read, whatever chunk carries the byte past it.
        let past_the_bound = b"data: one\n\ndata: two\n\ndata: three\n\n";
        for (limit, within) in [(10, 0), (11, 1), (21, 1), (22, 2), (34, 2)] {
            let too_long = SseError::AnswerTooLong { limit };
            let read: Vec<String> = both().into_iter().take(within).collect();
            for at in 0..=past_the_bound.len() {
                let found = read_until_it_fails(past_the_bound, at, limit);
                assert_eq!(
                    found,
                    (read.clone(), Some(too_long.clone())),
                    "{limit} at {at}"
                );
            }
        }
        assert_eq!(read_until_it_fails(past_the_bound, 5, 35).1, None);
        // A line and an event past their bounds, after an event.
        let long_line = [&b"data: one\n\n"[..], &vec![b'x'; LINE_LIMIT + 1]].concat();
        let line_too_long = SseError::StreamLineTooLong { limit: LINE_LIMIT };
        let data_line = [&b"data: "[..], &vec![b'x'; EVENT_LIMIT / 2], b"\n"].concat();
        let long_event = [&b"data: one\n\n"[..], &data_line, &data_line, &data_line].concat();
        let event_too_long = SseError::StreamEventTooLong { limit: EVENT_LIMIT };
        let one = || vec!["one".to_owned()];
        for at in [0, 5, 11, 12, 4096] {
            let read = read_until_it_fails(&long_line, at, usize::MAX);
            assert_eq!(read, (one(), Some(line_too_long.clone())), "{at}");
            let read = read_until_it_fails(&long_event, at, usize::MAX);
            assert_eq!(read, (one(), Some(event_too_long.clone())), "{at}");
        }
    }

    /// The events of `answer` sent in two chunks, cut at `at`, then ended.
    fn read_in_two(answer: &[u8], at: usize) -> Result<Vec<String>, SseError> {
        let (head, tail) = answer.split_at(at);
        let mut events = Events::new(1024);
        let mut read = events.fed(head)?;
        read.extend(events.fed(tail)?);
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
            read.extend(reader.fed(&[*byte]).unwrap());
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
    fn a_data_line_without_a_colon_is_skipped() {
        let mut events = Events::new(1024);
        let bare = b"data: one\ndata\ndata: two\n\n";
        assert_eq!(events.fed(bare).unwrap(), ["one\ntwo"]);
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
        assert_eq!(events.fed(b"data: 12345678\n\n").unwrap(), ["12345678"]);
        assert_eq!(
            events.fed(b"\n"),
            Err(SseError::AnswerTooLong { limit: 16 })
        );
        let mut events = Events::new(16);
        assert_eq!(
            events.fed(b"data: 123456789\n\n"),
            Err(SseError::AnswerTooLong { limit: 16 })
        );
    }

    #[test]
    fn a_line_is_read_to_its_bound_and_refused_one_byte_past_it() {
        let too_long = Err(SseError::StreamLineTooLong { limit: LINE_LIMIT });
        let line = |length: usize| format!("data:{}\n\n", "x".repeat(length - 5));
        let mut events = Events::new(usize::MAX);
        let read = events.fed(line(LINE_LIMIT).as_bytes()).unwrap();
        let lengths: Vec<usize> = read.iter().map(String::len).collect();
        assert_eq!(lengths, [LINE_LIMIT - 5]);
        let mut events = Events::new(usize::MAX);
        assert_eq!(events.fed(line(LINE_LIMIT + 1).as_bytes()), too_long);
        // The bound is the line's, however many chunks bring it.
        let mut events = Events::new(usize::MAX);
        assert_eq!(events.fed(&vec![b'x'; LINE_LIMIT]), Ok(Vec::new()));
        assert_eq!(events.fed(b"x"), too_long);
    }

    /// The line break that joins two data lines counts in the event.
    #[test]
    fn an_event_is_read_to_its_bound_and_refused_one_byte_past_it() {
        let event = |second: usize| {
            let first = "x".repeat(EVENT_LIMIT - 10);
            format!("data:{first}\ndata:{}\n\n", "x".repeat(second))
        };
        let mut events = Events::new(usize::MAX);
        let read = events.fed(event(9).as_bytes()).unwrap();
        let lengths: Vec<usize> = read.iter().map(String::len).collect();
        assert_eq!(lengths, [EVENT_LIMIT]);
        let mut events = Events::new(usize::MAX);
        assert_eq!(
            events.fed(event(10).as_bytes()),
            Err(SseError::StreamEventTooLong { limit: EVENT_LIMIT })
        );
    }

    /// A server that sends a line without end is cut at the line's bound,
    /// whatever the answer's.
    #[test]
    fn a_line_that_never_ends_is_cut_at_its_bound() {
        let mut events = Events::new(usize::MAX);
        let chunk = vec![b'x'; 64 * 1024];
        let failed = (0..64).find_map(|_| events.fed(&chunk).err());
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
        let failed = (0..2048).find_map(|_| events.fed(line.as_bytes()).err());
        assert_eq!(
            failed,
            Some(SseError::StreamEventTooLong { limit: EVENT_LIMIT })
        );
    }

    /// Events that never stop are cut at the stream's bound.
    #[test]
    fn events_that_never_stop_are_cut_at_the_stream_s_bound() {
        let mut events = Events::new(4096);
        let failed = (0..1024).find_map(|_| events.fed(b"data: {}\n\n").err());
        assert_eq!(failed, Some(SseError::AnswerTooLong { limit: 4096 }));
    }
}
