//! A streamed answer read through the client: the data of its events as
//! its bytes arrive, what it is when it is no stream, and what is given
//! before it fails.

use crate::items::{Yielder, items, next};
use reqwest::header::HeaderMap;
use roomkit_ai::{
    ProviderError, ProviderErrorOptions, ProviderStream, StreamDone, StreamEvent, StreamTextDelta,
};
use serde_json::json;

use crate::harness::fake::{Body, Ending, Fake, Reply};
use crate::stream::{Item, Streamed, text};
use crate::testing::{client, left, url_on, usual};
use crate::{ANSWER_LIMIT, Call};

/// An answer of HTTP 200 the server says is `kind`, sent in `reads`, each
/// one read, which end as `ending` says.
fn answer(kind: &str, reads: &[&str], ending: Ending) -> Reply {
    let reads = reads.iter().map(|read| read.as_bytes().to_vec());
    Reply::Answer {
        status: 200,
        headers: vec![("content-type".to_owned(), kind.to_owned())],
        body: Body::Reads(reads.collect(), ending),
    }
}

/// The streamed answer `client` gets of `fake`.
async fn streamed(client: &crate::Client, fake: &Fake) -> Streamed {
    let (url, headers) = (url_on(fake, "/v1/stream"), HeaderMap::new());
    let call = Call {
        url: &url,
        headers: &headers,
        body: Some(b"{}"),
        token: None,
    };
    client.stream(call).await.unwrap()
}

/// The data of every event of `answer`, and the words of the failure that
/// ended it.
async fn read(mut answer: Streamed) -> (Vec<String>, Option<String>) {
    let mut data = Vec::new();
    loop {
        match answer.next().await {
            Ok(Some(event)) => data.push(event),
            Ok(None) => return (data, None),
            Err(failure) => return (data, Some(left(&failure).message)),
        }
    }
}

// The events come in order however the reads cut them, one the answer ends
// on with no blank line after it included. An answer that gave no event
// and that the server says is JSON is the document it holds, for its
// reader to read as a whole answer; any other is none.
#[tokio::test]
async fn a_streamed_answer_gives_its_events_or_the_document_it_is() {
    const EVENTS: &str = "text/event-stream";
    const JSON: &str = "application/json";
    let object = json!({"error": {"message": "Overloaded."}}).to_string();
    let cut = ["data: one\n\nda", "ta: two\n\n: a comment\n\ndata: three"];
    let replies = vec![
        answer(EVENTS, &cut, Ending::Ends),
        answer(JSON, &[&object], Ending::Ends),
        answer(EVENTS, &[&object], Ending::Ends),
        answer(JSON, &["<html>Sign in</html>"], Ending::Ends),
    ];
    let fake = Fake::serving(replies).await;
    let client = client(usual(), 0);
    let mut events = streamed(&client, &fake).await;
    let mut data = Vec::new();
    while let Some(event) = events.next().await.unwrap() {
        data.push(event);
    }
    assert_eq!(data, ["one", "two", "three"]);
    assert_eq!(events.next().await.unwrap(), None);
    assert_eq!(events.unstreamed(), None);
    let documents = [
        Some(json!({"error": {"message": "Overloaded."}})),
        None,
        None,
    ];
    for document in documents {
        let mut unstreamed = streamed(&client, &fake).await;
        assert_eq!(unstreamed.next().await.unwrap(), None);
        assert_eq!(unstreamed.unstreamed(), document);
    }
}

// An answer that ends inside an event was cut there: the event is given
// with what came of its data, and the answer says so of that event alone,
// when its data does not read as a JSON document. An event whose data
// reads came whole, whatever follows its last line: a blank line, a line
// end or nothing.
#[tokio::test]
async fn an_answer_says_of_its_last_event_whether_it_was_cut_inside_it() {
    const EVENTS: &str = "text/event-stream";
    let whole = r#"{"said": "two"}"#;
    let endings = [
        (format!("data: {whole}\n\n"), whole, false),
        (format!("data: {whole}\n"), whole, false),
        (format!("data: {whole}"), whole, false),
        (r#"data: {"said": "tw"#.to_owned(), r#"{"said": "tw"#, true),
        ("data: {\"said\":\n".to_owned(), r#"{"said":"#, true),
        (
            "data: {\"said\":\ndata: \"two\"}".to_owned(),
            "{\"said\":\n\"two\"}",
            false,
        ),
        ("data: [DONE]".to_owned(), "[DONE]", true),
        ("data:".to_owned(), "", true),
        ("data: caf\u{e9}".to_owned(), "caf\u{e9}", true),
    ];
    let first = r#"data: {"said": "one"}"#;
    let replies = endings.iter().map(|(last, _, _)| {
        let body = format!("{first}\n\n{last}");
        answer(EVENTS, &[&body], Ending::Ends)
    });
    let fake = Fake::serving(replies.collect()).await;
    let client = client(usual(), 0);
    for (last, data, cut) in &endings {
        let mut events = streamed(&client, &fake).await;
        assert!(!events.was_cut(), "{last:?}: before any event");
        let one = events.next().await.unwrap();
        assert_eq!(one.as_deref(), Some(r#"{"said": "one"}"#), "{last:?}");
        assert!(!events.was_cut(), "{last:?}: an event its blank line ended");
        assert_eq!(events.next().await.unwrap().as_deref(), Some(*data));
        assert_eq!(events.was_cut(), *cut, "{last:?}");
        assert_eq!(events.next().await.unwrap(), None, "{last:?}");
    }
    // A character the answer's end cuts is left out of the event it cut.
    let fake = Fake::serving(vec![Reply::Answer {
        status: 200,
        headers: vec![("content-type".to_owned(), EVENTS.to_owned())],
        body: Body::Reads(vec![b"data: {\"said\": \"caf\xc3".to_vec()], Ending::Ends),
    }])
    .await;
    let mut events = streamed(&client, &fake).await;
    let last = events.next().await.unwrap();
    assert_eq!(last.as_deref(), Some(r#"{"said": "caf"#));
    assert!(events.was_cut());
}

// What was read before a stream fails is given first: the events ended
// before the bound of a stream was passed, and those that came before the
// connection was lost.
#[tokio::test]
async fn what_a_stream_gave_before_it_failed_is_given_first() {
    const EVENTS: &str = "text/event-stream";
    let long = format!("data: one\n\ndata: two\n\n{}", ": padding\n".repeat(64));
    let replies = vec![
        answer(EVENTS, &[&long], Ending::Ends),
        answer(EVENTS, &["data: one\n\n", "data: tw"], Ending::Lost),
    ];
    let fake = Fake::serving(replies).await;
    let mut client = client(usual(), 0);
    client.set_stream_limit(32);
    let bounded = streamed(&client, &fake).await;
    let said = "the provider's answer is longer than 32 bytes";
    let given = vec!["one".to_owned(), "two".to_owned()];
    assert_eq!(read(bounded).await, (given, Some(said.to_owned())));
    let mut client = client;
    client.set_stream_limit(crate::stream::STREAM_LIMIT);
    let lost = streamed(&client, &fake).await;
    let said = "the connection to the provider failed";
    assert_eq!(
        read(lost).await,
        (vec!["one".to_owned()], Some(said.to_owned()))
    );
}

// A stream leaves with the request of its generation: the failure it
// carried leaves without the key that request brings, the events before it
// as they were, and nothing after it. So does a failure the contract's
// check built on the way. The stream of a request that brings no key
// leaves as it is.
#[tokio::test]
async fn a_stream_leaves_with_the_request_of_its_generation() {
    use crate::items::{Yielder, items, next};
    use roomkit_ai::{
        API_KEY_METADATA_KEY, AiContext, ProviderError, ProviderErrorOptions, StreamEvent,
        StreamTextDelta,
    };

    use crate::failure::{Exit, Failure, Secrets, carried, told};
    use crate::stream::{Item, leaving};

    const LENT: &str = "placeholder-of-one-request";
    let said = || StreamEvent::TextDelta(StreamTextDelta::new("Hel"));
    let events = |failure: ProviderError| {
        Box::pin(items(move |out: Yielder<Item>| async move {
            out.give(Ok(said())).await;
            out.give(Err(failure)).await;
            out.give(Ok(said())).await;
        }))
    };
    let echoed = told(format!("Wrong key: {LENT}."), 500);
    let refused = carried(Failure::said(vec![echoed], false, Some(401)));
    let quoting = format!("The answer quotes {LENT}.");
    let checked = ProviderError::new(quoting, ProviderErrorOptions::default());
    let mut lending = AiContext::default();
    let key = (API_KEY_METADATA_KEY.to_owned(), LENT.into());
    lending.metadata.extend([key]);
    let exit = Exit::new("test", Secrets::default());
    let streams = [
        (refused.clone(), &lending, "Wrong key: [***].", Some(401)),
        (checked, &lending, "The answer quotes [***].", None),
        (
            refused,
            &AiContext::default(),
            "Wrong key: placeholder-of-one-request.",
            Some(401),
        ),
    ];
    for (failure, request, message, status) in streams {
        let mut leaving = leaving(events(failure), &exit, request);
        assert_eq!(next(&mut leaving).await.unwrap().unwrap(), said());
        let error = next(&mut leaving).await.unwrap().unwrap_err();
        assert_eq!(
            (error.message.as_str(), error.status_code),
            (message, status)
        );
        assert!(next(&mut leaving).await.is_none());
    }
}

// An answer the server says is JSON and that gave no event is the document
// it holds, past the byte order mark it may start with, within what an
// answer read whole may weigh: a byte more, and it is none.
#[tokio::test]
async fn a_document_in_place_of_a_stream_is_kept_within_the_bound_of_a_whole_answer() {
    const JSON: &str = "application/json";
    // An empty list of `length` bytes, on lines a stream's reader takes.
    let document = |length: usize| format!("[{}]", "\n".repeat(length - 2));
    let replies = vec![
        answer(JSON, &[&document(ANSWER_LIMIT)], Ending::Ends),
        answer(JSON, &[&document(ANSWER_LIMIT + 1)], Ending::Ends),
        answer(JSON, &["\u{feff}{\"error\": {}}"], Ending::Ends),
    ];
    let fake = Fake::serving(replies).await;
    let client = client(usual(), 0);
    let mut documents = Vec::new();
    for _ in 0..3 {
        let mut answer = streamed(&client, &fake).await;
        assert_eq!(answer.next().await.unwrap(), None);
        documents.push(answer.unstreamed());
    }
    assert_eq!(
        documents,
        [Some(json!([])), None, Some(json!({"error": {}}))]
    );
}

// A stream that failed gives nothing more: its failure once, then its end.
#[tokio::test]
async fn a_stream_that_failed_gives_nothing_more() {
    let long = format!("data: one\n\n{}", ": padding\n".repeat(64));
    let reply = answer("text/event-stream", &[&long], Ending::Ends);
    let fake = Fake::serving(vec![reply]).await;
    let mut client = client(usual(), 0);
    client.set_stream_limit(32);
    let mut bounded = streamed(&client, &fake).await;
    assert_eq!(bounded.next().await.unwrap().as_deref(), Some("one"));
    assert!(bounded.next().await.is_err());
    assert_eq!(bounded.next().await.unwrap(), None);
}

// The text of a stream of events is its text alone, and ends at its
// failure: what a stream gives after one is not handed on.
#[tokio::test]
async fn the_text_of_a_stream_ends_at_its_failure() {
    let delta = |text: &str| Ok(StreamEvent::TextDelta(StreamTextDelta::new(text)));
    let failed = Err(ProviderError::new(
        "failed",
        ProviderErrorOptions::default(),
    ));
    let given = [
        delta("one"),
        Ok(StreamEvent::Done(StreamDone::default())),
        failed,
        delta("two"),
    ];
    let events: ProviderStream<'_> = Box::pin(items(move |out: Yielder<Item>| async move {
        for item in given {
            out.give(item).await;
        }
    }));
    let mut said = text(events);
    let mut handed = Vec::new();
    while let Some(item) = next(&mut said).await {
        handed.push(item.map_err(|error| error.message));
    }
    assert_eq!(handed, [Ok("one".to_owned()), Err("failed".to_owned())]);
}

// The `Debug` form of a streamed answer says how far it was read, and
// nothing of what it said.
#[tokio::test]
async fn the_debug_form_of_a_streamed_answer_says_how_far_it_was_read() {
    let said = "data: placeholder-of-an-answer\n\n";
    let reply = answer("text/event-stream", &[said], Ending::Ends);
    let fake = Fake::serving(vec![reply]).await;
    let mut answer = streamed(&client(usual(), 0), &fake).await;
    let mut shown = vec![format!("{answer:?}")];
    assert!(answer.next().await.unwrap().is_some());
    shown.push(format!("{answer:?}"));
    assert_eq!(answer.next().await.unwrap(), None);
    shown.push(format!("{answer:?}"));
    assert_eq!(
        shown,
        [
            "Streamed { given: false, ended: false, .. }",
            "Streamed { given: true, ended: false, .. }",
            "Streamed { given: true, ended: true, .. }",
        ]
    );
}
