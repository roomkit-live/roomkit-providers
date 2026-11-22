//! Asks three servers from behind a proxy, with nothing of the proxy in the
//! provider's configuration: the provider reads the environment of its
//! process once, when it is built, as `RoomKit`'s Python provider does.
//!
//! - `HTTPS_PROXY` carries the request for an `https` endpoint through a
//!   tunnel, and `SSL_CERT_FILE` names the authority that signed the
//!   endpoint's certificate;
//! - `HTTP_PROXY` is handed the request for an `http` endpoint whole;
//! - `NO_PROXY` exempts the third endpoint, which is reached directly.
//!
//! A process does not set its own variables: this example runs itself
//! again as a child, in the environment a host behind a proxy has, and the
//! child is the host's code. Everything else is a stand-in on a local port:
//! the proxy, the three servers, and the authority, which is made when the
//! example runs. Nothing leaves the machine, and the key is a placeholder.
//!
//! ```text
//! cargo run -p roomkit-providers-openai --example behind_a_proxy
//! ```

use std::env;
use std::error::Error;
use std::io::Write as _;
use std::process::{Command, ExitCode};

use roomkit_ai::{AiContext, AiMessage, AiProvider, AiRole};
use roomkit_providers_client::harness::fake::{Fake, Reply, Seen};
use roomkit_providers_client::harness::tls::Authority;
use roomkit_providers_openai::{OpenAi, OpenAiConfig};
use serde_json::json;

/// What tells the example that it is the child: the host behind the proxy.
const HOST: &str = "as-the-host";

/// The credentials the proxy asks for: placeholders.
const CREDENTIALS: &str = "user:placeholder-not-a-password";

/// What the provider writes of [`CREDENTIALS`] for the proxy.
const BASIC: &str = "Basic dXNlcjpwbGFjZWhvbGRlci1ub3QtYS1wYXNzd29yZA==";

/// The path of Chat Completions under a base URL that ends in `/v1`.
const PATH: &str = "/v1/chat/completions";

/// A whole completion that says `said`.
fn completion(said: &str) -> Reply {
    let message = json!({"role": "assistant", "content": said});
    let body = json!({
        "id": "chatcmpl-1",
        "object": "chat.completion",
        "model": "local-model",
        "choices": [{"index": 0, "message": message, "finish_reason": "stop"}],
    });
    Reply::json(200, body.to_string())
}

/// The host's code: it asks each base URL it is given, and names no proxy
/// and no authority. What it reads of them, it reads from its environment.
async fn host(base_urls: Vec<String>) -> Result<(), Box<dyn Error>> {
    let mut shown = std::io::stdout().lock();
    for base_url in base_urls {
        let config = OpenAiConfig {
            base_url: Some(base_url.clone()),
            ..OpenAiConfig::new("placeholder-not-a-key", "local-model")
        };
        let provider = OpenAi::new(config)?;
        let request = AiContext {
            messages: vec![AiMessage::new(AiRole::User, "How did you get here?")],
            ..AiContext::default()
        };
        let response = provider.generate(&request).await?;
        writeln!(shown, "{base_url}: {}", response.content)?;
    }
    Ok(())
}

/// Whether a provider reads the variable `name` when it is built.
fn is_read_by_a_provider(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let named = matches!(
        name.as_str(),
        "request_method" | "ssl_cert_file" | "ssl_cert_dir"
    );
    named || name.ends_with("_proxy")
}

/// The only request a stand-in was sent, with its `Proxy-Authorization`.
fn only(seen: &[Seen]) -> Option<(&str, &str, Option<&str>)> {
    let [request] = seen else {
        return None;
    };
    let authorization = request.headers.get("proxy-authorization");
    let authorization = authorization.map(String::as_str);
    Some((&request.method, &request.target, authorization))
}

/// Starts the stand-ins, runs the host among them, and checks which way
/// each of its requests went.
async fn among_stand_ins() -> Result<(), Box<dyn Error>> {
    let authority = Authority::new();
    let tls = authority.serving(&["secure.example"]);
    let secure = Fake::serving_tls(vec![completion("Through a tunnel.")], tls).await;
    let near = Fake::serving(vec![completion("Directly.")]).await;
    // The proxy opens a tunnel for the first request, to the server it
    // knows as `secure.example`, and answers the second itself, as the
    // server it knows as `plain.example` would.
    let proxy = Fake::serving(vec![
        Reply::Tunnel(secure.address),
        completion("Handed to the proxy."),
    ])
    .await;

    let scratch = env::temp_dir().join(format!("behind-a-proxy-{}", std::process::id()));
    std::fs::create_dir_all(&scratch)?;
    let authorities = scratch.join("authority.pem");
    std::fs::write(&authorities, authority.pem())?;

    let mut command = Command::new(env::current_exe()?);
    command.arg(HOST).args([
        "https://secure.example/v1".to_owned(),
        "http://plain.example/v1".to_owned(),
        format!("http://{}/v1", near.address),
    ]);
    // The host's environment: this one, without what a provider would read
    // of it, and with the variables of a host behind the proxy.
    for (name, _) in env::vars_os() {
        if is_read_by_a_provider(&name.to_string_lossy()) {
            command.env_remove(name);
        }
    }
    let through = format!("http://{CREDENTIALS}@{}", proxy.address);
    command
        .env("HTTPS_PROXY", &through)
        .env("HTTP_PROXY", &through)
        .env("NO_PROXY", "127.0.0.1")
        .env("SSL_CERT_FILE", &authorities);
    // The stand-ins answer the host while it runs.
    let ran = tokio::task::spawn_blocking(move || command.status()).await?;
    std::fs::remove_dir_all(&scratch)?;
    if !ran?.success() {
        return Err("the host behind the proxy failed".into());
    }

    let asked = proxy.seen();
    let [tunnel, handed] = asked.as_slice() else {
        return Err("the proxy was not asked twice".into());
    };
    let handed = std::slice::from_ref(handed);
    let tunnel = std::slice::from_ref(tunnel);
    if only(tunnel) != Some(("CONNECT", "secure.example:443", Some(BASIC))) {
        return Err("the https endpoint was not reached through a tunnel of the proxy".into());
    }
    if only(&secure.seen()) != Some(("POST", PATH, None)) {
        return Err("the tunnel did not carry the request to the https endpoint".into());
    }
    let whole = format!("http://plain.example{PATH}");
    if only(handed) != Some(("POST", &whole, Some(BASIC))) {
        return Err("the request for the http endpoint was not handed to the proxy".into());
    }
    if only(&near.seen()) != Some(("POST", PATH, None)) {
        return Err("the host NO_PROXY names was not reached directly".into());
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let mut arguments = env::args().skip(1);
    let ran = if arguments.next().as_deref() == Some(HOST) {
        host(arguments.collect()).await
    } else {
        among_stand_ins().await
    };
    match ran {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // A failure that cannot be written is still a failure.
            writeln!(std::io::stderr(), "{error}").ok();
            ExitCode::FAILURE
        }
    }
}
