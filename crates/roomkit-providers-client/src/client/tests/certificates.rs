//! A server whose certificate the client refuses: what the failure says,
//! and how it is typed. Each test gives its client the authority it
//! trusts, made when the test runs, and reads no environment.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use reqwest::header::HeaderMap;
use roomkit_providers_http::TlsUnset;

use crate::client::without_tls;
use crate::harness::fake::{Fake, Reply};
use crate::harness::tls::Authority;
use crate::proxy::Proxies;
use crate::testing::{left, settings, usual};
use crate::{Call, Client, Pace};

/// The path a test asks.
const PATH: &str = "/v1/chat/completions";

// Python: OpenAIAIProvider before a server whose certificate its client
// refuses: a connection that failed, tried again and worth a new request.
// The failure says here what was refused of the certificate.
#[tokio::test]
async fn a_refused_certificate_is_a_connection_that_failed_and_says_why() {
    let (ours, theirs) = (Authority::new(), Authority::new());
    let untrusted = "the provider's certificate is not trusted";
    let other_name = "the provider's certificate is not for its host name";
    let cases = [
        (&theirs, "127.0.0.1", untrusted),
        (&ours, "other.example", other_name),
    ];
    for (signed_by, name, said) in cases {
        let tls = signed_by.serving(&[name]);
        let server = Fake::serving_tls(vec![Reply::json(200, "{}")], tls).await;
        let settings = settings(usual(), 2);
        let mut client = Client::built(settings, Proxies::default(), ours.trusting()).unwrap();
        let waits = Arc::new(AtomicU32::new(0));
        let counted = Arc::clone(&waits);
        client.set_pace(Pace {
            now: Arc::new(|| 0.0),
            wait: Arc::new(move |_| {
                counted.fetch_add(1, Ordering::Relaxed);
                Box::pin(std::future::ready(()))
            }),
        });
        let url = format!("https://{}{PATH}", server.address);
        let (url, headers) = (reqwest::Url::parse(&url).unwrap(), HeaderMap::new());
        let call = Call {
            url: &url,
            headers: &headers,
            body: Some(b"{}"),
        };
        let refused = left(&client.send(call).await.unwrap_err());
        let read = (
            refused.message.as_str(),
            refused.retryable,
            refused.status_code,
        );
        assert_eq!(read, (said, true, None));
        assert_eq!(waits.load(Ordering::Relaxed), 2);
        // The server saw a client leave its greeting, and no request.
        server.turned_away().await;
        assert!(server.seen().is_empty());
    }
}

// A server the client's authority signed for the name it is asked under is
// reached.
#[tokio::test]
async fn a_server_the_client_s_authority_signed_is_reached() {
    let authority = Authority::new();
    let tls = authority.serving(&["127.0.0.1"]);
    let server = Fake::serving_tls(vec![Reply::json(200, "trusted")], tls).await;
    let settings = settings(usual(), 0);
    let client = Client::built(settings, Proxies::default(), authority.trusting()).unwrap();
    let url = format!("https://{}{PATH}", server.address);
    let (url, headers) = (reqwest::Url::parse(&url).unwrap(), HeaderMap::new());
    let call = Call {
        url: &url,
        headers: &headers,
        body: Some(b"{}"),
    };
    let (body, json) = client.send(call).await.unwrap();
    assert_eq!((body.as_slice(), json), (&b"trusted"[..], true));
    assert_eq!(server.seen()[0].target, PATH);
}

// A provider is not built on a system whose certificate store is there and
// does not read: the failure is one of before any request, final, and names
// the store by its path. The public roots compiled in are for a system that
// keeps no store, and stand in for no other.
#[test]
fn a_system_store_that_does_not_read_is_a_provider_that_cannot_be_built() {
    let store = PathBuf::from("/etc/ssl/certs/ca-certificates.crt");
    let refused = left(&without_tls(&TlsUnset::SystemUnread(store)));
    assert_eq!(
        refused.message,
        "the provider cannot be built: the system's certificate store at \
         /etc/ssl/certs/ca-certificates.crt cannot be read"
    );
    assert!(refused.before_request && !refused.retryable);
    // A file the environment names is refused in the same words as before.
    assert_eq!(
        left(&without_tls(&TlsUnset::FileEmpty)).message,
        "the provider cannot be built: the file SSL_CERT_FILE names holds no certificate"
    );
}
