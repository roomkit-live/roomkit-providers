//! Where a replayed case differs from what Python did of it.

use std::collections::BTreeMap;
use std::error::Error as _;

use roomkit_providers_core::{ProviderError, ResponseSchemaError};
use serde::Serialize;
use serde_json::Value;

use super::{Case, Failure, Outcome, Replayed, Wait};

/// Where the requests the provider sent differ from Python's.
fn requests_differ(case: &Case, replayed: &Replayed, found: &mut Vec<String>) {
    if replayed.seen.len() != case.exchanges.len() {
        found.push(format!(
            "{} requests sent, where Python sent {}",
            replayed.seen.len(),
            case.exchanges.len()
        ));
    }
    for (at, (seen, exchange)) in replayed.seen.iter().zip(&case.exchanges).enumerate() {
        let sent = &exchange.request;
        let url = reqwest::Url::parse(&sent.url).unwrap();
        let target = &url[url::Position::BeforePath..];
        if (seen.method.as_str(), seen.target.as_str()) != (sent.method.as_str(), target) {
            found.push(format!("request {at}: {} {}", seen.method, seen.target));
        }
        let body = serde_json::from_slice::<Value>(&seen.body).ok();
        let written = |body: Option<&Value>| body.map(Value::to_string);
        if written(body.as_ref()) != written(sent.body.as_ref()) {
            found.push(format!(
                "request {at}: its body is {:?}",
                written(body.as_ref())
            ));
        }
        for (name, value) in &sent.headers {
            let key = format!("Bearer {}", replayed.config.api_key);
            let expected = value
                .as_ref()
                .map(|value| value.replace("Bearer <config key>", &key));
            if seen.headers.get(name) != expected.as_ref() {
                found.push(format!(
                    "request {at}: {name} is {:?}",
                    seen.headers.get(name)
                ));
            }
        }
        let timeouts = [
            ("connect", replayed.config.connect_timeout),
            ("read", replayed.config.timeout),
        ];
        for (name, timeout) in timeouts {
            if sent.timeouts.get(name) != Some(&timeout.as_secs_f64()) {
                found.push(format!("request {at}: the {name} timeout is {timeout:?}"));
            }
        }
    }
}

/// Where the waits the provider took between its tries differ from the
/// SDK's: one read from the answer's headers is exact, a backoff is within
/// a quarter below what the fixture writes.
fn waits_differ(case: &Case, replayed: &Replayed, found: &mut Vec<String>) {
    let expected: Vec<&Wait> = case
        .exchanges
        .iter()
        .filter_map(|e| e.wait.as_ref())
        .collect();
    if expected.len() != replayed.waits.len() {
        found.push(format!(
            "{} waits, where Python took {}",
            replayed.waits.len(),
            expected.len()
        ));
    }
    for (taken, wait) in replayed.waits.iter().zip(expected) {
        let (least, most) = if wait.from == "the answer's headers" {
            (wait.seconds, wait.seconds)
        } else {
            (wait.seconds * 0.75, wait.seconds)
        };
        if *taken < least - 1e-6 || *taken > most + 1e-6 {
            found.push(format!(
                "a wait of {taken} s, where Python took {} s",
                wait.seconds
            ));
        }
    }
}

/// Whether `given` has the shape of an id Python minted as `minted`:
/// `call_<name>_<12 hexadecimal digits>`, for the same name.
fn shaped_as(given: &str, minted: &str) -> bool {
    let (name, drawn) = given.rsplit_once('_').unwrap_or_default();
    let shaped = drawn.len() == 12 && drawn.bytes().all(|byte| byte.is_ascii_hexdigit());
    shaped
        && minted
            .rsplit_once('_')
            .is_some_and(|(expected, _)| expected == name)
}

/// Writes into `given` the ids Python minted, where `given` holds an id of
/// the same shape in the same place: such an id is random, so its shape is
/// compared, and that the items of one call share one id, not its value.
/// `held` keeps which id of the provider stands for which of Python's.
fn name_minted_ids(
    case: &Case,
    expected: &Value,
    given: &mut Value,
    held: &mut BTreeMap<String, String>,
) {
    match (expected, given) {
        (Value::Array(expected), Value::Array(given)) => {
            for (expected, given) in expected.iter().zip(given) {
                name_minted_ids(case, expected, given, held);
            }
        }
        (Value::Object(expected), Value::Object(given)) => {
            for (name, expected) in expected {
                let Some(given) = given.get_mut(name) else {
                    continue;
                };
                let minted = expected.as_str().filter(|id| {
                    name == "id" && case.minted_call_ids.iter().any(|minted| minted == id)
                });
                let (Some(minted), Some(id)) = (minted, given.as_str().map(str::to_owned)) else {
                    name_minted_ids(case, expected, given, held);
                    continue;
                };
                let stands_for = held.entry(id.clone()).or_insert_with(|| minted.to_owned());
                if shaped_as(&id, minted) && stands_for == minted {
                    *given = minted.into();
                }
            }
        }
        _ => {}
    }
}

/// Where what the provider gave differs from what Python gave: `given`
/// against `expected`, as texts, so that the members come in the same order
/// too.
fn result_differs<T: Serialize>(
    case: &Case,
    what: &str,
    expected: Option<&Value>,
    given: &T,
    found: &mut Vec<String>,
) {
    let mut given = serde_json::to_value(given).unwrap();
    let Some(expected) = expected else {
        return found.push(format!("{what} where Python gives none: {given}"));
    };
    name_minted_ids(case, expected, &mut given, &mut BTreeMap::new());
    let (given, expected) = (given.to_string(), expected.to_string());
    if given != expected {
        found.push(format!("{what}: {given}"));
    }
}

/// Where the failure of the provider differs from Python's, by what the
/// case says a replay reproduces.
fn failure_differs(case: &Case, failure: &Failure, error: &ProviderError, found: &mut Vec<String>) {
    let Some(compare) = &case.compare else {
        return found.push("a failure with no rule to compare it".to_owned());
    };
    if compare.exactly.is_empty() {
        return found.push(format!("a {} no provider reproduces", failure.class));
    }
    let schema = error
        .source()
        .and_then(|source| source.downcast_ref::<ResponseSchemaError>());
    for attribute in &compare.exactly {
        let same = match attribute.as_str() {
            "class" => (failure.class == "ResponseSchemaError") == schema.is_some(),
            "status_code" => failure.status_code == error.status_code,
            "retryable" => failure.retryable == Some(error.retryable),
            "provider" => failure.provider.as_deref() == Some(error.provider.as_str()),
            "context_overflow" => failure.context_overflow == error.context_overflow,
            "before_request" => failure.before_request == Some(error.before_request),
            "reason" => failure.reason.as_deref() == schema.map(|schema| schema.reason.as_str()),
            // What a stream gave before it failed is compared with its items.
            "events" | "text" => true,
            other => panic!("{}: {other} is not compared", case.name),
        };
        if !same {
            found.push(format!("the failure's {attribute} differs: {error:?}"));
        }
    }
    let compared = matches!(compare.message.as_str(), "roomkit" | "server");
    if compared && error.message != failure.message {
        found.push(format!("the failure says {:?}", error.message));
    }
}

/// Where the end of a call differs from Python's: its failure, or none.
fn end_differs(case: &Case, error: Option<&ProviderError>, found: &mut Vec<String>) {
    match (error, &case.expected.error) {
        (Some(error), Some(failure)) => failure_differs(case, failure, error, found),
        (Some(error), None) => found.push(format!("a failure where Python answers: {error:?}")),
        (None, Some(failure)) => {
            found.push(format!("an answer where Python fails: {}", failure.message));
        }
        (None, None) => {}
    }
}

/// Every way `replayed` differs from what Python did of `case`; none when
/// the provider replays the case as Python.
pub(super) fn differences(case: &Case, replayed: &Replayed) -> Vec<String> {
    let mut found = Vec::new();
    if let Some(unbuilt) = &replayed.unbuilt {
        found.push(format!("the provider is not built: {}", unbuilt.message));
        return found;
    }
    requests_differ(case, replayed, &mut found);
    waits_differ(case, replayed, &mut found);
    let expected = &case.expected;
    let list = |items: &Option<Vec<Value>>| items.clone().map(Value::Array);
    let texts = expected.text.clone().map(Value::from);
    match &replayed.outcome {
        Some(Outcome::Response(Ok(response))) => {
            let expected = expected.response.as_ref();
            result_differs(case, "the response", expected, response, &mut found);
            end_differs(case, None, &mut found);
        }
        Some(Outcome::Models(Ok(models))) => {
            let expected = list(&expected.models);
            result_differs(case, "the models", expected.as_ref(), models, &mut found);
            end_differs(case, None, &mut found);
        }
        Some(Outcome::Response(Err(error)) | Outcome::Models(Err(error))) => {
            end_differs(case, Some(error), &mut found);
        }
        // What a stream gave before it failed is compared too.
        Some(Outcome::Events(events, error)) => {
            let expected = list(&expected.events);
            result_differs(case, "the events", expected.as_ref(), events, &mut found);
            end_differs(case, error.as_ref(), &mut found);
        }
        Some(Outcome::Text(text, error)) => {
            result_differs(case, "the text", texts.as_ref(), text, &mut found);
            end_differs(case, error.as_ref(), &mut found);
        }
        None => found.push("nothing was asked".to_owned()),
    }
    found
}
