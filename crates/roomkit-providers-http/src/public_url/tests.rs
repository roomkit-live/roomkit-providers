//! The validator with a resolver of its own: no test here asks a name of the
//! network.

use super::*;

#[test]
fn every_dns_answer_is_checked_at_the_callers_port() {
    let options = ValidatePublicUrlOptions::default();
    let uri = "https://service.example:8443/path";
    let result = validate(uri, &options, |host, port| {
        assert_eq!((host, port), ("service.example", 8443));
        vec![
            "93.184.216.34".parse().unwrap(),
            "127.0.0.1".parse().unwrap(),
        ]
    });
    assert_eq!(
        result,
        Err("URL host 'service.example' resolves to loopback address 127.0.0.1".to_owned())
    );
    assert_eq!(
        validate(uri, &options, |_, _| Vec::new()),
        Err("URL host 'service.example' did not resolve to any address".to_owned())
    );
    assert_eq!(
        validate(uri, &options, |_, _| vec!["93.184.216.34".parse().unwrap()]),
        Ok(uri)
    );
}

#[test]
fn the_raw_authority_is_checked_before_resolution() {
    for authority in [
        r"name\part.example",
        "name\tpart.example",
        "name%2Epart.example",
        "one@two@service.example",
        "[2001:db8::1%25example]",
        "caf\u{e9}.example",
        "\u{212a}.example",
    ] {
        for scheme in ["https", "service"] {
            for resolve_dns in [false, true] {
                let options = ValidatePublicUrlOptions {
                    allowed_schemes: &[scheme],
                    resolve_dns,
                };
                let uri = format!("{scheme}://{authority}/");
                assert!(
                    validate(&uri, &options, |_, _| panic!(
                        "invalid authority reached DNS"
                    ))
                    .is_err()
                );
            }
        }
    }
}

#[test]
fn ascii_dns_spelling_preserves_userinfo_path_and_query() {
    let uri = "service://user%40mail@XN--CAF-DMA.example/caf\u{e9}?q=\u{e9}t\u{e9}";
    let options = ValidatePublicUrlOptions {
        allowed_schemes: &["service"],
        resolve_dns: true,
    };
    assert_eq!(
        validate(uri, &options, |host, port| {
            assert_eq!((host, port), ("xn--caf-dma.example", 80));
            vec!["93.184.216.34".parse().unwrap()]
        }),
        Ok(uri)
    );
}

#[test]
fn mixed_dns_answers_include_the_ipv6_policy() {
    let result = validate(
        "https://service.example/",
        &ValidatePublicUrlOptions::default(),
        |_, _| {
            vec![
                "93.184.216.34".parse().unwrap(),
                std::net::Ipv6Addr::LOCALHOST.into(),
            ]
        },
    );
    assert_eq!(
        result,
        Err("URL host 'service.example' resolves to loopback address ::1".to_owned())
    );
}

#[test]
fn custom_schemes_share_the_same_leading_whitespace_rule() {
    let options = ValidatePublicUrlOptions {
        allowed_schemes: &["service"],
        resolve_dns: false,
    };
    for prefix in [" ", "\t", "\r", "\n", "\u{00a0}"] {
        let uri = format!("{prefix}service://service.example/");
        assert_eq!(
            validate(&uri, &options, |_, _| unreachable!()).is_ok(),
            prefix != "\u{00a0}"
        );
    }
}
