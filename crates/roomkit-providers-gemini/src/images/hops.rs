//! An interaction continued hop after hop, as Python's google-genai SDK
//! continues one on its own (`continue_async_unary_interaction`): a hop cut
//! at the decode limit answers `requires_continuation` with a token, and
//! the next hop continues from it, until one completes. The hops are then
//! one interaction (`merge_unary_hops`): the last hop, with the steps of
//! every hop and the usage of them all.

use serde_json::Value;

/// The token `hop` asks to be continued from, unless it is the one `sent`
/// continued it from: continuing again would loop with no progress
/// (`_next_unary_hop_token`).
pub(crate) fn next_token<'a>(hop: &'a Value, sent: Option<&str>) -> Option<&'a str> {
    if hop.get("status").and_then(Value::as_str) != Some("requires_continuation") {
        return None;
    }
    let token = hop.get("continuation_token").and_then(Value::as_str);
    token.filter(|token| !token.is_empty() && Some(*token) != sent)
}

/// The id of `hop`, which owns the token it gave.
pub(crate) fn owner(hop: &Value) -> Option<&str> {
    hop.get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
}

/// The words of the SDK for a hop that failed after `hops` hops completed,
/// and `failure`, what failed it.
pub(crate) fn interrupted(hops: usize, failure: &str) -> String {
    format!(
        "Automatic continuation was interrupted after {hops} completed hop(s): {failure}. The \
         partial interaction (status 'requires_continuation') and its continuation_token can \
         be used to resume."
    )
}

/// `hops`, in order, as one interaction: the last, with the steps of every
/// hop, a later hop's leading user inputs left out (it repeats the input),
/// and their usage summed.
pub(crate) fn merged(mut hops: Vec<Value>) -> Value {
    let first_usage = hops.first().and_then(|hop| hop.get("usage")).cloned();
    let usages: Vec<Value> = hops
        .iter()
        .filter_map(|hop| hop.get("usage").filter(|usage| !usage.is_null()).cloned())
        .collect();
    let mut steps = Vec::new();
    for (at, hop) in hops.iter().enumerate() {
        let Some(hop_steps) = hop.get("steps").and_then(Value::as_array) else {
            continue;
        };
        let leading = hop_steps
            .iter()
            .take_while(|step| at > 0 && is_user_input(step));
        steps.extend(hop_steps.iter().skip(leading.count()).cloned());
    }
    let Some(Value::Object(mut last)) = hops.pop() else {
        return Value::Null;
    };
    last.insert("steps".to_owned(), Value::Array(steps));
    if let Some(usage) = merged_usage(&usages, first_usage.as_ref()) {
        last.insert("usage".to_owned(), usage);
    }
    Value::Object(last)
}

fn is_user_input(step: &Value) -> bool {
    step.get("type").and_then(Value::as_str) == Some("user_input")
}

/// The lists of a usage merged by a key, their counts summed
/// (`_USAGE_LIST_KEYS`): the list, the member that keys an entry, the
/// member that counts.
const USAGE_LISTS: [(&str, &str, &str); 5] = [
    ("cached_tokens_by_modality", "modality", "tokens"),
    ("input_tokens_by_modality", "modality", "tokens"),
    ("output_tokens_by_modality", "modality", "tokens"),
    ("tool_use_tokens_by_modality", "modality", "tokens"),
    ("grounding_tool_count", "type", "count"),
];

/// The usage of every hop as one (`_merge_usages`): each count summed, each
/// list merged, other members the last hop's, and the input of the first
/// hop kept as `initial_input_tokens`.
fn merged_usage(usages: &[Value], first: Option<&Value>) -> Option<Value> {
    let Value::Object(final_usage) = usages.last()? else {
        return usages.last().cloned();
    };
    let mut merged = final_usage.clone();
    let mut names: Vec<&String> = Vec::new();
    for usage in usages.iter().filter_map(Value::as_object) {
        names.extend(
            usage
                .keys()
                .filter(|name| !names.contains(name))
                .collect::<Vec<_>>(),
        );
    }
    for name in names
        .into_iter()
        .filter(|name| *name != "initial_input_tokens")
    {
        let present: Vec<&Value> = usages
            .iter()
            .filter_map(|usage| usage.get(name).filter(|value| !value.is_null()))
            .collect();
        if present.is_empty() {
            continue;
        }
        if present.iter().all(|value| is_int(value)) {
            let total = present
                .iter()
                .filter_map(|value| value.as_i64())
                .fold(0_i64, i64::saturating_add);
            merged.insert(name.clone(), total.into());
            continue;
        }
        let merged_by = USAGE_LISTS.iter().find(|(listed, _, _)| listed == name);
        let lists = present.iter().all(|value| value.is_array());
        if let Some((_, key, count)) = merged_by.filter(|_| lists) {
            merged.insert(name.clone(), merged_list(&present, key, count));
        }
    }
    let stated = |usage: Option<&Value>, name: &str| {
        usage
            .and_then(|usage| usage.get(name))
            .filter(|value| !value.is_null())
            .cloned()
    };
    let initial =
        stated(first, "initial_input_tokens").or_else(|| stated(first, "total_input_tokens"));
    if initial.is_some() || stated(usages.last(), "initial_input_tokens").is_some() {
        merged.insert(
            "initial_input_tokens".to_owned(),
            initial.unwrap_or(Value::Null),
        );
    }
    Some(Value::Object(merged))
}

/// Whether `value` is a whole number, as Python's `int` and not its `bool`.
fn is_int(value: &Value) -> bool {
    value.as_i64().is_some()
}

/// The entries of `lists` merged by their `key`, the first of each kept and
/// its `count` summed over the others (`_merge_usage_list`).
fn merged_list(lists: &[&Value], key: &str, count: &str) -> Value {
    let mut merged: Vec<(Value, Value)> = Vec::new();
    for item in lists.iter().filter_map(|list| list.as_array()).flatten() {
        let keyed = item.get(key).cloned().unwrap_or(Value::Null);
        let Some((_, kept)) = merged.iter_mut().find(|(seen, _)| *seen == keyed) else {
            merged.push((keyed, item.clone()));
            continue;
        };
        let (before, added) = (kept.get(count).and_then(Value::as_i64), item.get(count));
        if let (Some(before), Some(added)) = (before, added.filter(|added| is_int(added))) {
            let total = before.saturating_add(added.as_i64().unwrap_or_default());
            if let Value::Object(kept) = kept {
                kept.insert(count.to_owned(), total.into());
            }
        }
    }
    Value::Array(merged.into_iter().map(|(_, item)| item).collect())
}
