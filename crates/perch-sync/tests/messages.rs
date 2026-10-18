use perch_sync::message::{ErrorCode, HeadEntry, Hello, Limits, Message, ParseError, Role, WantEntry, limits};

fn h64(c: char) -> String {
    std::iter::repeat_n(c, 64).collect()
}

fn round_trip(m: Message) {
    let text = m.to_json();
    let back = Message::parse(&text).unwrap_or_else(|e| panic!("{e}: {text}"));
    assert_eq!(back, m, "{text}");
}

#[test]
fn every_message_round_trips() {
    round_trip(Message::Hello(Hello {
        versions: vec![1],
        node: "0123456789abcdef0123456789abcdef".into(),
        role: Role::Node,
        software: "perch-node/0.1.0".into(),
        listen: Some("wss://perch.example.org/v1/sync".into()),
        limits: Limits::default(),
    }));
    round_trip(Message::GetHeads { after: None, limit: 4096 });
    round_trip(Message::GetHeads { after: Some(h64('a')), limit: 10 });
    round_trip(Message::Heads {
        heads: vec![
            HeadEntry { author: h64('1'), seq: 12, id: Some(h64('2')), forked: false, evidence: None },
            HeadEntry { author: h64('3'), seq: 0, id: None, forked: true, evidence: Some([h64('4'), h64('5')]) },
        ],
        next: Some(h64('3')),
    });
    round_trip(Message::Want {
        wants: vec![
            WantEntry { author: h64('1'), from_seq: 13, to_seq: None },
            WantEntry { author: h64('2'), from_seq: 1, to_seq: Some(9) },
        ],
    });
    round_trip(Message::Records { records: vec!["{\"a\":1}".into()], truncated: true });
    round_trip(Message::Push { records: vec!["{\"a\":1}".into()], evidence: vec![["{\"b\":1}".into(), "{\"c\":2}".into()]] });
    round_trip(Message::Push { records: vec![], evidence: vec![] });
    round_trip(Message::WantEvidence { authors: vec![h64('a')] });
    round_trip(Message::Evidence { pairs: vec![["{}".into(), "{\"x\":[1,2]}".into()]] });
    round_trip(Message::WantBodies { ids: vec![h64('b')] });
    round_trip(Message::Bodies { records: vec!["{\"z\":\"é\"}".into()], truncated: false });
    round_trip(Message::GetPeers);
    round_trip(Message::Peers { peers: vec!["ws://127.0.0.1:7701/v1/sync".into()] });
    round_trip(Message::error(ErrorCode::RateLimited, "slow down"));
}

#[test]
fn records_are_kept_verbatim() {
    let text = r#"{"type":"records","records":[{"b" : 1, "a":"x"}],"truncated":false}"#;
    match Message::parse(text).unwrap() {
        Message::Records { records, .. } => assert_eq!(records, vec![r#"{"b" : 1, "a":"x"}"#.to_string()]),
        m => panic!("{m:?}"),
    }
}

#[test]
fn strict_parsing() {
    let bad = [
        r#"{"type":"hello","protocol":"other","versions":[1],"node":"0123456789abcdef0123456789abcdef","role":"node"}"#,
        r#"{"type":"hello","protocol":"perch-sync","versions":[1],"node":"xyz","role":"node"}"#,
        r#"{"type":"hello","protocol":"perch-sync","versions":[1],"node":"0123456789abcdef0123456789abcdef","role":"relay"}"#,
        r#"{"type":"hello","protocol":"perch-sync","versions":[1],"node":"0123456789abcdef0123456789abcdef","role":"node","listen":"http://x"}"#,
        r#"{"type":"want","wants":[{"author":"AB","from_seq":1}]}"#,
        r#"{"type":"want","wants":[{"author":"0000000000000000000000000000000000000000000000000000000000000000","from_seq":0}]}"#,
        r#"{"type":"records","records":[1]}"#,
        r#"{"type":"records"}"#,
        r#"{"type":"get_heads","after":"zz"}"#,
        r#"[1,2]"#,
        r#"not json"#,
    ];
    for b in bad {
        assert!(matches!(Message::parse(b), Err(ParseError::Bad(_))), "{b}");
    }
    // Heads must be sorted and consistent.
    let unsorted = format!(
        r#"{{"type":"heads","heads":[{{"author":"{}","seq":1,"id":"{}"}},{{"author":"{}","seq":1,"id":"{}"}}],"next":null}}"#,
        h64('b'),
        h64('1'),
        h64('a'),
        h64('1')
    );
    assert!(Message::parse(&unsorted).is_err());
    let null_id = format!(r#"{{"type":"heads","heads":[{{"author":"{}","seq":3,"id":null}}],"next":null}}"#, h64('b'));
    assert!(Message::parse(&null_id).is_err());
}

#[test]
fn entry_limits() {
    let ids: Vec<String> = (0..limits::MAX_BODY_IDS + 1).map(|_| format!("\"{}\"", h64('a'))).collect();
    let text = format!(r#"{{"type":"want_bodies","ids":[{}]}}"#, ids.join(","));
    assert!(matches!(Message::parse(&text), Err(ParseError::TooLarge(_))));
    let recs: Vec<&str> = (0..limits::MAX_RECORDS + 1).map(|_| "{}").collect();
    let text = format!(r#"{{"type":"push","records":[{}]}}"#, recs.join(","));
    assert!(matches!(Message::parse(&text), Err(ParseError::TooLarge(_))));
}

#[test]
fn unknown_types_and_fields_are_tolerated() {
    assert_eq!(Message::parse(r#"{"type":"future_thing","x":1}"#).unwrap(), Message::Unknown { type_name: "future_thing".into() });
    assert_eq!(Message::parse(r#"{"type":"get_peers","extra":true}"#).unwrap(), Message::GetPeers);
    match Message::parse(r#"{"type":"error","code":"new_code","message":"m"}"#).unwrap() {
        Message::Error { code, fatal, .. } => {
            assert_eq!(code, ErrorCode::Other("new_code".into()));
            assert!(!fatal);
        }
        m => panic!("{m:?}"),
    }
}

#[test]
fn hello_limits_are_clamped() {
    let text = r#"{"type":"hello","protocol":"perch-sync","versions":[1,2],"node":"0123456789abcdef0123456789abcdef","role":"browser","listen":null,"limits":{"max_message_bytes":999999999,"max_records":5,"max_heads":100}}"#;
    match Message::parse(text).unwrap() {
        Message::Hello(h) => {
            assert_eq!(h.limits.max_message_bytes, limits::MAX_MESSAGE_BYTES);
            assert_eq!(h.limits.max_records, 5);
            assert_eq!(h.role, Role::Browser);
        }
        m => panic!("{m:?}"),
    }
}
