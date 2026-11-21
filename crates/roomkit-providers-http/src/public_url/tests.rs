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

// The client's parser reads each host as a name, which the client resolves
// by itself: the public address it spells without its dots does not stand
// for it.
#[test]
fn a_name_that_spells_a_public_address_without_its_dots_is_still_resolved() {
    let options = ValidatePublicUrlOptions::default();
    for (uri, name) in [
        ("http://15.0x../", "15.0x"),
        ("http://192.10.0x.0x..../", "192.10.0x.0x"),
    ] {
        let mut asked = Vec::new();
        let unanswered = validate(uri, &options, |host, port| {
            asked.push((host.to_owned(), port));
            Vec::new()
        });
        assert_eq!(
            unanswered,
            Err(format!("URL host '{name}' did not resolve to any address"))
        );
        assert_eq!(asked, [(name.to_owned(), 80)]);
        assert_eq!(
            validate(uri, &options, |_, _| vec!["10.0.0.5".parse().unwrap()]),
            Err(format!(
                "URL host '{name}' resolves to private address 10.0.0.5"
            ))
        );
        assert_eq!(
            validate(uri, &options, |_, _| vec!["93.184.216.34".parse().unwrap()]),
            Ok(uri)
        );
    }
}

// No name is taken unchecked under the defaults. With a resolver that
// answers nothing, a URL is taken only when the client's parser reads its
// host as an address, whatever numbers, hexadecimal parts and dots the host
// is written with.
#[test]
fn a_host_is_taken_without_an_answer_only_when_the_parser_reads_an_address() {
    const PARTS: [&str; 6] = ["0", "15", "0x", "0xf", "017", "256"];
    let options = ValidatePublicUrlOptions::default();
    let mut hosts = vec![String::new()];
    let mut taken = 0;
    for _ in 0..4 {
        hosts = hosts
            .iter()
            .flat_map(|host| PARTS.iter().map(move |part| format!("{host}{part}.")))
            .collect();
        for host in &hosts {
            for dots in ["", ".", "..", "..."] {
                let uri = format!("http://{}{dots}/", host.trim_end_matches('.'));
                if validate(&uri, &options, |_, _| Vec::new()).is_err() {
                    continue;
                }
                let parsed = url::Url::parse(&uri).unwrap();
                assert!(matches!(parsed.host(), Some(url::Host::Ipv4(_))), "{uri}");
                taken += 1;
            }
        }
    }
    assert!(taken > 0);
}
