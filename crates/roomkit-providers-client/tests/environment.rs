//! What a client reads of the environment of its process, when it is built:
//! the proxies it sends through, and the authorities it checks a server's
//! certificate against.
//!
//! A test cannot set a variable of its own process, so each test here runs
//! this test binary again as a child, in the environment the test gives it.
//! The child builds a client there, asks what it is told to ask, and writes
//! what came of it; the test reads that, and what its fakes observed. The
//! fakes and the child are on this machine: nothing leaves it.
//!
//! The fakes are the harness's: without the `harness` feature this file
//! holds no test.

#![cfg(feature = "harness")]
#![expect(
    clippy::unwrap_used,
    reason = "what the tests share fails a test by panicking, as a test does"
)]

use std::env;
use std::ffi::OsString;
use std::io::Write as _;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use reqwest::header::HeaderMap;
use roomkit_providers_client::failure::Stated;
use roomkit_providers_client::harness::fake::{Fake, Reply};
use roomkit_providers_client::harness::tls::Authority;
use roomkit_providers_client::retry::{Past, Rule};
use roomkit_providers_client::{Call, Client, Exit, Failure, Settings, Timeouts};
use roomkit_providers_llm::ProviderError;
use serde_json::{Value, json};

/// The test a child runs.
const CHILD: &str = "a_child_asks_what_its_parent_tells_it";

/// The variable that holds the URLs a child asks, between spaces.
const ASKS: &str = "ROOMKIT_PROVIDERS_TEST_ASKS";

/// The variable that has a child point its client at a local fake before
/// it asks, and says through which seam: `test` or `replay`, a space, then
/// the address of the fake.
const SEAM: &str = "ROOMKIT_PROVIDERS_TEST_SEAM";

/// What starts a line a child writes for its parent.
const MARK: &str = "came-of-it\t";

/// The path every test asks.
const PATH: &str = "/v1/chat/completions";

/// A proxy's credentials: placeholders.
const CREDENTIALS: &str = "user:placeholder-not-a-password";

/// What a client writes of [`CREDENTIALS`] for a proxy.
const BASIC: &str = "Basic dXNlcjpwbGFjZWhvbGRlci1ub3QtYS1wYXNzd29yZA==";

/// What a test's client states: no try again, and a refusal worded from
/// its status alone.
fn settings() -> Settings {
    Settings {
        timeouts: Timeouts::new(Duration::from_secs(30), Duration::from_secs(5)).unwrap(),
        max_retries: 0,
        retry: Rule {
            longest_asked: 120.0,
            past_it: Past::Final,
        },
        refused: |status, body| Failure::refused(status, body, Stated::default()),
        tried_again: roomkit_providers_client::tried_again!("roomkit::providers::test"),
    }
}

/// What a failure reads as once it left.
fn left(error: &ProviderError) -> Value {
    json!({
        "failed": error.message,
        "retryable": error.retryable,
        "status": error.status_code,
        "before_request": error.before_request,
    })
}

/// Points `client` at the fake `seam` names, through the seam it names: a
/// test's, or a replay's. `url`, which the client is about to ask, is sent
/// there.
fn pointed(client: &mut Client, url: &mut reqwest::Url, seam: &str) {
    let (seam, fake) = seam.split_once(' ').unwrap();
    let fake: SocketAddr = fake.parse().unwrap();
    match seam {
        "test" => assert!(client.pointed_at([url], fake)),
        #[cfg(feature = "replay")]
        "replay" => {
            roomkit_providers_client::replayed_at([url], fake).unwrap();
            client.replaying(0.0);
        }
        _ => unreachable!("a parent names one of the seams, not {seam}"),
    }
}

// Run by the other tests alone, each in the environment it gives its child:
// builds a client there, asks each URL once, and writes a line for each.
#[tokio::test]
#[ignore = "the child the other tests run, each in the environment it gives it"]
async fn a_child_asks_what_its_parent_tells_it() {
    let asks = env::var(ASKS).expect("run by another test, which says what to ask");
    let seam = env::var(SEAM).ok();
    let mut out = std::io::stdout().lock();
    let mut client = match Client::new(settings()) {
        Ok(client) => client,
        Err(failure) => {
            let unbuilt = Exit::unbuilt("test", &failure);
            writeln!(out, "{MARK}{}", left(&unbuilt)).unwrap();
            return;
        }
    };
    let (headers, no_headers) = (HeaderMap::new(), [""; 0]);
    for url in asks.split(' ').filter(|url| !url.is_empty()) {
        let mut url = reqwest::Url::parse(url).unwrap();
        if let Some(seam) = &seam {
            pointed(&mut client, &mut url, seam);
        }
        let call = Call {
            url: &url,
            headers: &headers,
            body: Some(b"{}"),
        };
        let exit = client.exit("test", [], no_headers, &url);
        let came = match client.send(call).await {
            Ok((body, _)) => json!({"answered": String::from_utf8_lossy(&body)}),
            Err(failure) => left(&exit.leave(&failure)),
        };
        writeln!(out, "{MARK}{came}").unwrap();
    }
    writeln!(out, "{client:?}").unwrap();
}

/// Whether a client reads the variable `name`.
fn is_read_by_a_client(name: &OsString) -> bool {
    let name = name.to_string_lossy().to_ascii_lowercase();
    let named = matches!(
        name.as_str(),
        "request_method" | "ssl_cert_file" | "ssl_cert_dir"
    );
    named || name.ends_with("_proxy")
}

/// What a child wrote: a value for each URL it asked, or one for a client
/// that could not be built, and everything it printed.
struct Came {
    of_each: Vec<Value>,
    printed: String,
}

/// Runs a child in the environment of this process without the variables a
/// client reads, with `variables` in their place, and has it ask `asks`.
async fn child(variables: &[(&str, &str)], asks: &[String]) -> Came {
    let mut command = Command::new(env::current_exe().unwrap());
    command.args(["--exact", CHILD, "--ignored", "--nocapture"]);
    for (name, _) in env::vars_os().filter(|(name, _)| is_read_by_a_client(name)) {
        command.env_remove(name);
    }
    command.envs(variables.iter().copied());
    command.env(ASKS, asks.join(" "));
    // The fakes answer the child while it runs: it is waited for off the
    // thread that serves them.
    let output = tokio::task::spawn_blocking(move || command.output());
    let output = output.await.unwrap().unwrap();
    let printed = [output.stdout, output.stderr].concat();
    let printed = String::from_utf8_lossy(&printed).into_owned();
    assert!(output.status.success(), "the child failed: {printed}");
    let lines = printed.lines().filter_map(|line| line.split_once(MARK));
    let of_each = lines.map(|(_, came)| serde_json::from_str(came).unwrap());
    Came {
        of_each: of_each.collect(),
        printed,
    }
}

/// A directory of a test's own, removed with it.
struct Scratch(PathBuf);

impl Scratch {
    fn new(test: &str) -> Self {
        let name = format!("roomkit-providers-client-{}-{test}", std::process::id());
        let directory = env::temp_dir().join(name);
        std::fs::create_dir_all(&directory).unwrap();
        Self(directory)
    }

    /// The path of a file of the directory that holds `content`.
    fn file(&self, name: &str, content: &str) -> String {
        let file = self.0.join(name);
        std::fs::write(&file, content).unwrap();
        file.to_str().unwrap().to_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

fn answered(said: &str) -> Value {
    json!({"answered": said})
}

/// A failed connection, in the words given: worth a retry, with no status.
fn lost(said: &str) -> Value {
    json!({"failed": said, "retryable": true, "status": null, "before_request": false})
}

fn unbuilt(reason: &str) -> Value {
    let failed = format!("the provider cannot be built: {reason}");
    json!({"failed": failed, "retryable": false, "status": null, "before_request": true})
}

// A request for an `http` endpoint is handed whole to the proxy the
// environment names, with the proxy's credentials; a host `NO_PROXY` names
// is reached directly, and is shown nothing of the proxy.
#[tokio::test]
async fn a_client_sends_through_the_proxy_its_environment_names_unless_no_proxy_exempts_the_host() {
    let proxy = Fake::serving(vec![Reply::json(200, "through the proxy")]).await;
    let endpoint = Fake::serving(vec![Reply::json(200, "directly")]).await;
    let named = format!("http://{CREDENTIALS}@{}", proxy.address);
    let variables = [("HTTP_PROXY", named.as_str()), ("NO_PROXY", "127.0.0.1")];
    let proxied = format!("http://llm.example{PATH}");
    let exempted = format!("http://{}{PATH}", endpoint.address);
    let came = child(&variables, &[proxied.clone(), exempted]).await;
    assert_eq!(
        came.of_each,
        [answered("through the proxy"), answered("directly")]
    );
    let handed = proxy.seen();
    assert_eq!(handed.len(), 1);
    assert_eq!(
        (handed[0].method.as_str(), &handed[0].target),
        ("POST", &proxied)
    );
    assert_eq!(handed[0].headers["host"], "llm.example");
    assert_eq!(handed[0].headers["proxy-authorization"], BASIC);
    let direct = endpoint.seen();
    assert_eq!(direct.len(), 1);
    assert_eq!(direct[0].target, PATH);
    assert!(!direct[0].headers.contains_key("proxy-authorization"));
    // The child printed its client: nothing of the proxy's credentials.
    assert!(came.printed.contains("Client {"), "{}", came.printed);
    assert!(!came.printed.contains("placeholder-not-a-password"));
}

// The proxies a client sends through are those it read of its environment,
// and no other: where it reads none, the HTTP client under it does not read
// the variables its own way. A lower-case name set to nothing unsets the
// upper-case one, as with Python's client, where the HTTP client's own
// reader would take the upper-case one.
#[tokio::test]
async fn a_client_that_reads_no_proxy_of_its_environment_sends_directly() {
    let proxy = Fake::serving(vec![Reply::json(200, "through the proxy")]).await;
    let endpoint = Fake::serving(vec![Reply::json(200, "directly")]).await;
    let named = format!("http://{CREDENTIALS}@{}", proxy.address);
    let variables = [("HTTP_PROXY", named.as_str()), ("http_proxy", "")];
    let direct = format!("http://{}{PATH}", endpoint.address);
    let came = child(&variables, &[direct]).await;
    assert_eq!(came.of_each, [answered("directly")]);
    assert!(proxy.seen().is_empty());
    assert_eq!(endpoint.seen().len(), 1);
    assert!(came.printed.contains("Proxies { rules: 0, proxies: 0 }"));
}

// An `https` endpoint is reached through a tunnel the proxy opens, and its
// certificate is checked against the authority `SSL_CERT_FILE` names: the
// endpoint's name is not one this machine resolves, and the proxy alone
// knows where it is.
#[tokio::test]
async fn a_client_reaches_an_https_endpoint_through_a_tunnel_and_trusts_the_authority_named() {
    let scratch = Scratch::new("tunnel");
    let authority = Authority::new();
    let tls = authority.serving(&["llm.example"]);
    let endpoint = Fake::serving_tls(vec![Reply::json(200, "through the tunnel")], tls).await;
    let proxy = Fake::serving(vec![Reply::Tunnel(endpoint.address)]).await;
    let named = format!("http://{CREDENTIALS}@{}", proxy.address);
    let file = scratch.file("authority.pem", &authority.pem());
    let variables = [("https_proxy", named.as_str()), ("SSL_CERT_FILE", &file)];
    let came = child(&variables, &[format!("https://llm.example{PATH}")]).await;
    assert_eq!(came.of_each, [answered("through the tunnel")]);
    let asked = proxy.seen();
    assert_eq!(asked.len(), 1);
    assert_eq!(
        (asked[0].method.as_str(), asked[0].target.as_str()),
        ("CONNECT", "llm.example:443")
    );
    assert_eq!(asked[0].headers["proxy-authorization"], BASIC);
    let carried = endpoint.seen();
    assert_eq!(carried.len(), 1);
    assert_eq!(
        (carried[0].method.as_str(), carried[0].target.as_str()),
        ("POST", PATH)
    );
    assert!(!carried[0].headers.contains_key("proxy-authorization"));
}

// The user information of a proxy is what stands before the last `@` of its
// URL, whatever it holds, and no part of where the proxy is: a backslash
// there, as the user of a Windows domain is written, ends no host. The
// request goes to the proxy the variable names, with the user and the
// password the client of Python's providers gives it.
#[tokio::test]
async fn a_backslash_in_a_proxy_s_user_ends_no_host() {
    let scratch = Scratch::new("backslash");
    let authority = Authority::new();
    let tls = authority.serving(&["llm.example"]);
    let endpoint = Fake::serving_tls(vec![Reply::json(200, "through the tunnel")], tls).await;
    let proxy = Fake::serving(vec![Reply::Tunnel(endpoint.address)]).await;
    let given = "CORP\\jdoe:placeholder-not-a-password";
    let named = format!("http://{given}@{}", proxy.address);
    let file = scratch.file("authority.pem", &authority.pem());
    let variables = [("HTTPS_PROXY", named.as_str()), ("SSL_CERT_FILE", &file)];
    let came = child(&variables, &[format!("https://llm.example{PATH}")]).await;
    assert_eq!(came.of_each, [answered("through the tunnel")]);
    let asked = proxy.seen();
    assert_eq!(asked.len(), 1);
    assert_eq!(
        (asked[0].method.as_str(), asked[0].target.as_str()),
        ("CONNECT", "llm.example:443")
    );
    assert_eq!(
        asked[0].basic("proxy-authorization").as_deref(),
        Some(given)
    );
    assert!(!came.printed.contains("placeholder-not-a-password"));
}

// What stands between a first `@` and a backslash names no host either: it
// is a part of the password. The listener there is sent nothing, and the
// proxy after the last `@` is handed the request.
#[tokio::test]
async fn an_address_before_a_backslash_in_a_proxy_s_password_is_sent_nothing() {
    let elsewhere = Fake::serving(vec![Reply::json(200, "elsewhere")]).await;
    let proxy = Fake::serving(vec![Reply::json(200, "through the proxy")]).await;
    let given = format!("user:pw@{}\\x", elsewhere.address);
    let named = format!("http://{given}@{}", proxy.address);
    let proxied = format!("http://llm.example{PATH}");
    let came = child(&[("HTTP_PROXY", &named)], std::slice::from_ref(&proxied)).await;
    assert_eq!(came.of_each, [answered("through the proxy")]);
    assert!(elsewhere.seen().is_empty());
    let handed = proxy.seen();
    assert_eq!(handed.len(), 1);
    assert_eq!(handed[0].target, proxied);
    assert_eq!(
        handed[0].basic("proxy-authorization").as_deref(),
        Some(given.as_str())
    );
}

/// Has a child point its client at a local fake through `seam`, in an
/// environment that names a proxy for every scheme, and ask: the fake must
/// answer, and the proxy be handed nothing.
async fn pointed_at_a_fake_through(seam: &str) {
    let proxy = Fake::serving(vec![Reply::json(200, "through the proxy")]).await;
    let fake = Fake::serving(vec![Reply::json(200, "directly")]).await;
    let named = format!("http://{CREDENTIALS}@{}", proxy.address);
    let pointed = format!("{seam} {}", fake.address);
    let variables = [
        ("HTTP_PROXY", named.as_str()),
        ("http_proxy", named.as_str()),
        ("ALL_PROXY", named.as_str()),
        (SEAM, pointed.as_str()),
    ];
    let came = child(&variables, &[format!("http://llm.example{PATH}")]).await;
    assert_eq!(came.of_each, [answered("directly")]);
    assert!(proxy.seen().is_empty());
    let direct = fake.seen();
    assert_eq!(direct.len(), 1);
    let direct = direct.first().unwrap();
    assert_eq!(direct.target, PATH);
    assert!(!direct.headers.contains_key("proxy-authorization"));
}

// A client a test points at a local fake reaches it directly: the proxy
// its environment names is handed nothing. A developer's proxy then changes
// no test of a provider, and receives none of its requests.
#[tokio::test]
async fn a_client_a_test_points_at_a_fake_goes_through_no_proxy_of_its_environment() {
    pointed_at_a_fake_through("test").await;
}

// A replay sends nothing off the machine, whatever proxy the environment
// names: the client it drives reaches the fake directly too.
#[cfg(feature = "replay")]
#[tokio::test]
async fn a_client_a_replay_drives_goes_through_no_proxy_of_its_environment() {
    pointed_at_a_fake_through("replay").await;
}

// A server is reached when the file `SSL_CERT_FILE` names holds the
// authority that signed its certificate, and refused otherwise, in words
// that say so: one another authority signed, one whose certificate is for
// another name, and, in an environment that names no file, one the test's
// own authority signed, which no system trusts.
#[tokio::test]
async fn a_client_trusts_the_authority_its_environment_names_and_no_other() {
    let scratch = Scratch::new("authority");
    let (ours, theirs) = (Authority::new(), Authority::new());
    let serving = |authority: &Authority, name: &str| {
        let reply = vec![Reply::json(200, "trusted")];
        Fake::serving_tls(reply, authority.serving(&[name]))
    };
    let trusted = serving(&ours, "127.0.0.1").await;
    let of_another = serving(&theirs, "127.0.0.1").await;
    let for_another_name = serving(&ours, "other.example").await;
    let url = |fake: &Fake| format!("https://{}{PATH}", fake.address);
    let asks = [url(&trusted), url(&of_another), url(&for_another_name)];
    let file = scratch.file("authority.pem", &ours.pem());
    let came = child(&[("SSL_CERT_FILE", &file)], &asks).await;
    assert_eq!(
        came.of_each,
        [
            answered("trusted"),
            lost("the provider's certificate is not trusted"),
            lost("the provider's certificate is not for its host name"),
        ]
    );
    assert_eq!(trusted.seen().len(), 1);
    assert!(of_another.seen().is_empty() && for_another_name.seen().is_empty());
    // Without the variable, and with it set to nothing, the system decides.
    for variables in [vec![], vec![("SSL_CERT_FILE", "")]] {
        let came = child(&variables, &[url(&trusted)]).await;
        assert_eq!(
            came.of_each,
            [lost("the provider's certificate is not trusted")]
        );
    }
    assert_eq!(trusted.seen().len(), 1);
}

// The directories `SSL_CERT_DIR` names give the authorities filed there
// under the name OpenSSL files one by; the file `SSL_CERT_FILE` names is
// read before them, and they are then not read.
#[tokio::test]
async fn a_client_trusts_what_is_filed_in_the_directories_its_environment_names() {
    let scratch = Scratch::new("directories");
    let (filed, unfiled) = (Authority::new(), Authority::new());
    scratch.file("0a1b2c3d.0", &filed.pem());
    scratch.file("unfiled.pem", &unfiled.pem());
    let serving = |authority: &Authority| {
        let reply = vec![Reply::json(200, "trusted"), Reply::json(200, "trusted")];
        Fake::serving_tls(reply, authority.serving(&["127.0.0.1"]))
    };
    let (of_filed, of_unfiled) = (serving(&filed).await, serving(&unfiled).await);
    let asks = [&of_filed, &of_unfiled].map(|fake| format!("https://{}{PATH}", fake.address));
    let directories = env::join_paths([scratch.0.join("nowhere"), scratch.0.clone()]);
    let directories = directories.unwrap().into_string().unwrap();
    let came = child(&[("SSL_CERT_DIR", &directories)], &asks).await;
    let refused = lost("the provider's certificate is not trusted");
    assert_eq!(came.of_each, [answered("trusted"), refused.clone()]);
    let file = scratch.file("authority.pem", &unfiled.pem());
    let both = [
        ("SSL_CERT_DIR", directories.as_str()),
        ("SSL_CERT_FILE", &file),
    ];
    let came = child(&both, &asks).await;
    assert_eq!(came.of_each, [refused, answered("trusted")]);
}

// A client is not built in an environment Python's client refuses when it
// is built, and says which variable does not read, without what it holds.
#[tokio::test]
async fn a_client_is_not_built_in_an_environment_that_does_not_read() {
    let scratch = Scratch::new("unread");
    let socks = format!("socks5://{CREDENTIALS}@127.0.0.1:1080");
    let came = child(&[("ALL_PROXY", &socks)], &[]).await;
    let reason = "the environment names a SOCKS proxy, which is not supported";
    assert_eq!(came.of_each, [unbuilt(reason)]);
    assert!(!came.printed.contains("placeholder-not-a-password"));
    let missing = format!("{}/missing.pem", scratch.0.display());
    let came = child(&[("SSL_CERT_FILE", &missing)], &[]).await;
    assert_eq!(
        came.of_each,
        [unbuilt("the file SSL_CERT_FILE names cannot be read")]
    );
    let empty = scratch.file("empty.pem", "");
    let came = child(&[("SSL_CERT_FILE", &empty)], &[]).await;
    assert_eq!(
        came.of_each,
        [unbuilt("the file SSL_CERT_FILE names holds no certificate")]
    );
}
