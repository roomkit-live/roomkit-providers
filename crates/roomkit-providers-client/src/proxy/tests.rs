//! Which proxy an environment names for an endpoint: the table of what the
//! client of `RoomKit`'s Python providers decides
//! (`tests/fixtures/python_proxy_cases.json`), read here and held to the
//! same answers. No test sets a variable: each gives the variables an
//! environment would hold.

use serde::Deserialize;

use super::*;
use crate::testing::left;

/// What Python's client decided of a table of environments.
#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    /// The proxy variables, in the order the environment holds them.
    environment: Vec<(String, String)>,
    /// The exception Python's client raises when it is built.
    refused: Option<String>,
    asked: Vec<Asked>,
}

/// An endpoint, and where Python's client sends a request for it.
#[derive(Deserialize)]
struct Asked {
    url: String,
    proxy: Option<Dialed>,
    /// What the header the proxy is sent decodes to: the user and the
    /// password, around a colon.
    credentials: Option<String>,
    /// The user the proxy is given.
    user: Option<String>,
    /// The password the proxy is given.
    password: Option<String>,
}

/// The proxy a request goes through.
#[derive(Debug, Deserialize, PartialEq, Eq)]
struct Dialed {
    scheme: String,
    host: String,
    port: u32,
}

/// The cases Python's client reads and this one refuses when it is built,
/// each with the reason: a proxy Python's client could not reach, or a
/// port its number parser alone reads.
const REFUSED_HERE_ALONE: [(&str, &str); 8] = [
    (
        "a_proxy_without_a_host",
        "Python's client takes it, and connects to no host",
    ),
    (
        "a_proxy_whose_host_holds_a_backslash",
        "Python's client asks the network for a name that holds the backslash; the HTTP client \
         here would end the host at it, and dial another",
    ),
    (
        "a_proxy_whose_host_holds_a_percent_sign",
        "Python's client asks the network for the name as written; the HTTP client here would \
         decode it, and dial another",
    ),
    (
        "a_proxy_whose_host_holds_a_quote",
        "Python's client asks the network for the name as written; the HTTP client here would \
         not read the proxy, and send around it",
    ),
    (
        "a_proxy_whose_ipv6_address_has_a_zone",
        "Python's client takes the zone of a link-local address, which this one does not read",
    ),
    (
        "a_proxy_whose_port_is_past_the_last",
        "Python's client takes it, and no connection opens on such a port",
    ),
    (
        "a_proxy_whose_port_has_a_sign",
        "Python reads a port with `int`, which takes a sign",
    ),
    (
        "a_port_with_a_sign",
        "Python reads a port with `int`, which takes a sign",
    ),
];

/// The endpoints this client reaches directly where Python's goes through
/// the proxy: an address of `NO_PROXY` is compared with the address the
/// HTTP client dials, where Python compares it with the text of the URL.
const EXEMPTED_HERE_ALONE: [(&str, &str); 3] = [
    (
        "an_ipv6_endpoint_is_the_text_it_is_written_in",
        "http://[0:0:0:0:0:0:0:1]/v1",
    ),
    (
        "an_ipv4_endpoint_is_the_text_it_is_written_in",
        "http://127.1/v1",
    ),
    (
        "an_address_written_short_exempts_the_endpoint_written_as_it_is",
        "http://127.0.0.1/v1",
    ),
];

fn fixture() -> Fixture {
    let fixture = include_str!("../../tests/fixtures/python_proxy_cases.json");
    serde_json::from_str(fixture).unwrap()
}

/// Where `proxy` is dialed, which is all its address holds: nothing of the
/// user information or of the path its variable wrote.
fn dialed(proxy: &Proxy) -> Dialed {
    let address = &proxy.address;
    let nothing_else = (address.username(), address.password(), address.path());
    assert_eq!(nothing_else, ("", None, "/"));
    assert_eq!((address.query(), address.fragment()), (None, None));
    let host = address.host_str().unwrap_or_default();
    Dialed {
        scheme: address.scheme().to_owned(),
        host: host.trim_matches(['[', ']']).to_owned(),
        port: address.port_or_known_default().map(u32::from).unwrap(),
    }
}

// Python: httpx2 `get_environment_proxies`, `URLPattern` and
// `_transport_for_url`, over urllib's `getproxies_environment`.
#[test]
fn an_environment_names_the_proxies_python_s_client_reads_of_it() {
    let (mut as_python, mut refused_here_alone, mut exempted_here_alone) = (0, 0, 0);
    for case in fixture().cases {
        let name = case.name.as_str();
        let named = REFUSED_HERE_ALONE.iter().any(|(named, _)| *named == name);
        let read = Proxies::read(case.environment.clone());
        let proxies = match (case.refused, read) {
            (Some(_), Err(_)) => {
                assert!(!named, "{name}: Python's client refuses it too");
                as_python += 1;
                continue;
            }
            (Some(raised), Ok(_)) => panic!("{name}: Python's client raises {raised}"),
            (None, Err(failure)) => {
                let said = left(&failure).message;
                assert!(named, "{name}: Python's client reads it, here: {said}");
                refused_here_alone += 1;
                continue;
            }
            (None, Ok(proxies)) => proxies,
        };
        assert!(!named, "{name}: it is read here");
        for asked in case.asked {
            let url = reqwest::Url::parse(&asked.url).unwrap();
            let taken = proxies.for_url(&url).map(|(_, proxy)| proxy);
            let proxy = taken.map(dialed);
            let about = format!("{name}: {}", asked.url);
            if EXEMPTED_HERE_ALONE.contains(&(name, asked.url.as_str())) {
                assert!(
                    asked.proxy.is_some(),
                    "{about}: Python's client goes direct too"
                );
                assert_eq!(proxy, None, "{about}");
                exempted_here_alone += 1;
                continue;
            }
            assert_eq!(proxy, asked.proxy, "{about}");
            // The user and the password, each as Python's client gives it,
            // and the two as the header carries them.
            let given = taken.and_then(|proxy| proxy.credentials.as_ref());
            let given = given.map(|given| (given.user.clone(), given.password.clone()));
            assert_eq!(given, asked.user.zip(asked.password), "{about}");
            let sent = given.map(|(user, password)| format!("{user}:{password}"));
            assert_eq!(sent, asked.credentials, "{about}");
        }
        as_python += 1;
    }
    assert_eq!(
        (as_python, refused_here_alone, exempted_here_alone),
        (128, 8, 3)
    );
}

// A proxy's name holds letters, digits, hyphens, dots and underscores. The
// HTTP client reads a proxy's address twice, with two readers that do not
// take every other sign alike: it ends a host at a backslash, decodes one
// after a percent sign, and sends around a proxy whose name holds a quote.
// Such a name is refused when the client is built, written in ASCII or in
// a character the HTTP client turns into one.
#[test]
fn a_proxy_whose_name_holds_another_sign_is_refused() {
    let read = |host: &str| {
        let named = format!("http://{host}:3128");
        Proxies::read([("HTTPS_PROXY".to_owned(), named)])
    };
    // The signs that end an authority, or part it, name another proxy.
    let parts_an_authority = |sign: &char| "/?#@:[]".contains(*sign);
    let of_a_name = |sign: &char| sign.is_ascii_alphanumeric() || "-._".contains(*sign);
    let others = (' '..='~').filter(|sign| !of_a_name(sign) && !parts_an_authority(sign));
    let mut refused = 0;
    for sign in others.chain(['\u{ff02}', '\u{ff5b}', '\u{2100}']) {
        let unread = read(&format!("pro{sign}xy.example")).map(|_| ());
        assert_eq!(
            left(&unread.unwrap_err()).message,
            "the provider cannot be built: a proxy the environment names does not read as a URL",
            "{sign}"
        );
        refused += 1;
    }
    assert_eq!(refused, 23 + 3);
    for name in [
        "pro-xy_1.example",
        "PROXY.Example.",
        "b\u{fc}cher.example",
        "10.0.0.7",
    ] {
        assert!(read(name).is_ok(), "{name}");
    }
}

/// What a provider says when it cannot be built in the environment
/// `variables` make.
fn unbuilt(variables: &[(&str, &str)]) -> String {
    let variables = variables.iter();
    let read =
        Proxies::read(variables.map(|(name, value)| ((*name).to_owned(), (*value).to_owned())));
    let failure = read.map(|_| ()).unwrap_err();
    let error = left(&failure);
    assert!(error.before_request && !error.retryable);
    error.message
}

// A provider that cannot be built says which kind of text does not read,
// and nothing of the text: a proxy's URL may carry credentials.
#[test]
fn an_environment_that_does_not_read_is_refused_in_words_that_repeat_nothing_of_it() {
    let secret = "placeholder-not-a-password";
    let socks = format!("socks5://user:{secret}@proxy.example:1080");
    assert_eq!(
        unbuilt(&[("ALL_PROXY", &socks)]),
        "the provider cannot be built: the environment names a SOCKS proxy, which is not \
         supported"
    );
    let other = format!("ftp://user:{secret}@proxy.example");
    assert_eq!(
        unbuilt(&[("HTTPS_PROXY", &other)]),
        "the provider cannot be built: a proxy the environment names is neither an http nor an \
         https proxy"
    );
    let unread = format!("http://user:{secret}@proxy.example:port");
    assert_eq!(
        unbuilt(&[("HTTP_PROXY", &unread)]),
        "the provider cannot be built: a proxy the environment names does not read as a URL"
    );
    assert_eq!(
        unbuilt(&[("ALL_PROXY", "http://proxy.example"), ("NO_PROXY", "[::1]")]),
        "the provider cannot be built: an entry of NO_PROXY in the environment does not read"
    );
}

// The `Debug` form of the proxies says how many they are, and shows no URL.
#[test]
fn the_debug_form_of_the_proxies_shows_no_url() {
    let proxy = "http://user:placeholder-not-a-password@proxy.example:3128";
    let variables = [("ALL_PROXY", proxy), ("NO_PROXY", "localhost")];
    let variables = variables.map(|(name, value)| (name.to_owned(), value.to_owned()));
    let proxies = Proxies::read(variables).unwrap();
    assert_eq!(format!("{proxies:?}"), "Proxies { rules: 2, proxies: 1 }");
}
