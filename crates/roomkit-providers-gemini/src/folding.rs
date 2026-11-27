//! The chunks of a stream folded into the contract's events (Python's
//! `GeminiAIProvider._events`): each chunk's text and thoughts handed on as
//! they come, its usage read, its input told once; the calls folded across
//! the chunks, a call re-emitted later folding into the first while two
//! identical calls of one chunk stay two, each keeping the signature either
//! copy carried; then, at the end, the round's calls and the done event,
//! its finish, its usage, the model that answered and a refusal.

use roomkit::model::Metadata;
use roomkit_ai::{
    CallIds, FinishReason, StreamDone, StreamEvent, StreamTextDelta, StreamThinkingDelta,
    StreamToolCall, StreamUsage,
};
use serde_json::{Value, json};

use crate::request::contents::SIGNATURE;

/// The finish reasons that mean the model withheld its answer: a refusal.
const REFUSALS: [&str; 5] = [
    "SAFETY",
    "RECITATION",
    "PROHIBITED_CONTENT",
    "BLOCKLIST",
    "SPII",
];

/// One call of the round, as its copies told it.
struct Call {
    server_id: Option<String>,
    name: String,
    arguments: Metadata,
    signature: Option<String>,
    /// The call's name and arguments, written as one key, and which copy
    /// of it in its chunk it was.
    key: String,
}

/// What the stream told so far.
pub(crate) struct Folding<'a> {
    model: &'a str,
    calls: Vec<Call>,
    usage: Metadata,
    told_input: bool,
    finish: Option<String>,
    blocked: Option<String>,
    answered: Option<String>,
    chunks: usize,
}

impl<'a> Folding<'a> {
    pub(crate) fn new(model: &'a str) -> Self {
        Self {
            model,
            calls: Vec::new(),
            usage: Metadata::new(),
            told_input: false,
            finish: None,
            blocked: None,
            answered: None,
            chunks: 0,
        }
    }

    /// Whether a chunk came.
    pub(crate) fn has_started(&self) -> bool {
        self.chunks > 0
    }

    /// Folds `chunk`, pushing onto `ready` the events it gives.
    pub(crate) fn take(&mut self, chunk: &Value, ready: &mut Vec<StreamEvent>) {
        self.chunks += 1;
        if let Some(counted) = chunk
            .get("usageMetadata")
            .filter(|counted| counted.is_object())
        {
            self.usage = usage_of(counted);
            let prompt = counted.get("promptTokenCount").and_then(Value::as_u64);
            if !self.told_input && prompt.is_some_and(|prompt| prompt > 0) {
                self.told_input = true;
                let usage = input_counters(&self.usage);
                ready.push(StreamEvent::Usage(StreamUsage { usage }));
            }
        }
        let version = chunk.get("modelVersion").and_then(Value::as_str);
        if let Some(version) = version.filter(|version| !version.is_empty()) {
            self.answered = Some(version.to_owned());
        }
        let blocked = chunk
            .pointer("/promptFeedback/blockReason")
            .and_then(Value::as_str);
        if let Some(blocked) = blocked.filter(|blocked| !blocked.is_empty()) {
            self.blocked = Some(blocked.to_owned());
        }
        let candidate = chunk
            .get("candidates")
            .and_then(Value::as_array)
            .and_then(|c| c.first());
        let Some(candidate) = candidate else {
            return;
        };
        let finish = candidate.get("finishReason").and_then(Value::as_str);
        if let Some(finish) = finish.filter(|finish| !finish.is_empty()) {
            self.finish = Some(finish.to_owned());
        }
        let parts = candidate
            .pointer("/content/parts")
            .and_then(Value::as_array);
        let Some(parts) = parts.filter(|parts| !parts.is_empty()) else {
            return;
        };
        let mut in_chunk: Vec<(String, usize)> = Vec::new();
        for part in parts {
            let text = part.get("text").and_then(Value::as_str).unwrap_or_default();
            if !text.is_empty() {
                let thought = part.get("thought").and_then(Value::as_bool) == Some(true);
                ready.push(if thought {
                    StreamEvent::ThinkingDelta(StreamThinkingDelta::new(text))
                } else {
                    StreamEvent::TextDelta(StreamTextDelta::new(text))
                });
            } else if let Some(function) = part.get("functionCall").filter(|f| is_set(f)) {
                self.fold(part, function, &mut in_chunk);
            }
        }
    }

    /// Folds one function-call part into the round's calls (Python's
    /// `_fold_function_call` and `_call_key`).
    fn fold(&mut self, part: &Value, function: &Value, in_chunk: &mut Vec<(String, usize)>) {
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let arguments = match function.get("args") {
            Some(Value::Object(arguments)) => arguments.clone(),
            _ => Metadata::new(),
        };
        let server_id = function
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty());
        let signature = part.get("thoughtSignature").and_then(Value::as_str);
        let signature = signature
            .filter(|signature| !signature.is_empty())
            .map(str::to_owned);
        // A call's own id finds the call an earlier copy carried it on.
        let by_id =
            |call: &&mut Call| server_id.is_some() && call.server_id.as_deref() == server_id;
        if let Some(held) = self.calls.iter_mut().find(by_id) {
            held.signature = held.signature.take().or(signature);
            return;
        }
        let base = format!("{name}::{}", fingerprint(&arguments));
        let occurrence =
            if let Some((_, count)) = in_chunk.iter_mut().find(|(seen, _)| *seen == base) {
                *count += 1;
                *count - 1
            } else {
                in_chunk.push((base.clone(), 1));
                0
            };
        let mut key = format!("{base}#{occurrence}");
        let held = self.calls.iter().find(|call| call.key == key);
        if let (Some(id), Some(held)) = (server_id, held) {
            let other = held.server_id.as_deref();
            if other.is_some_and(|other| other != id) {
                key = format!("{key}@{id}");
            }
        }
        match self.calls.iter_mut().find(|call| call.key == key) {
            Some(held) => {
                held.signature = held.signature.take().or(signature);
                if held.server_id.is_none() {
                    held.server_id = server_id.map(str::to_owned);
                }
            }
            None => self.calls.push(Call {
                server_id: server_id.map(str::to_owned),
                name: name.to_owned(),
                arguments,
                signature,
                key,
            }),
        }
    }

    /// The round's calls and the done event, pushed onto `ready`.
    pub(crate) fn finish(self, ready: &mut Vec<StreamEvent>) {
        let mut ids = CallIds::new();
        for call in self.calls {
            let mut metadata = Metadata::new();
            if let Some(signature) = call.signature {
                metadata.insert(SIGNATURE.to_owned(), signature.into());
            }
            let id = ids.call(call.server_id.as_deref().unwrap_or_default(), &call.name);
            let mut event = StreamToolCall::new(id, call.name);
            event.arguments = call.arguments;
            event.metadata = metadata;
            ready.push(StreamEvent::ToolCall(event));
        }
        let refused = self
            .finish
            .as_deref()
            .filter(|finish| REFUSALS.contains(finish));
        let mut metadata = Metadata::new();
        let answered = self.answered.as_deref().unwrap_or(self.model);
        metadata.insert("model".to_owned(), answered.into());
        if let Some(refusal) = refused.or(self.blocked.as_deref()) {
            metadata.insert("refusal".to_owned(), refusal.into());
        }
        if let Some(blocked) = &self.blocked {
            metadata.insert("prompt_block_reason".to_owned(), blocked.as_str().into());
        }
        ready.push(StreamEvent::Done(StreamDone {
            finish_reason: finish_reason_of(self.finish.as_deref()),
            usage: self.usage,
            metadata,
        }));
    }
}

/// Whether a function call part names a call, as Python's truth reads it.
fn is_set(function: &Value) -> bool {
    function
        .as_object()
        .is_some_and(|members| !members.is_empty())
}

/// A call's arguments as one key: Python's `json.dumps(args, sort_keys=True)`.
fn fingerprint(arguments: &Metadata) -> String {
    fn sorted(value: &Value) -> Value {
        match value {
            Value::Object(members) => {
                let mut names: Vec<&String> = members.keys().collect();
                names.sort();
                let ordered = names
                    .into_iter()
                    .map(|name| (name.clone(), sorted(&members[name])))
                    .collect();
                Value::Object(ordered)
            }
            Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
            other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => {
                other.clone()
            }
        }
    }
    sorted(&Value::Object(arguments.clone())).to_string()
}

/// Gemini's usage as the contract's disjoint counters (Python's
/// `_usage_from_metadata`): the input without what the cache held, the
/// thoughts counted as output and told apart.
fn usage_of(counted: &Value) -> Metadata {
    let count = |name: &str| {
        counted
            .get(name)
            .and_then(Value::as_u64)
            .unwrap_or_default()
    };
    let (prompt, cached, thoughts) = (
        count("promptTokenCount"),
        count("cachedContentTokenCount"),
        count("thoughtsTokenCount"),
    );
    let mut usage = Metadata::new();
    usage.insert(
        "input_tokens".to_owned(),
        json!(prompt.saturating_sub(cached)),
    );
    usage.insert(
        "output_tokens".to_owned(),
        json!(count("candidatesTokenCount") + thoughts),
    );
    if cached > 0 {
        usage.insert("cache_read_input_tokens".to_owned(), json!(cached));
    }
    if thoughts > 0 {
        usage.insert("reasoning_tokens".to_owned(), json!(thoughts));
    }
    usage
}

/// The input counters of `usage`, what a chunk tells before the end.
fn input_counters(usage: &Metadata) -> Metadata {
    usage
        .iter()
        .filter(|(name, _)| matches!(name.as_str(), "input_tokens" | "cache_read_input_tokens"))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

/// A finish reason in the contract's words; none for one with no word.
fn finish_reason_of(finish: Option<&str>) -> Option<FinishReason> {
    match finish? {
        "STOP" => Some(FinishReason::Stop),
        "MAX_TOKENS" => Some(FinishReason::Length),
        "MALFORMED_FUNCTION_CALL" => Some(FinishReason::MalformedCall),
        "UNEXPECTED_TOOL_CALL" => Some(FinishReason::UnexpectedCall),
        refusal if REFUSALS.contains(&refusal) => Some(FinishReason::ContentFilter),
        _ => None,
    }
}
