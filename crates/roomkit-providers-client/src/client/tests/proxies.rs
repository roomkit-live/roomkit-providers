//! A client behind a proxy: what the proxy answers for itself, what a
//! proxy that is not there reads as, and what no failure and no `Debug`
//! form repeats of it. Each test gives its client the proxies an
//! environment would name, and reads no environment.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use reqwest::header::HeaderMap;
use roomkit_providers_llm::ProviderError;
use serde_json::Value;

use crate::failure::{Stated, past_mark};
use crate::harness::fake::{Body, Fake, Reply};
use crate::proxy::Proxies;
use crate::testing::{left, no_authority, settings, usual};
use crate::{Call, Client, Exit, Failure, Pace};

/// The path a test asks.
const PATH: &str = "/v1/chat/completions";

/// A proxy's credentials: placeholders.
const CREDENTIALS: &str = "user:placeholder-not-a-password";

/// What a client writes of [`CREDENTIALS`] for a proxy.
const BASIC: &str = "Basic dXNlcjpwbGFjZWhvbGRlci1ub3QtYS1wYXNzd29yZA==";

/// A client that sends everything through the proxy at `proxy`, with
/// [`CREDENTIALS`], tries a failed request `max_retries` times again and
/// takes no wait between two tries: each is counted in what is returned
/// beside it.
fn behind(proxy: &str, max_retries: u32) -> (Client, Arc<AtomicU32>) {
    let named = format!("http://{CREDENTIALS}@{proxy}");
    let proxies = Proxies::read([("ALL_PROXY".to_owned(), named)]).unwrap();
    let settings = settings(usual(), max_retries);
    let mut client = Client::built(settings, proxies, no_authority()).unwrap();
    let waits = Arc::new(AtomicU32::new(0));
    let counted = Arc::clone(&waits);
    client.set_pace(Pace {
        now: Arc::new(|| 0.0),
        wait: Arc::new(move |_| {
            counted.fetch_add(1, Ordering::Relaxed);
            Box::pin(std::future::ready(()))
        }),
    });
    (client, waits)
}

/// What asking `url` through `client` fails with, once it left.
async fn failed(client: &Client, url: &str) -> ProviderError {
    let (url, headers) = (reqwest::Url::parse(url).unwrap(), HeaderMap::new());
    let call = Call {
        url: &url,
        headers: &headers,
        body: Some(b"{}"),
    };
    left(&client.send(call).await.unwrap_err())
}

/// A proxy's own answer of HTTP `status`, a page.
fn page(status: u16) -> Reply {
    Reply::Answer {
        status,
        headers: vec![("content-type".to_owned(), "text/html".to_owned())],
        body: Body::Whole(b"<html>the proxy says no</html>".to_vec()),
    }
}

/// What a failure says, whether a new request may succeed, and its status.
fn read(error: &ProviderError) -> (&str, bool, Option<u16>) {
    (error.message.as_str(), error.retryable, error.status_code)
}

// Python: OpenAIAIProvider behind a proxy that answers an `http` request
// itself: its status is the failure's, as the endpoint's would be. A 407 is
// final; a 502 is tried again, and worth a new request.
#[tokio::test]
async fn what_a_proxy_answers_a_request_handed_to_it_reads_as_the_endpoint_s_answer() {
    let url = format!("http://llm.example{PATH}");
    let proxy = Fake::serving(vec![page(407)]).await;
    let (client, waits) = behind(&proxy.address.to_string(), 2);
    let refused = failed(&client, &url).await;
    let said = "the provider answered HTTP 407: Proxy Authentication Required";
    assert_eq!(read(&refused), (said, false, Some(407)));
    assert_eq!((proxy.seen().len(), waits.load(Ordering::Relaxed)), (1, 0));
    assert_eq!(proxy.seen()[0].target, url);
    assert_eq!(proxy.seen()[0].headers["proxy-authorization"], BASIC);

    let proxy = Fake::serving(vec![page(502), page(502), page(502)]).await;
    let (client, waits) = behind(&proxy.address.to_string(), 2);
    let refused = failed(&client, &url).await;
    let said = "the provider answered HTTP 502: Bad Gateway";
    assert_eq!(read(&refused), (said, true, Some(502)));
    assert_eq!((proxy.seen().len(), waits.load(Ordering::Relaxed)), (3, 2));
}

// Python: OpenAIAIProvider behind a proxy that refuses the tunnel of an
// `https` request: no answer of the endpoint came, so the failure is a
// connection that failed, with no status, tried again and worth a new
// request, whatever the proxy's status was.
#[tokio::test]
async fn a_tunnel_a_proxy_refuses_is_a_connection_that_failed() {
    let url = format!("https://llm.example{PATH}");
    for status in [407, 502, 302] {
        let proxy = Fake::serving(vec![page(status), page(status), page(status)]).await;
        let (client, waits) = behind(&proxy.address.to_string(), 2);
        let refused = failed(&client, &url).await;
        let said = "the connection to the provider failed";
        assert_eq!(read(&refused), (said, true, None), "{status}");
        let asked = proxy.seen();
        assert_eq!((asked.len(), waits.load(Ordering::Relaxed)), (3, 2));
        let tunnel = (asked[0].method.as_str(), asked[0].target.as_str());
        assert_eq!(tunnel, ("CONNECT", "llm.example:443"));
        assert_eq!(asked[0].headers["proxy-authorization"], BASIC);
    }
}

// A redirect is not followed, a proxy's own no more than an endpoint's:
// the request fails, for good, and nothing is asked of where it points.
#[tokio::test]
async fn a_proxy_s_redirect_is_not_followed() {
    let elsewhere = "http://elsewhere.example/".to_owned();
    let redirect = Reply::Answer {
        status: 302,
        headers: vec![("location".to_owned(), elsewhere)],
        body: Body::Whole(Vec::new()),
    };
    let proxy = Fake::serving(vec![redirect]).await;
    let (client, waits) = behind(&proxy.address.to_string(), 2);
    let refused = failed(&client, &format!("http://llm.example{PATH}")).await;
    let said = "the endpoint answered with a redirect (302), which is not followed";
    assert_eq!(read(&refused), (said, false, Some(302)));
    assert_eq!((proxy.seen().len(), waits.load(Ordering::Relaxed)), (1, 0));
}

// A proxy nothing listens at is a connection that failed, for an `http`
// endpoint as for an `https` one: tried again, and worth a new request.
// The endpoint is there, and is not reached around the proxy.
#[tokio::test]
async fn a_proxy_that_is_not_there_is_a_connection_that_failed() {
    // A port that was open a moment ago, and that nothing listens on now.
    let closed = Fake::serving(Vec::new()).await.address.to_string();
    let endpoint = Fake::serving(vec![Reply::json(200, "{}")]).await;
    let urls = [
        format!("http://{}{PATH}", endpoint.address),
        format!("https://llm.example{PATH}"),
    ];
    for url in urls {
        let (client, waits) = behind(&closed, 1);
        let refused = failed(&client, &url).await;
        let said = "the connection to the provider failed";
        assert_eq!(read(&refused), (said, true, None), "{url}");
        assert_eq!(waits.load(Ordering::Relaxed), 1);
    }
    assert!(endpoint.seen().is_empty());
}

/// A refusal that repeats what its body says under `error.message`, as a
/// provider reads an error object.
fn echoing(status: u16, body: &[u8]) -> Failure {
    let parsed: Option<Value> = serde_json::from_slice(past_mark(body)).ok();
    let message = |body: &Value| Some(body.get("error")?.get("message")?.as_str()?.to_owned());
    let stated = Stated {
        message: parsed.as_ref().and_then(message),
        ..Stated::default()
    };
    Failure::refused(status, body, stated)
}

// A proxy's credentials are a secret of the process: a failure repeats
// none of them, in any form a proxy may send them back in, and neither
// does the `Debug` form of the client or the error of the HTTP client
// under it, for a proxy that refuses as for one that is not there.
#[tokio::test]
async fn a_proxy_s_credentials_appear_in_no_failure_and_no_debug_form() {
    let secret = "placeholder-not-a-password";
    let said = format!("no way in for {CREDENTIALS}, sent as {BASIC}, password {secret}");
    let body = serde_json::json!({"error": {"message": said}}).to_string();
    let proxy = Fake::serving(vec![Reply::json(407, body), page(407)]).await;
    let (mut client, _) = behind(&proxy.address.to_string(), 0);
    client.refused = echoing;
    let url = reqwest::Url::parse(&format!("http://llm.example{PATH}")).unwrap();
    let no_headers: [&str; 0] = [];
    let exit = Exit::new("test", client.secrets([], no_headers, &url));
    let headers = HeaderMap::new();
    let call = Call {
        url: &url,
        headers: &headers,
        body: Some(b"{}"),
    };
    let refused = exit.leave(&client.send(call).await.unwrap_err());
    assert_eq!(
        refused.message,
        "the provider answered HTTP 407: no way in for [***], sent as [***], password [***]"
    );
    let mut shown = vec![format!("{client:?} {exit:?}")];
    // What the HTTP client itself says of a tunnel the proxy refused, and
    // of a proxy that is not there, with every cause under it.
    let closed = Fake::serving(Vec::new()).await.address.to_string();
    let (unreachable, _) = behind(&closed, 0);
    for client in [&client, &unreachable] {
        let secure = format!("https://llm.example{PATH}");
        let error = client.http.post(secure).send().await.unwrap_err();
        let mut cause: Option<&dyn std::error::Error> = Some(&error);
        while let Some(found) = cause {
            shown.push(format!("{found} {found:?}"));
            cause = found.source();
        }
    }
    let shown = shown.join(" ");
    assert!(
        shown.contains("Proxies { rules: 1, proxies: 1 }"),
        "{shown}"
    );
    for hidden in [
        secret,
        "dXNlcjpwbGFjZWhvbGRlci1ub3QtYS1wYXNzd29yZA",
        "user:",
    ] {
        assert!(!shown.contains(hidden), "{hidden} in {shown}");
    }
}
