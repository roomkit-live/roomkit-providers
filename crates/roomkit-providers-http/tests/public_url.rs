//! Applications can validate an outbound URL independently of a provider
//! client. No test here asks a name of the network: each URL is an address,
//! is refused before any lookup, or is checked without resolution.

use roomkit_providers_http::{ValidatePublicUrlOptions, validate_public_url};

#[test]
fn a_host_can_check_an_address_without_rewriting_it() {
    let url = "https://service.example:8443/A?q=1#fragment";
    let offline = ValidatePublicUrlOptions {
        resolve_dns: false,
        ..Default::default()
    };
    assert_eq!(validate_public_url(url, &offline).unwrap(), url);
    assert_eq!(
        validate_public_url("https://localhost", &offline),
        Err("URL must not point to localhost (host='localhost')".to_owned())
    );
    assert_eq!(
        validate_public_url("https://127.0.0.1", &offline),
        Err("URL host '127.0.0.1' resolves to loopback address 127.0.0.1".to_owned())
    );
}

// Python: validate_public_url(allowed_schemes=("http", "https"),
// resolve_dns=True).
#[test]
fn callers_can_set_the_schemes_and_the_default_checks_dns() {
    let defaults = ValidatePublicUrlOptions::default();
    assert!(defaults.resolve_dns);
    assert_eq!(defaults.allowed_schemes, ["http", "https"]);
    let secure = ValidatePublicUrlOptions {
        allowed_schemes: &["https"],
        resolve_dns: false,
    };
    assert_eq!(
        validate_public_url("http://service.example", &secure),
        Err("URL scheme must be one of ('https',), got 'http'".to_owned())
    );
    assert!(validate_public_url("https://service.example", &secure).is_ok());
}

// Python: tests/test_providers/test_public_url.py::test_an_invalid_port_is_rejected_before_dns
#[test]
fn explicit_ports_are_checked_before_resolving_a_host() {
    for dns in [true, false] {
        let options = ValidatePublicUrlOptions {
            resolve_dns: dns,
            ..Default::default()
        };
        for host in ["service.example", "93.184.216.34"] {
            for (port, words) in [
                (
                    "invalid",
                    "Port could not be cast to integer value as 'invalid'",
                ),
                ("65536", "Port out of range 0-65535"),
            ] {
                let url = format!("https://{host}:{port}/");
                assert_eq!(
                    validate_public_url(&url, &options),
                    Err(words.to_owned()),
                    "{url}"
                );
            }
        }
    }
}

#[test]
fn an_application_scheme_uses_the_same_address_policy() {
    let options = ValidatePublicUrlOptions {
        allowed_schemes: &["service"],
        resolve_dns: false,
    };
    let uri = "service://example.org/resource";
    assert_eq!(validate_public_url(uri, &options), Ok(uri));
    assert!(validate_public_url("service://127.0.0.1/resource", &options).is_err());
}

// Python: validate_public_url("http://127.0.01../", resolve_dns=False). The
// dots dropped, the host is the address its short form spells: no lookup is
// needed to refuse it.
#[test]
fn a_short_address_followed_by_dots_is_refused_without_a_lookup() {
    let offline = ValidatePublicUrlOptions {
        resolve_dns: false,
        ..Default::default()
    };
    for dots in [".", "..", "..."] {
        assert_eq!(
            validate_public_url(&format!("http://127.0.01{dots}/"), &offline),
            Err("URL host '127.0.01' resolves to loopback address 127.0.0.1".to_owned()),
            "{dots}"
        );
    }
    let name = "http://service.example../";
    assert_eq!(validate_public_url(name, &offline), Ok(name));
}

// The default resolves a name. An address is judged as it is written, and
// asks none.
#[test]
fn an_address_is_judged_without_a_lookup_under_the_defaults() {
    let options = ValidatePublicUrlOptions::default();
    let public = "https://[2606:4700:4700::1111]:8443/hook";
    assert_eq!(validate_public_url(public, &options), Ok(public));
    assert_eq!(
        validate_public_url("http://[::ffff:169.254.169.254]/latest", &options),
        Err(
            "URL host '::ffff:169.254.169.254' resolves to private address \
             ::ffff:169.254.169.254"
                .to_owned()
        )
    );
}
