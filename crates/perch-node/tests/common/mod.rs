//! Helpers for live tests: spawn `perch-node`, talk HTTP/1.1 over std TCP.

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::Value;

pub const BIN: &str = env!("CARGO_BIN_EXE_perch-node");

/// A fresh temporary directory for one test.
pub fn tmpdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("perch-live-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A running node process.
pub struct NodeProc {
    pub child: Child,
    pub addr: String,
    pub data: PathBuf,
    pub log: PathBuf,
}

impl NodeProc {
    pub fn ws(&self) -> String {
        format!("ws://{}/v1/sync", self.addr)
    }

    /// Stops the process with SIGKILL (no graceful shutdown).
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// Stops the process with SIGTERM and waits.
    pub fn stop(&mut self) {
        let _ = Command::new("kill").arg("-TERM").arg(self.child.id().to_string()).status();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        self.kill();
    }

    pub fn get(&self, path: &str) -> Resp {
        http(&self.addr, "GET", path, None)
    }

    pub fn post(&self, path: &str, body: &str) -> Resp {
        http(&self.addr, "POST", path, Some(body))
    }

    pub fn status(&self) -> Value {
        self.get("/v1/status").json()
    }

    pub fn records(&self) -> u64 {
        self.status()["store"]["records"].as_u64().unwrap_or(0)
    }

    pub fn log_text(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

impl Drop for NodeProc {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Starts `perch-node serve` on a free port with `--data <data>` and extra args.
pub fn spawn(data: &Path, args: &[String]) -> NodeProc {
    std::fs::create_dir_all(data).unwrap();
    let log = data.with_extension("log");
    let logf = std::fs::File::create(&log).unwrap();
    let mut child = Command::new(BIN)
        .arg("serve")
        .arg("--listen")
        .arg("127.0.0.1:0")
        .arg("--data")
        .arg(data)
        .args(args)
        .env("RUST_LOG", "info")
        .stdout(Stdio::piped())
        .stderr(Stdio::from(logf))
        .spawn()
        .expect("spawn perch-node");
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut r = BufReader::new(stdout);
        let mut line = String::new();
        while r.read_line(&mut line).unwrap_or(0) > 0 {
            if let Some(i) = line.find("listening on http://") {
                let addr: String = line[i + 20..].chars().take_while(|c| !c.is_whitespace()).collect();
                let _ = tx.send(addr);
            }
            line.clear();
        }
    });
    let addr = rx.recv_timeout(Duration::from_secs(60)).unwrap_or_else(|_| {
        let _ = child.kill();
        panic!("perch-node did not start; log:\n{}", std::fs::read_to_string(&log).unwrap_or_default())
    });
    NodeProc { child, addr, data: data.to_path_buf(), log }
}

/// An HTTP response.
#[derive(Debug)]
pub struct Resp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Resp {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("{e}: {} {}", self.status, self.body))
    }
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

fn dechunk(mut b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let Some(pos) = b.windows(2).position(|w| w == b"\r\n") else { break };
        let size_line = std::str::from_utf8(&b[..pos]).unwrap_or("0");
        let size = usize::from_str_radix(size_line.split(';').next().unwrap_or("0").trim(), 16).unwrap_or(0);
        b = &b[pos + 2..];
        if size == 0 || b.len() < size {
            break;
        }
        out.extend_from_slice(&b[..size]);
        b = &b[(size + 2).min(b.len())..];
    }
    out
}

/// One HTTP/1.1 request with `Connection: close`.
pub fn http(addr: &str, method: &str, path: &str, body: Option<&str>) -> Resp {
    http_with(addr, method, path, body, &[("Host", addr), ("Content-Type", "application/x-ndjson")])
}

/// One HTTP/1.1 request with exactly the given headers (plus Connection and Content-Length).
pub fn http_with(addr: &str, method: &str, path: &str, body: Option<&str>, headers: &[(&str, &str)]) -> Resp {
    let mut s = TcpStream::connect(addr).unwrap_or_else(|e| panic!("connect {addr}: {e}"));
    s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    let body = body.unwrap_or("");
    let mut req = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes()).unwrap();
    s.write_all(body.as_bytes()).unwrap();
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(raw.len());
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let mut lines = head.split("\r\n");
    let status = lines.next().and_then(|l| l.split(' ').nth(1)).and_then(|c| c.parse().ok()).unwrap_or(0);
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
        .collect();
    let mut bytes = raw[(split + 4).min(raw.len())..].to_vec();
    if headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("transfer-encoding") && v.contains("chunked")) {
        bytes = dechunk(&bytes);
    }
    Resp { status, headers, body: String::from_utf8_lossy(&bytes).to_string() }
}

/// Polls `f` until it returns true or `timeout` passes.
pub fn wait_until(timeout: Duration, mut f: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    f()
}

/// The demo dataset and policy, written once per test directory.
pub fn demo_files(dir: &Path) -> (PathBuf, PathBuf) {
    let jsonl = dir.join("demo.jsonl");
    let policy = dir.join("policy.json");
    let st = Command::new(BIN)
        .args(["demo", "--out"])
        .arg(&jsonl)
        .arg("--policy-out")
        .arg(&policy)
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(st.success());
    (jsonl, policy)
}
