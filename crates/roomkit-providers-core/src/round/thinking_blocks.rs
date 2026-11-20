//! A response's reasoning blocks, folded from its streamed deltas (RFC §6.4):
//! Python's `ThinkingBlocks`.
//!
//! A vendor whose reasoning comes in blocks signs each block and wants the
//! blocks back as it sent them: it refuses a replayed round whose blocks
//! changed in number, and a block without its signature. A redacted block
//! comes as opaque data, replayed as received. A delta names its block; one
//! that names none belongs to the response's single block, which keeps the
//! joined text and the last signature, as a provider without blocks reports
//! its reasoning.
//!
//! A named block without a signature was cut before it ended (the output
//! cap): it is not replayed, since the vendor refuses an unsigned block and
//! accepts a round without it.

use std::collections::HashMap;

use crate::request::AiThinkingPart;
use crate::response::StreamThinkingDelta;

/// One block, as its deltas came.
#[derive(Clone, Debug, Default)]
struct Block {
    text: String,
    signature: Option<String>,
    redacted: Option<String>,
}

impl Block {
    /// The part this block replays as, `named` when its vendor named it.
    fn part(&self, named: bool) -> Option<AiThinkingPart> {
        if let Some(redacted) = &self.redacted {
            return Some(AiThinkingPart {
                redacted: Some(redacted.clone()),
                ..AiThinkingPart::new("")
            });
        }
        let empty = self.text.is_empty() && self.signature.is_none();
        if empty || (named && self.signature.is_none()) {
            return None;
        }
        Some(AiThinkingPart {
            signature: self.signature.clone(),
            ..AiThinkingPart::new(self.text.clone())
        })
    }
}

/// The reasoning blocks of one response, in the order they opened.
#[derive(Clone, Debug, Default)]
pub struct ThinkingBlocks {
    /// The blocks, each with its name, in the order they opened.
    blocks: Vec<(Option<u32>, Block)>,
    /// Where each block is among `blocks`, by its name: a delta finds its
    /// block at once, however many blocks a server names.
    places: HashMap<Option<u32>, usize>,
    last_signature: Option<String>,
}

impl ThinkingBlocks {
    /// No block yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Folds `delta` into its block; whether it opened one.
    pub fn add(&mut self, delta: &StreamThinkingDelta) -> bool {
        let opened = !self.places.contains_key(&delta.block);
        let at = *self.places.entry(delta.block).or_insert(self.blocks.len());
        if opened {
            self.blocks.push((delta.block, Block::default()));
        }
        let Some((_, held)) = self.blocks.get_mut(at) else {
            return opened;
        };
        held.text.push_str(&delta.thinking);
        if let Some(signature) = delta.signature.as_ref().filter(|given| !given.is_empty()) {
            held.signature = Some(signature.clone());
            self.last_signature = Some(signature.clone());
        }
        if let Some(redacted) = &delta.redacted {
            held.redacted = Some(redacted.clone());
        }
        opened
    }

    /// The last signature any block got, for a reader of one signature, as a
    /// provider fills
    /// [`AiResponse::thinking_signature`](crate::AiResponse::thinking_signature).
    #[must_use]
    pub fn last_signature(&self) -> Option<&str> {
        self.last_signature.as_deref()
    }

    /// Whether the response reasoned at all.
    #[must_use]
    pub fn seen(&self) -> bool {
        !self.blocks.is_empty()
    }

    /// All the reasoning text, every block's, replayable or not: what the
    /// provider exposed of its reasoning.
    #[must_use]
    pub fn text(&self) -> String {
        self.blocks
            .iter()
            .map(|(_, held)| held.text.as_str())
            .collect()
    }

    /// Whether the vendor named its blocks, so their place in the round
    /// counts.
    #[must_use]
    pub fn keyed(&self) -> bool {
        let unnamed = usize::from(self.places.contains_key(&None));
        self.places.len() > unnamed
    }

    /// The part a block replays as, `None` for one with nothing to replay.
    #[must_use]
    pub fn part(&self, block: Option<u32>) -> Option<AiThinkingPart> {
        let at = *self.places.get(&block)?;
        let (_, held) = self.blocks.get(at)?;
        held.part(block.is_some())
    }

    /// Every block's part, in the order the blocks opened.
    #[must_use]
    pub fn parts(&self) -> Vec<AiThinkingPart> {
        self.blocks
            .iter()
            .filter_map(|(block, held)| held.part(block.is_some()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delta(block: Option<u32>, thinking: &str) -> StreamThinkingDelta {
        StreamThinkingDelta {
            block,
            ..StreamThinkingDelta::new(thinking)
        }
    }

    fn signed(block: Option<u32>, signature: &str) -> StreamThinkingDelta {
        StreamThinkingDelta {
            signature: Some(signature.to_owned()),
            ..delta(block, "")
        }
    }

    fn part(thinking: &str, signature: Option<&str>, redacted: Option<&str>) -> AiThinkingPart {
        AiThinkingPart {
            thinking: thinking.to_owned(),
            signature: signature.map(str::to_owned),
            redacted: redacted.map(str::to_owned),
        }
    }

    // Python: tests/test_providers/test_thinking_blocks.py::TestThinkingBlocks::test_each_block_keeps_its_text_and_signature
    #[test]
    fn each_block_keeps_its_text_and_signature() {
        let mut blocks = ThinkingBlocks::new();
        let redacted = StreamThinkingDelta {
            redacted: Some("RRR".to_owned()),
            ..delta(Some(2), "")
        };
        for given in [
            delta(Some(0), "first "),
            delta(Some(0), "thought"),
            signed(Some(0), "S0"),
            redacted,
            delta(Some(3), "second"),
            signed(Some(3), "S3"),
        ] {
            blocks.add(&given);
        }
        assert_eq!(
            blocks.parts(),
            [
                part("first thought", Some("S0"), None),
                part("", None, Some("RRR")),
                part("second", Some("S3"), None),
            ]
        );
        assert_eq!(blocks.last_signature(), Some("S3"));
    }

    // Python: tests/test_providers/test_thinking_blocks.py::TestThinkingBlocks::test_a_named_block_cut_before_its_signature_is_not_replayed
    #[test]
    fn a_named_block_cut_before_its_signature_is_not_replayed() {
        let mut blocks = ThinkingBlocks::new();
        let given = [
            delta(Some(0), "kept"),
            signed(Some(0), "S0"),
            delta(Some(2), "cut"),
        ];
        let opened: Vec<bool> = given.iter().map(|given| blocks.add(given)).collect();
        assert_eq!(opened, [true, false, true]);
        assert_eq!(blocks.parts(), [part("kept", Some("S0"), None)]);
        assert_eq!(blocks.part(Some(2)), None);
        assert_eq!(blocks.text(), "keptcut");
        assert_eq!(blocks.last_signature(), Some("S0"));
    }

    // Python holds its blocks in a dict: a delta finds its block whatever
    // their number and the order they are named in, and the blocks keep the
    // order they opened in.
    #[test]
    fn many_blocks_named_out_of_order_keep_their_text_and_the_order_they_opened_in() {
        const COUNT: u32 = 2_000;
        // Every name once, in an order that is neither rising nor falling.
        let names: Vec<u32> = (0..COUNT).map(|at| (at * 7_919) % COUNT).collect();
        let mut blocks = ThinkingBlocks::new();
        assert!(blocks.add(&delta(None, "unnamed")));
        for name in &names {
            assert!(blocks.add(&delta(Some(*name), &format!("b{name} "))));
        }
        for name in names.iter().rev() {
            let more = StreamThinkingDelta {
                signature: Some(format!("S{name}")),
                ..delta(Some(*name), "more")
            };
            assert!(!blocks.add(&more), "block {name} is open already");
        }
        assert!(!blocks.add(&delta(None, " block")));

        let parts = blocks.parts();
        assert_eq!(parts.len(), names.len() + 1);
        assert_eq!(parts[0], part("unnamed block", None, None));
        for (replayed, name) in parts.iter().skip(1).zip(&names) {
            let signature = format!("S{name}");
            assert_eq!(
                replayed,
                &part(&format!("b{name} more"), Some(&signature), None)
            );
            assert_eq!(blocks.part(Some(*name)).as_ref(), Some(replayed));
        }
        assert_eq!(blocks.part(Some(COUNT)), None);
        assert_eq!(blocks.part(None), Some(part("unnamed block", None, None)));
        assert!(blocks.keyed());
        assert_eq!(blocks.last_signature(), Some("S0"));
        assert!(blocks.text().starts_with("unnamed blockb0 more"));
    }

    // Python: tests/test_providers/test_thinking_blocks.py::TestThinkingBlocks::test_reasoning_without_blocks_is_one_block
    #[test]
    fn reasoning_without_blocks_is_one_block() {
        let mut blocks = ThinkingBlocks::new();
        assert!(!blocks.seen());
        for given in [delta(None, "a"), signed(None, "S1"), delta(None, "b")] {
            blocks.add(&given);
        }
        assert!(blocks.seen());
        assert!(!blocks.keyed());
        assert_eq!(blocks.parts(), [part("ab", Some("S1"), None)]);
    }
}
