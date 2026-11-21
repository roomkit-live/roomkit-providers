//! A streamed answer read through the client: the data of its events as
//! its bytes arrive, what it is when it is no stream, and what is given
//! before it fails.

use reqwest::header::HeaderMap;
use serde_json::json;

use crate::Call;
use crate::harness::fake::{Body, Ending, Fake, Reply};
use crate::stream::Streamed;
use crate::testing::{client, left, url_on, usual};

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
