//! Live tests: real `perch-node` processes on localhost syncing over WebSocket.

mod common;

use std::time::Duration;

use common::*;
use perch_core::{Keypair, Record, RecordId, Unsigned, sign_record};
use serde_json::{Value, json};

const T: Duration = Duration::from_secs(60);

fn profile(k: &Keypair, seq: u64, prev: Option<RecordId>, ts: u64, name: &str) -> Record {
    let body = json!({ "name": name, "role": "buyer" }).as_object().unwrap().clone();
    sign_record(k, Unsigned { seq, prev, ts, kind: "profile".into(), replaces: None, body }).unwrap()
}

fn line(nodes: &[&str], policy: &std::path::Path, dir: &std::path::Path) -> Vec<NodeProc> {
    let mut out: Vec<NodeProc> = Vec::new();
    for name in nodes {
        let mut args = vec!["--policy".to_string(), policy.display().to_string(), "--refresh-secs".into(), "5".into()];
        if let Some(prev) = out.last() {
            args.push("--peer".into());
            args.push(prev.ws());
        }
        out.push(spawn(&dir.join(name), &args));
    }
    out
}

fn synced(a: &NodeProc, b: &NodeProc) -> bool {
    let (x, y) = (a.get("/v1/export.jsonl"), b.get("/v1/export.jsonl"));
    x.status == 200 && y.status == 200 && x.body == y.body
}

fn first_product_key(n: &NodeProc) -> String {
    let hits = n.get(&format!("/v1/search?q=&limit=1&now={}", perch_demo::DEMO_NOW)).json();
    hits[0]["key"].as_str().expect("a product").to_string()
}

fn enc(s: &str) -> String {
    s.replace('%', "%25").replace(':', "%3A").replace('/', "%2F").replace(' ', "%20")
}

#[test]
fn a_line_of_three_nodes_converges() {
    let dir = tmpdir("line");
    let (demo, policy) = demo_files(&dir);
    let nodes = line(&["a", "b", "c"], &policy, &dir);
    let (a, c) = (&nodes[0], &nodes[2]);
    let text = std::fs::read_to_string(&demo).unwrap();
    let r = a.post("/v1/records", &text);
    assert_eq!(r.status, 200, "{}", r.body);
    let v = r.json();
    assert!(v["summary"]["inserted"].as_u64().unwrap() >= 300, "{}", v["summary"]);
    assert!(wait_until(T, || c.records() == a.records() && synced(a, c)), "C did not converge with A:\n{}", c.log_text());
    // Views are byte-identical across nodes for a pinned time.
    let key = first_product_key(a);
    let path = format!("/v1/products/{}?now={}", enc(&key), perch_demo::DEMO_NOW);
    let (pa, pc) = (a.get(&path), c.get(&path));
    assert_eq!(pa.status, 200, "{}", pa.body);
    assert_eq!(pa.body, pc.body);
    assert_eq!(pa.header("x-perch-policy-hash"), pc.header("x-perch-policy-hash"));
    // Live gossip of a new record submitted at A.
    let k = Keypair::from_seed([71; 32]);
    let r1 = profile(&k, 1, None, 1_000, "Late Ada");
    assert_eq!(a.post("/v1/records", &r1.to_json_string()).status, 200);
    let p = format!("/v1/records/{}", r1.id());
    assert!(wait_until(T, || c.get(&p).status == 200), "gossip did not reach C");
    assert_eq!(c.get(&p).body, r1.to_json_string());
    let st = nodes[1].status();
    assert_eq!(st["peers"].as_array().unwrap().len(), 2, "B has two sessions: {st}");
}

#[test]
fn b_restart_keeps_data() {
    let dir = tmpdir("restart");
    let (demo, policy) = demo_files(&dir);
    let pol = policy.display().to_string();
    let a = spawn(&dir.join("a"), &["--policy".into(), pol.clone(), "--import".into(), demo.display().to_string()]);
    let mut b = spawn(&dir.join("b"), &["--policy".into(), pol.clone(), "--peer".into(), a.ws()]);
    assert!(wait_until(T, || synced(&a, &b)), "B did not sync");
    let before = b.get("/v1/export.jsonl").body;
    // Hard kill: no graceful shutdown.
    b.kill();
    let b2 = spawn(&dir.join("b"), &["--policy".into(), pol.clone()]);
    assert_eq!(b2.get("/v1/export.jsonl").body, before, "state after SIGKILL and restart");
    assert!(b2.log_text().contains("replayed"));
    drop(b2);
    // Graceful stop and restart of A.
    let mut a = a;
    let before = a.get("/v1/export.jsonl").body;
    a.stop();
    let a2 = spawn(&dir.join("a"), &["--policy".into(), pol]);
    assert_eq!(a2.get("/v1/export.jsonl").body, before);
}

#[test]
fn c_late_joiner_catches_up() {
    let dir = tmpdir("late");
    let (demo, policy) = demo_files(&dir);
    let pol = policy.display().to_string();
    let a = spawn(&dir.join("a"), &["--policy".into(), pol.clone(), "--import".into(), demo.display().to_string()]);
    let b = spawn(&dir.join("b"), &["--policy".into(), pol.clone(), "--peer".into(), a.ws()]);
    assert!(wait_until(T, || synced(&a, &b)));
    let d = spawn(&dir.join("d"), &["--policy".into(), pol, "--peer".into(), b.ws()]);
    assert!(wait_until(T, || synced(&a, &d)), "late joiner did not catch up:\n{}", d.log_text());
    // And then receives live gossip.
    let k = Keypair::from_seed([72; 32]);
    let r1 = profile(&k, 1, None, 5, "Joiner");
    a.post("/v1/records", &r1.to_json_string());
    assert!(wait_until(T, || d.get(&format!("/v1/records/{}", r1.id())).status == 200));
}

#[test]
fn d_equivocation_yields_evidence_on_all_nodes() {
    let dir = tmpdir("equiv");
    let (_, policy) = demo_files(&dir);
    let nodes = line(&["a", "b", "c"], &policy, &dir);
    let k = Keypair::from_seed([73; 32]);
    let r1 = profile(&k, 1, None, 10, "Eve");
    nodes[0].post("/v1/records", &r1.to_json_string());
    let p1 = format!("/v1/records/{}", r1.id());
    assert!(wait_until(T, || nodes.iter().all(|n| n.get(&p1).status == 200)));
    let x = profile(&k, 2, Some(r1.id()), 20, "Eve X");
    let y = profile(&k, 2, Some(r1.id()), 20, "Eve Y");
    assert_eq!(nodes[0].post("/v1/records", &x.to_json_string()).json()["results"][0]["outcome"], "inserted");
    // x may already have reached C by gossip, in which case y is evidence on arrival.
    let oy = nodes[2].post("/v1/records", &y.to_json_string()).json()["results"][0]["outcome"].clone();
    assert!(oy == "inserted" || oy == "fork", "{oy}");
    let author = format!("/v1/authors/{}?now=100", k.author());
    for n in &nodes {
        assert!(
            wait_until(T, || n.get(&author).json()["status"] == "forked"),
            "node {} did not mark the author forked: {}",
            n.addr,
            n.get(&author).body
        );
        for r in [&x, &y] {
            let got = n.get(&format!("/v1/records/{}", r.id()));
            assert_eq!(got.status, 200, "evidence record missing on {}", n.addr);
            assert_eq!(got.body, r.without_body().to_json_string(), "evidence is stored header-only");
        }
        assert_eq!(n.status()["store"]["forked_authors"], 1);
        let log: Value = n.get(&format!("/v1/authors/{}/log", k.author())).json();
        assert_eq!(log.as_array().unwrap().len(), 1, "log cut at fork seq - 1");
    }
    // Every node holds the same export (prefix plus the same evidence pair).
    assert!(synced(&nodes[0], &nodes[1]) && synced(&nodes[1], &nodes[2]));
}

#[test]
fn e_invalid_submissions_are_rejected_with_clear_errors() {
    let dir = tmpdir("invalid");
    let n = spawn(&dir.join("a"), &[]);
    let k = Keypair::from_seed([74; 32]);
    let r1 = profile(&k, 1, None, 1000, "Valid");
    let good = r1.to_json_string();
    let tampered = good.replace("\"Valid\"", "\"Forged\"");
    let r2 = profile(&k, 2, Some(r1.id()), 999, "Older ts");
    let mut badsig = r2.to_json_string();
    let i = badsig.find("\"sig\":\"").unwrap() + 7;
    let flipped = if &badsig[i..i + 1] == "0" { "1" } else { "0" };
    badsig.replace_range(i..i + 1, flipped);
    let unknown = good.replacen('{', "{\"extra\":1,", 1);
    let body = [good.as_str(), tampered.as_str(), badsig.as_str(), unknown.as_str(), "{not json", &r2.to_json_string()].join("\n");
    let r = n.post("/v1/records", &body);
    assert_eq!(r.status, 200, "{}", r.body);
    let v = r.json();
    let res = v["results"].as_array().unwrap();
    let outcome = |i: usize| res[i]["outcome"].as_str().unwrap().to_string();
    let error = |i: usize| res[i]["error"].as_str().unwrap_or("").to_string();
    assert_eq!(outcome(0), "inserted");
    assert_eq!(outcome(1), "invalid");
    assert!(error(1).contains("body_hash"), "{}", error(1));
    assert_eq!(outcome(2), "invalid");
    assert!(error(2).contains("signature"), "{}", error(2));
    assert_eq!(outcome(3), "invalid");
    assert!(error(3).contains("extra"), "{}", error(3));
    assert_eq!(outcome(4), "invalid");
    assert_eq!(res[4]["line"], 5);
    assert_eq!(outcome(5), "ts_decrease");
    assert_eq!(v["summary"]["rejected"], 5);
    assert_eq!(n.records(), 1);
    // Request-level errors.
    let e = n.post("/v1/records", "[1,2");
    assert_eq!(e.status, 400);
    assert_eq!(e.json()["error"]["code"], "bad_json");
    let many = vec![good.as_str(); 1001].join("\n");
    let e = n.post("/v1/records", &many);
    assert_eq!(e.status, 413);
    assert_eq!(e.json()["error"]["code"], "too_many_records");
    let huge = "x".repeat(4 * 1024 * 1024 + 1);
    let e = n.post("/v1/records", &huge);
    assert_eq!(e.status, 413);
    assert_eq!(e.json()["error"]["code"], "too_large");
    let e = n.get("/v1/records/ABC");
    assert_eq!(e.status, 400);
    assert_eq!(e.json()["error"]["code"], "bad_id");
    let e = n.get(&format!("/v1/records/{}", "0".repeat(64)));
    assert_eq!(e.status, 404);
    assert_eq!(e.json()["error"]["code"], "not_found");
    let e = n.get("/v1/lookup/zz");
    assert_eq!(e.json()["error"]["code"], "bad_prefix");
    assert_eq!(n.get("/v1/lookup/abc").json()["error"]["code"], "bad_prefix", "audit #10: at least 4 hex");
    // Audit #12: JSON arrays apply the per-record size limit and strict parsing.
    let dup = good.replacen("{\"author\"", "{\"kind\":\"profile\",\"author\"", 1);
    let padded = good.replacen('{', &format!("{{{}", " ".repeat(131_072)), 1);
    let v = n.post("/v1/records", &format!("[{good},{dup},{padded}]")).json();
    let outs: Vec<&str> = v["results"].as_array().unwrap().iter().map(|r| r["outcome"].as_str().unwrap()).collect();
    assert_eq!(outs, ["duplicate", "invalid", "invalid"], "{v}");
    assert!(v["results"][2]["error"].as_str().unwrap().contains("limit"));
    let e = n.get("/v1/search?now=abc");
    assert_eq!(e.json()["error"]["code"], "bad_query");
    assert_eq!(n.get("/v1/nothing").status, 404);
}

#[test]
fn f_operator_redaction_and_static_files() {
    let dir = tmpdir("redact");
    let (demo, policy) = demo_files(&dir);
    let web = dir.join("web");
    std::fs::create_dir_all(&web).unwrap();
    std::fs::write(web.join("app.wasm"), b"\0asm\x01\0\0\0").unwrap();
    std::fs::write(web.join("index.html"), "<!doctype html><title>t</title>").unwrap();
    let key = dir.join("operator.json");
    let st = std::process::Command::new(BIN).arg("keygen").arg("--out").arg(&key).output().unwrap();
    assert!(st.status.success());
    let operator = String::from_utf8(st.stdout).unwrap().trim().to_string();
    let admin = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    let args = |extra: bool| {
        let mut v = vec!["--policy".to_string(), policy.display().to_string(), "--web".into(), web.display().to_string()];
        if extra {
            v.extend(["--admin".into(), admin.to_string(), "--operator-key".into(), key.display().to_string()]);
            v.extend(["--import".into(), demo.display().to_string()]);
        }
        v
    };
    let mut n = spawn(&dir.join("a"), &args(true));
    let wasm = n.get("/app.wasm");
    assert_eq!(wasm.status, 200);
    assert_eq!(wasm.header("content-type"), Some("application/wasm"));
    assert_eq!(n.get("/").status, 200);
    // Pick a review with a body.
    let text = std::fs::read_to_string(&demo).unwrap();
    let review = text
        .lines()
        .map(|l| Record::from_json_str(l).unwrap())
        .find(|r| r.kind() == "review" && r.has_body() && n.get(&format!("/v1/records/{}", r.id())).body.contains("\"body\""))
        .expect("a review");
    let product = review.body().unwrap()["product"].as_str().unwrap().to_string();
    let secret_text = review.body().unwrap()["text"].as_str().unwrap().to_string();
    // Audit #5: the admin endpoint needs the token, no Origin, the admin Host, and JSON.
    let token = std::fs::read_to_string(dir.join("a").join("admin-token")).unwrap().trim().to_string();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join("a").join("admin-token")).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "token file is private");
    }
    let ad = admin.to_string();
    let bearer = format!("Bearer {token}");
    let body = json!({"id": review.id().to_hex(), "reason": "personal_data"}).to_string();
    let call = |hdrs: &[(&str, &str)]| http_with(&ad, "POST", "/admin/redact", Some(&body), hdrs);
    let good: Vec<(&str, &str)> = vec![("Host", ad.as_str()), ("Content-Type", "application/json"), ("Authorization", bearer.as_str())];
    let r = call(&good[..2]);
    assert_eq!((r.status, r.json()["error"]["code"].clone()), (401, json!("unauthorized")));
    let r = call(&[good[0], good[1], ("Authorization", "Bearer wrong")]);
    assert_eq!(r.status, 401);
    let r = call(&[good[0], good[1], good[2], ("Origin", "https://evil.example")]);
    assert_eq!((r.status, r.json()["error"]["code"].clone()), (403, json!("cross_origin")));
    let r = call(&[("Host", "evil.example:80"), good[1], good[2]]);
    assert_eq!((r.status, r.json()["error"]["code"].clone()), (403, json!("bad_host")));
    let r = call(&[good[0], ("Content-Type", "text/plain"), good[2]]);
    assert_eq!((r.status, r.json()["error"]["code"].clone()), (415, json!("bad_content_type")));
    assert!(n.get(&format!("/v1/records/{}", review.id())).body.contains("\"body\""), "refused calls changed nothing");
    let r = call(&good);
    assert_eq!(r.status, 200, "{}", r.body);
    let v = r.json();
    assert_eq!(v["body_dropped"], true);
    assert_eq!(v["redact"]["author"], operator.as_str());
    assert_eq!(v["redact"]["body"]["product"], product.as_str());
    let got = n.get(&format!("/v1/records/{}", review.id())).body;
    assert!(!got.contains("\"body\""), "body removed: {got}");
    // Control kinds are refused.
    let attest = text.lines().map(|l| Record::from_json_str(l).unwrap()).find(|r| r.kind() == "attest.identity").unwrap();
    let body2 = json!({"id": attest.id().to_hex(), "reason": "other"}).to_string();
    let r = http_with(&ad, "POST", "/admin/redact", Some(&body2), &good);
    assert_eq!(r.status, 409, "{}", r.body);
    // The body stays removed after a restart and is gone from disk.
    n.stop();
    let disk = std::fs::read_to_string(dir.join("a").join("records.jsonl")).unwrap();
    assert!(!disk.contains(&secret_text), "removed text is still on disk");
    let n2 = spawn(&dir.join("a"), &args(false));
    assert!(!n2.get(&format!("/v1/records/{}", review.id())).body.contains("\"body\""));
    // A peer that has the body cannot push it back.
    let r = n2.post("/v1/records", &review.to_json_string());
    assert_eq!(r.json()["results"][0]["outcome"], "duplicate");
    assert!(!n2.get(&format!("/v1/records/{}", review.id())).body.contains("\"body\""));
}

#[test]
fn g_related_set_and_lookup() {
    let dir = tmpdir("related");
    let (demo, policy) = demo_files(&dir);
    let n = spawn(&dir.join("a"), &["--policy".into(), policy.display().to_string(), "--import".into(), demo.display().to_string()]);
    let key = first_product_key(&n);
    let r = n.get(&format!("/v1/products/{}/records?now={}", enc(&key), perch_demo::DEMO_NOW));
    assert_eq!(r.status, 200);
    assert_eq!(r.header("content-type"), Some("application/x-ndjson"));
    let lines: Vec<&str> = r.body.lines().collect();
    assert!(!lines.is_empty());
    for l in &lines {
        Record::from_json_str(l).expect("every line is a valid record");
    }
    let prefix = &perch_core::ids::encode_hex(&perch_core::sha256(key.as_bytes()))[..4];
    let l = n.get(&format!("/v1/lookup/{prefix}?now={}", perch_demo::DEMO_NOW));
    assert_eq!(l.status, 200);
    for line in &lines {
        assert!(l.body.contains(line), "lookup bucket contains the related set");
    }
}

#[test]
fn j_rate_limits_apply_to_loopback() {
    let dir = tmpdir("limits");
    let n = spawn(&dir.join("a"), &[]);
    // Audit #14: POST is limited to 2 per second, burst 10, for loopback too.
    let codes: Vec<u16> = (0..15).map(|_| n.post("/v1/records", "").status).collect();
    assert!(codes.iter().filter(|c| **c == 429).count() >= 3, "{codes:?}");
    let r = n.post("/v1/records", "");
    if r.status == 429 {
        assert_eq!(r.json()["error"]["code"], "rate_limited");
    }
    // Audit #10: heavy GETs are limited per address too; light ones are not.
    let codes: Vec<u16> = (0..60).map(|_| n.get("/v1/stats").status).collect();
    assert!(codes.contains(&429), "{codes:?}");
    assert!(codes.iter().take(30).all(|c| *c == 200), "burst of 40 is allowed: {codes:?}");
    assert!((0..30).all(|_| n.get("/v1/status").status == 200));
}
