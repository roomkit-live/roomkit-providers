//! The replay of an image provider's cases (RFC §25): what a case asks the
//! image surface, the call it makes, and where what the provider gave
//! differs from what Python gave.
//!
//! A case's request is written as the image surface takes it: `prompt`,
//! `size`, `n`, `reference_images` and `mask` (image parts), `options` (the
//! controls) and `progress`: none, `record` (a callback that records every
//! attempt it is told of) or `fail` (one that records it, then fails when
//! told of a succeeded attempt). Python's provider draws an attempt's id at
//! random; the replay names the ids each side gives in the order they come.

use std::collections::BTreeMap;
use std::error::Error as _;
use std::sync::{Arc, Mutex};

use roomkit_ai::image::{
    GenerateOptions, GenerateWithOptionsOptions, ImageAttempt, ImageAttemptStatus, ImageError,
    ImageGenerationError, ImageOptions, ImageProgressCallback, ImageProvider, ImageResult,
};
use roomkit_ai::{AiImagePart, ProviderError};
use serde::Deserialize;
use serde_json::Value;

use super::compare::{differences, result_differs};
use super::fake::Door;
use super::named::Named;
use super::self_test::Wrong;
use super::{
    Called, Caller, Case, Failure, Fixture, Outcome, Replayed, Replays, judged, reply_of, served,
};

/// The call of a generation, `generate`.
pub const IMAGE_GENERATE: &str = "image_generate";

/// The call of a generation with its controls, `generate_with_options`.
pub const IMAGE_WITH_OPTIONS: &str = "image_generate_with_options";

/// What a case asks the image surface.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Asked {
    prompt: String,
    size: Option<String>,
    n: u32,
    reference_images: Vec<Value>,
    options: Option<ImageOptions>,
    mask: Option<Value>,
    progress: Option<String>,
}

/// An image part as the fixture writes Python's `AIImagePart`.
fn part_of(written: &Value) -> AiImagePart {
    AiImagePart {
        url: written["url"].as_str().unwrap().to_owned(),
        mime_type: written["mime_type"].as_str().map(str::to_owned),
    }
}

/// The callback of a case's `progress`, and what it was told, in order.
fn callback_of(
    progress: Option<&str>,
) -> (Option<ImageProgressCallback>, Arc<Mutex<Vec<ImageAttempt>>>) {
    let told = Arc::new(Mutex::new(Vec::new()));
    let fails = match progress {
        None => return (None, told),
        Some("record") => false,
        Some("fail") => true,
        Some(other) => panic!("an unknown progress {other}"),
    };
    let kept = Arc::clone(&told);
    let callback: ImageProgressCallback = Arc::new(move |attempt: ImageAttempt| {
        let succeeded = attempt.status == ImageAttemptStatus::Succeeded;
        kept.lock().unwrap().push(attempt);
        Box::pin(async move {
            if fails && succeeded {
                return Err("storage unavailable".into());
            }
            Ok(())
        })
    });
    (Some(callback), told)
}

/// An image provider's call of a case's request.
struct ImageCalls(Asked);

impl<P: Replays + ImageProvider> Caller<P> for ImageCalls {
    fn call<'a>(&'a self, provider: &'a P, case: &'a Case, _given: &'a Mutex<usize>) -> Called<'a> {
        let asked = &self.0;
        Box::pin(async move {
            let reference_images = asked.reference_images.iter().map(part_of).collect();
            let (size, n) = (asked.size.clone(), asked.n);
            let (on_progress, told) = callback_of(asked.progress.as_deref());
            let given = match case.call.as_str() {
                IMAGE_GENERATE => {
                    let options = GenerateOptions {
                        size,
                        n,
                        reference_images,
                    };
                    provider.generate(&asked.prompt, options).await
                }
                IMAGE_WITH_OPTIONS => {
                    let options = GenerateWithOptionsOptions {
                        size,
                        n,
                        reference_images,
                        options: asked.options.clone(),
                        mask: asked.mask.as_ref().map(part_of),
                        on_progress,
                    };
                    provider.generate_with_options(&asked.prompt, options).await
                }
                other => panic!("{}: an unknown call {other}", case.name),
            };
            let told = told.lock().unwrap().clone();
            Outcome::Images(given, told)
        })
    }
}

/// Replays `case`, a case of `fixture` that calls an image provider: as
/// [`replay`](super::replay) for a model's.
pub async fn replay<P: Replays + ImageProvider>(
    fixture: &Fixture,
    case: &Case,
    door: &Door,
) -> Replayed {
    let mut replayed = Replayed::of(case);
    let asked = match serde_json::from_value::<Asked>(case.request.clone()) {
        Ok(asked) => asked,
        Err(error) => {
            replayed.unread = Some(error.to_string());
            return replayed;
        }
    };
    served::<P, _>(fixture, case, door, &reply_of, replayed, &ImageCalls(asked)).await
}

/// Replays every case of `fixture` whose call is `call`, an image
/// provider's: how many replayed as Python, and how many are among `named`
/// and differ and do as their entry states. Fails for a case that is
/// neither, and for a named case that replays as Python. No request of the
/// image surface lends a key of its own.
pub async fn replayed_or_named<P: Replays + ImageProvider>(
    fixture: &Fixture,
    named: &Named,
    call: &str,
) -> (usize, usize) {
    let named = named.stated();
    let (mut as_python, mut otherwise, mut failures) = (0, 0, Vec::new());
    let door = Door::open().await;
    for case in fixture.cases.iter().filter(|case| case.call == call) {
        let replayed = replay::<P>(fixture, case, &door).await;
        match judged::<P>(case, named.get(case.name.as_str()), &replayed) {
            Ok(true) => as_python += 1,
            Ok(false) => otherwise += 1,
            Err(wrong) => failures.push(format!("{}: {wrong}", case.name)),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    (as_python, otherwise)
}

/// The attempt ids of `values`, in the order they come, each named by its
/// place: what a replay compares, Python and the provider drawing them at
/// random.
fn name_attempt_ids(values: &mut [&mut Value]) {
    let mut named = BTreeMap::new();
    for value in values {
        name_ids(value, &mut named);
    }
}

/// The ids of `value` named, as `named` names them, a new one by its place.
fn name_ids(value: &mut Value, named: &mut BTreeMap<String, String>) {
    match value {
        Value::Object(members) => {
            for (key, member) in members.iter_mut() {
                if matches!(key.as_str(), "id" | "attempt_id")
                    && let Value::String(id) = member
                {
                    let next = format!("<attempt {}>", named.len());
                    let name = named.entry(id.clone()).or_insert(next).clone();
                    *member = Value::String(name);
                    continue;
                }
                name_ids(member, named);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| name_ids(item, named)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// What the image surface gave, as the fixture writes Python's: the images,
/// the attempts the callback was told of, and the attempts and the images
/// of a failed generation.
fn given_of(given: &Result<Vec<ImageResult>, ImageError>, told: &[ImageAttempt]) -> [Value; 4] {
    let images = match given {
        Ok(images) => serde_json::to_value(images).unwrap(),
        Err(_) => Value::Null,
    };
    let generation = match given {
        Err(ImageError::Provider(error)) => generation_of(error),
        Ok(_) | Err(ImageError::Value(_)) => None,
    };
    let (attempts, results) = generation.map_or((Value::Null, Value::Null), |failed| {
        let attempts = serde_json::to_value(&failed.attempts).unwrap();
        (attempts, serde_json::to_value(&failed.results).unwrap())
    });
    [
        serde_json::to_value(told).unwrap(),
        images,
        attempts,
        results,
    ]
}

/// The generation failure `error` carries, when it carries one.
fn generation_of(error: &ProviderError) -> Option<&ImageGenerationError> {
    error.source()?.downcast_ref::<ImageGenerationError>()
}

/// Where what an image provider gave differs from what Python gave: its
/// images or its failure, and the attempts its callback was told of.
pub(super) fn differs(
    case: &Case,
    given: &Result<Vec<ImageResult>, ImageError>,
    told: &[ImageAttempt],
    found: &mut Vec<String>,
) {
    let expected = &case.expected;
    let failure = expected.error.as_ref();
    let mut expected_values = [
        Value::Array(expected.progress.clone().unwrap_or_default()),
        expected.results.clone().map_or(Value::Null, Value::Array),
        failure
            .and_then(|failure| failure.attempts.clone())
            .map_or(Value::Null, Value::Array),
        failure
            .and_then(|failure| failure.results.clone())
            .map_or(Value::Null, Value::Array),
    ];
    let mut given_values = given_of(given, told);
    // Attempts made at once tell their moves in the order they come: each
    // attempt's moves are compared in their order, not their interleaving.
    // So the replay proves no attempts were made at once: a provider that
    // makes them so proves it by a test of its own.
    by_attempt(&mut expected_values[0]);
    by_attempt(&mut given_values[0]);
    name_attempt_ids(&mut expected_values.iter_mut().collect::<Vec<_>>());
    name_attempt_ids(&mut given_values.iter_mut().collect::<Vec<_>>());
    // An attempt's error holds the failure's message: compared where the
    // message is.
    if !message_compared(case) {
        for value in expected_values.iter_mut().chain(given_values.iter_mut()) {
            unworded(value);
        }
    }
    let [
        expected_told,
        expected_images,
        expected_attempts,
        expected_results,
    ] = expected_values;
    let [given_told, given_images, given_attempts, given_results] = given_values;
    if expected.progress.is_some() {
        result_differs(
            case,
            "the attempts told",
            Some(&expected_told),
            &given_told,
            found,
        );
    }
    match (given, failure) {
        (Ok(_), None) => {
            result_differs(
                case,
                "the images",
                Some(&expected_images),
                &given_images,
                found,
            );
        }
        (Ok(_), Some(failure)) => {
            found.push(format!("images where Python fails: {}", failure.message));
        }
        (Err(error), None) => found.push(format!("a failure where Python answers: {error:?}")),
        (Err(error), Some(failure)) => {
            let outcomes = [
                (expected_attempts, given_attempts),
                (expected_results, given_results),
            ];
            failure_differs(case, failure, error, &outcomes, found);
        }
    }
}

/// The attempts told, `told`, in the order of their attempts' indexes, each
/// attempt's in the order it was told.
fn by_attempt(told: &mut Value) {
    if let Value::Array(attempts) = told {
        attempts.sort_by_key(|attempt| attempt.get("index").and_then(Value::as_u64));
    }
}

/// Whether the case compares its failure's message: RoomKit's words, or the
/// server's handed on, never a client's nor Python's repr of a body.
fn message_compared(case: &Case) -> bool {
    let compare = case.compare.as_ref();
    compare.is_some_and(|compare| matches!(compare.message.as_str(), "roomkit" | "server"))
}

/// `value` with the words of every attempt's error left out: whether the
/// attempt has an error is still compared.
fn unworded(value: &mut Value) {
    match value {
        Value::Object(members) => {
            let worded = members.get("error").is_some_and(Value::is_string);
            if members.contains_key("status") && worded {
                members.insert("error".to_owned(), Value::from("<an error>"));
            }
            members.values_mut().for_each(unworded);
        }
        Value::Array(items) => items.iter_mut().for_each(unworded),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Where the failure of an image provider differs from Python's, by what
/// the case says a replay reproduces: its class (`ValueError`,
/// `ProviderError` or `ImageGenerationError`), the attributes of a
/// provider's failure, and a failed generation's attempts and images.
fn failure_differs(
    case: &Case,
    failure: &Failure,
    error: &ImageError,
    outcomes: &[(Value, Value); 2],
    found: &mut Vec<String>,
) {
    let Some(compare) = &case.compare else {
        return found.push("a failure with no rule to compare it".to_owned());
    };
    if compare.exactly.is_empty() {
        return found.push(format!("a {} no provider reproduces", failure.class));
    }
    let (class, provider, message) = match error {
        ImageError::Value(refused) => ("ValueError", None, refused.to_string()),
        ImageError::Provider(error) if generation_of(error).is_some() => {
            ("ImageGenerationError", Some(error), error.message.clone())
        }
        ImageError::Provider(error) => ("ProviderError", Some(error), error.message.clone()),
    };
    for attribute in &compare.exactly {
        let same = match (attribute.as_str(), provider) {
            ("class", _) => failure.class == class,
            ("status_code", Some(error)) => failure.status_code == error.status_code,
            ("retryable", Some(error)) => failure.retryable == Some(error.retryable),
            ("provider", Some(error)) => {
                failure.provider.as_deref() == Some(error.provider.as_str())
            }
            ("context_overflow", Some(error)) => failure.context_overflow == error.context_overflow,
            ("before_request", Some(error)) => failure.before_request == Some(error.before_request),
            ("attempts", _) => outcomes[0].0 == outcomes[0].1,
            ("results", _) => outcomes[1].0 == outcomes[1].1,
            (_, None) => false,
            (other, Some(_)) => panic!("{}: {other} is not compared", case.name),
        };
        if !same {
            found.push(format!("the failure's {attribute} differs: {error:?}"));
        }
    }
    if message_compared(case) && message != failure.message {
        found.push(format!("the failure says {message:?}"));
    }
}

/// Replays the image case `name` of `fixture`, requires that it is as
/// Python, then requires the replay to refuse each of `wrongs`, with its
/// word in what is said: the self-test of what an image replay compares.
pub async fn refuses<P: Replays + ImageProvider>(fixture: &Fixture, name: &str, wrongs: &[Wrong]) {
    let case = fixture.case(name);
    let door = Door::open().await;
    for (what, word, wrong) in wrongs {
        let mut replayed = replay::<P>(fixture, case, &door).await;
        assert_eq!(differences(case, &replayed), Vec::<String>::new(), "{name}");
        wrong(&mut replayed);
        let found = differences(case, &replayed);
        let said = found.iter().any(|difference| difference.contains(word));
        assert!(said, "{name}, {what}: {found:?}");
    }
}

/// Replays the image case `name` of `fixture`, requires that it is as
/// Python, then that it stays so after each of `changes`: what an image
/// replay does not compare.
pub async fn allows<P: Replays + ImageProvider>(fixture: &Fixture, name: &str, changes: &[Wrong]) {
    let case = fixture.case(name);
    let door = Door::open().await;
    for (what, _, change) in changes {
        let mut replayed = replay::<P>(fixture, case, &door).await;
        assert_eq!(differences(case, &replayed), Vec::<String>::new(), "{name}");
        change(&mut replayed);
        assert_eq!(
            differences(case, &replayed),
            Vec::<String>::new(),
            "{name}, {what}"
        );
    }
}

/// The images and the attempts a replayed image case gave.
fn given_mut(
    replayed: &mut Replayed,
) -> (
    &mut Result<Vec<ImageResult>, ImageError>,
    &mut Vec<ImageAttempt>,
) {
    match replayed.outcome.as_mut().unwrap() {
        Outcome::Images(given, told) => (given, told),
        other @ (Outcome::Response(_)
        | Outcome::Events(..)
        | Outcome::Text(..)
        | Outcome::Models(_)) => panic!("no images: {other:?}"),
    }
}

/// The images a replayed image case gave.
fn images_mut(replayed: &mut Replayed) -> &mut Vec<ImageResult> {
    match given_mut(replayed).0 {
        Ok(images) => images,
        Err(error) => panic!("no images: {error:?}"),
    }
}

/// The failed generation a replayed image case ended with, changed by
/// `change`.
fn generation_changed(replayed: &mut Replayed, change: fn(&mut ImageGenerationError)) {
    let (given, _) = given_mut(replayed);
    let Err(ImageError::Provider(error)) = given else {
        panic!("no generation failed: {given:?}");
    };
    let mut generation = generation_of(error).cloned().expect("a generation failed");
    change(&mut generation);
    *error = generation.into();
}

/// Images that are not Python's: another image, one less, another usage.
pub const IMAGES: [Wrong; 3] = [
    ("another image", "the images", |r| {
        "data:image/png;base64,QUJD".clone_into(&mut images_mut(r)[0].data);
    }),
    ("an image less", "the images", |r| {
        images_mut(r).pop();
    }),
    ("another usage", "the images", |r| {
        images_mut(r)[0]
            .usage
            .insert("output_tokens".to_owned(), 1.into());
    }),
];

/// Attempts told that are not Python's: one less, another status.
pub const TOLD: [Wrong; 3] = [
    ("an attempt told less", "the attempts told", |r| {
        given_mut(r).1.pop();
    }),
    ("another status told", "the attempts told", |r| {
        given_mut(r).1[0].status = ImageAttemptStatus::Unknown;
    }),
    (
        "an attempt's moves in another order",
        "the attempts told",
        |r| {
            let told = given_mut(r).1;
            let first = told.iter().position(|attempt| attempt.index == 0).unwrap();
            let last = told.iter().rposition(|attempt| attempt.index == 0).unwrap();
            told.swap(first, last);
        },
    ),
];

/// What a replay does not compare: the attempts told of attempts made at
/// once, each attempt's moves together. Attempts made one after another
/// replay as attempts made at once.
pub const INTERLEAVED: [Wrong; 1] = [("each attempt's moves together", "", |r| {
    given_mut(r).1.sort_by_key(|attempt| attempt.index);
})];

/// A failed generation that is not Python's: another status, its attempt
/// standing otherwise, an attempt more, an attempt that says no error, an
/// image more.
pub const GENERATION: [Wrong; 5] = [
    (
        "an attempt that says no error",
        "the failure's attempts",
        |r| {
            generation_changed(r, |generation| generation.attempts[0].error = None);
        },
    ),
    ("another status", "status_code", |r| {
        generation_changed(r, |generation| {
            generation.attempts[0].status_code = Some(500);
        });
    }),
    (
        "an attempt standing otherwise",
        "the failure's attempts",
        |r| {
            generation_changed(r, |generation| {
                generation.attempts[0].status = ImageAttemptStatus::Unknown;
            });
        },
    ),
    ("an attempt more", "the failure's attempts", |r| {
        generation_changed(r, |generation| {
            generation.attempts.push(ImageAttempt::default());
        });
    }),
    ("an image more", "the failure's results", |r| {
        generation_changed(r, |generation| {
            let image = ImageResult::new("data:image/png;base64,QUJD", "image/png").unwrap();
            generation.results.push(image);
        });
    }),
];

/// A refusal before any request that is not Python's: another class,
/// other words.
pub const REFUSAL: [Wrong; 2] = [
    ("another class", "the failure's class", |r| {
        let failure = ProviderError::new("no", roomkit_ai::ProviderErrorOptions::default());
        *given_mut(r).0 = Err(ImageError::Provider(failure));
    }),
    ("other words", "the failure says", |r| {
        let refused = roomkit_ai::image::ImageRequestError::new("other words");
        *given_mut(r).0 = Err(ImageError::Value(refused));
    }),
];
