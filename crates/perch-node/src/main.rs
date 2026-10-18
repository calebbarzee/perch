//! `perch-node` command line (ARCHITECTURE 8.3, docs/api.md).

use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Parser, Subcommand};
use perch_core::{AuthorId, Keypair};
use perch_store::Store;
use perch_node::config::ServeConfig;
use perch_node::node::{BuildInfo, Node};
use perch_views::{ATTESTOR_KINDS, Policy, TrustedAttestor};
use serde_json::{Value, json};

#[derive(Parser)]
#[command(name = "perch-node", version, about = "Perch node: stores, verifies, and relays signed records")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run a node.
    Serve(ServeArgs),
    /// Generate a key pair and print it as JSON.
    Keygen {
        /// Write the key to this file (mode 0600) instead of stdout.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Write the signed demo dataset.
    Demo {
        /// Output JSONL file.
        #[arg(long)]
        out: PathBuf,
        /// Also write a default policy that trusts the demo attestors (without `now`).
        #[arg(long)]
        policy_out: Option<PathBuf>,
        /// Also write the scenario manifest (names of actors, products, records).
        #[arg(long)]
        manifest_out: Option<PathBuf>,
        /// small (about 300 records), medium (about 5,000), or large (about 100,000).
        #[arg(long, default_value = "small")]
        size: String,
        #[arg(long, default_value_t = perch_demo::DEMO_SEED)]
        seed: u64,
    },
}

#[derive(clap::Args)]
struct ServeArgs {
    /// Address for the HTTP API and sync (port 0 picks a free port).
    #[arg(long, default_value = "127.0.0.1:7700")]
    listen: SocketAddr,
    /// Directory for records.jsonl and removed.txt.
    #[arg(long, default_value = "./data")]
    data: PathBuf,
    /// Peer to keep connected, as ws://host:port/v1/sync. Repeatable.
    #[arg(long = "peer")]
    peers: Vec<String>,
    /// Trusted attestor (author hex) for all attestation kinds. Repeatable.
    #[arg(long = "attestor")]
    attestors: Vec<String>,
    /// Default policy file (JSON; `now` optional). --attestor entries are added to it.
    #[arg(long)]
    policy: Option<PathBuf>,
    /// Directory of the web app, served at /.
    #[arg(long)]
    web: Option<PathBuf>,
    /// Extra removal list (one record ID per line). Repeatable. <data>/removed.txt is always read.
    #[arg(long = "removed")]
    removed: Vec<PathBuf>,
    /// Loopback address for operator controls (/admin/*). Off when absent.
    #[arg(long)]
    admin: Option<SocketAddr>,
    /// Operator key file (secret hex or keygen JSON), used only to sign `redact` records.
    #[arg(long)]
    operator_key: Option<PathBuf>,
    /// URL other nodes use to reach this node. Default: ws://<listen>/v1/sync unless listening on all interfaces.
    #[arg(long)]
    public_url: Option<String>,
    /// Admit every author in full instead of applying admission tiers.
    #[arg(long)]
    open_admission: bool,
    /// Seconds between heads re-exchanges on each session.
    #[arg(long, default_value_t = 600)]
    refresh_secs: u64,
    /// Maximum outbound connections.
    #[arg(long, default_value_t = 8)]
    max_outbound: usize,
    /// Import a JSONL file at startup. Repeatable.
    #[arg(long = "import")]
    imports: Vec<PathBuf>,
    /// Tier-1 log limit in records, per person.
    #[arg(long, default_value_t = 1_000_000)]
    tier1_records: u64,
    /// Tier-2 log limit in records, per author.
    #[arg(long, default_value_t = 1_000)]
    tier2_records: u64,
    /// Tier-3 log limit in records, per author.
    #[arg(long, default_value_t = 100)]
    tier3_records: u64,
    /// Node-wide byte budget for all tier-3 logs.
    #[arg(long, default_value_t = 1 << 30)]
    tier3_budget: u64,
    /// Header with the client address, trusted only on loopback connections
    /// (for a reverse proxy on the same host), for example x-forwarded-for.
    #[arg(long)]
    trust_proxy_header: Option<String>,
    /// Which value of --trust-proxy-header is the client, counted from the right (1 = added by the nearest proxy).
    #[arg(long, default_value_t = 1)]
    trust_proxy_hops: usize,
    /// Extra trusted CA certificates (PEM) for dialing wss:// peers, added to the Mozilla roots.
    #[arg(long)]
    ca_file: Option<PathBuf>,
}

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("perch-node: {msg}");
    std::process::exit(2);
}

fn build_hash() -> String {
    std::env::current_exe()
        .and_then(std::fs::read)
        .map(|b| perch_core::ids::encode_hex(&perch_core::sha256(&b)))
        .unwrap_or_else(|_| "unknown".into())
}

fn load_policy(file: Option<&Path>, attestors: &[AuthorId]) -> Result<Policy, String> {
    let mut p = match file {
        Some(f) => {
            let text = std::fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?;
            let mut v: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", f.display()))?;
            if let Some(o) = v.as_object_mut() {
                o.entry("now").or_insert(json!(0));
            }
            Policy::from_value(v).map_err(|e| format!("{}: {e}", f.display()))?
        }
        None => perch_views::default_policy(0, &[]),
    };
    for a in attestors {
        if p.attestor(a).is_none() {
            p.trusted_attestors.push(TrustedAttestor {
                author: *a,
                kinds: ATTESTOR_KINDS.iter().map(|s| (*s).to_string()).collect(),
                not_after_seq: None,
            });
        }
    }
    p.trusted_attestors.sort_by(|a, b| a.author.cmp(&b.author));
    p.validate().map_err(|e| e.to_string())?;
    Ok(p)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = term => {}
    }
}

async fn serve(a: ServeArgs) {
    let attestors: Vec<AuthorId> = a
        .attestors
        .iter()
        .map(|s| AuthorId::from_hex(s).unwrap_or_else(|e| fail(format!("--attestor {s}: {e}"))))
        .collect();
    let policy = load_policy(a.policy.as_deref(), &attestors).unwrap_or_else(|e| fail(e));
    for p in &a.peers {
        if !perch_sync::message::is_valid_address(p) {
            fail(format!("--peer {p}: expected ws://host:port/v1/sync or wss://..."));
        }
    }
    if let Some(admin) = a.admin
        && !admin.ip().is_loopback()
    {
        fail("--admin must be a loopback address (127.0.0.1 or ::1)");
    }
    let listener = tokio::net::TcpListener::bind(a.listen).await.unwrap_or_else(|e| fail(format!("bind {}: {e}", a.listen)));
    let listen = listener.local_addr().unwrap_or(a.listen);
    let public_url = a.public_url.clone().or_else(|| {
        if listen.ip().is_unspecified() { None } else { Some(format!("ws://{listen}/v1/sync")) }
    });
    let config = ServeConfig {
        listen,
        data: a.data.clone(),
        peers: a.peers.clone(),
        web: a.web.clone(),
        removed_files: a.removed.clone(),
        admin: a.admin,
        operator_key: a.operator_key.clone(),
        public_url,
        open_admission: a.open_admission,
        refresh_secs: a.refresh_secs.max(5),
        max_outbound: a.max_outbound,
        imports: a.imports.clone(),
        tiers: perch_node::admission::TierLimits {
            t1_records: a.tier1_records,
            t2_records: a.tier2_records,
            t3_records: a.tier3_records,
            t3_budget: a.tier3_budget,
            ..Default::default()
        },
        trust_proxy_header: a.trust_proxy_header.as_ref().map(|h| h.to_ascii_lowercase()),
        trust_proxy_hops: a.trust_proxy_hops.max(1),
        ca_file: a.ca_file.clone(),
    };
    let build = BuildInfo { version: env!("CARGO_PKG_VERSION"), build_hash: build_hash() };
    let node = Arc::new(Node::open(config, policy, build).unwrap_or_else(|e| fail(e)));
    for f in &a.imports {
        let text = std::fs::read_to_string(f).unwrap_or_else(|e| fail(format!("--import {}: {e}", f.display())));
        let mut records = Vec::new();
        let mut invalid = 0;
        for (_, r) in perch_core::parse_jsonl(&text) {
            match r {
                Ok(r) => records.push(r),
                Err(_) => invalid += 1,
            }
        }
        let n = records.len();
        let outcomes = node.submit(records).unwrap_or_else(|e| fail(e));
        let inserted = outcomes.iter().filter(|o| o.changed()).count();
        tracing::info!(file = %f.display(), records = n, inserted, invalid, "imported");
    }
    for p in node.config.peers.clone() {
        tokio::spawn(perch_node::peers::keep_connected(node.clone(), p));
    }
    tokio::spawn(perch_node::peers::fill_slots(node.clone()));
    {
        let node = node.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(std::time::Duration::from_secs(node.config.refresh_secs));
            every.tick().await;
            loop {
                every.tick().await;
                let n = node.clone();
                let _ = tokio::task::spawn_blocking(move || n.maintain()).await;
            }
        });
    }
    if let Ok(pid) = std::env::var("PERCH_EXIT_WITH_PID")
        && let Ok(pid) = pid.trim().parse::<u32>()
    {
        // Test harnesses: exit when the parent process is gone (Linux /proc).
        let node = node.clone();
        tokio::spawn(async move {
            let proc_dir = std::path::PathBuf::from(format!("/proc/{pid}"));
            if !std::path::Path::new("/proc/self").exists() {
                return;
            }
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if !proc_dir.exists() {
                    tracing::info!(pid, "parent process exited; stopping");
                    let _ = node.write().flush();
                    std::process::exit(0);
                }
            }
        });
    }
    if let Some(admin) = node.config.admin {
        let l = tokio::net::TcpListener::bind(admin).await.unwrap_or_else(|e| fail(format!("bind --admin {admin}: {e}")));
        let admin = l.local_addr().unwrap_or(admin);
        let (token, path) = perch_node::admin::new_token(&node.config.data).unwrap_or_else(|e| fail(format!("admin token: {e}")));
        println!("perch-node admin on http://{admin} (bearer token {token}, also in {})", path.display());
        let r = perch_node::admin::router(perch_node::admin::Admin { node: node.clone(), token: Arc::new(token), addr: admin });
        tracing::info!(%admin, token_file = %path.display(), "operator controls listening");
        tokio::spawn(async move {
            let _ = axum::serve(l, r).await;
        });
    }
    let app = perch_node::http::router(node.clone());
    println!("perch-node listening on http://{listen} (sync: ws://{listen}/v1/sync, node {})", node.node_id);
    let _ = std::io::stdout().flush();
    tracing::info!(%listen, records = node.read().len(), policy_hash = %node.policy_hash, "serving");
    let server = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).with_graceful_shutdown(shutdown_signal());
    if let Err(e) = server.await {
        tracing::error!(error = %e, "server error");
    }
    if let Err(e) = node.write().flush() {
        tracing::error!(error = %e, "final flush failed");
    }
    tracing::info!("stopped");
    std::process::exit(0);
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Serve(a) => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
                )
                .with_writer(std::io::stderr)
                .init();
            let rt = tokio::runtime::Runtime::new().unwrap_or_else(|e| fail(e));
            rt.block_on(serve(a));
        }
        Cmd::Keygen { out } => {
            let kp = Keypair::generate();
            let v = json!({ "secret": kp.seed_hex(), "author": kp.author().to_hex() });
            match out {
                Some(p) => {
                    write_private(&p, &format!("{v:#}\n")).unwrap_or_else(|e| fail(format!("{}: {e}", p.display())));
                    println!("{}", kp.author().to_hex());
                }
                None => println!("{v:#}"),
            }
        }
        Cmd::Demo { out, policy_out, manifest_out, size, seed } => {
            let size = perch_demo::Size::parse(&size).unwrap_or_else(|| fail("--size must be small, medium, or large"));
            let demo = perch_demo::generate_demo(seed, size);
            std::fs::write(&out, demo.to_jsonl()).unwrap_or_else(|e| fail(format!("{}: {e}", out.display())));
            if let Some(p) = policy_out {
                let mut pol = perch_views::default_policy(0, &demo.attestors);
                if let Some(b) = demo.attestors.get(1)
                    && let Some(t) = pol.trusted_attestors.iter_mut().find(|t| t.author == *b)
                {
                    t.not_after_seq = Some(demo.attestor_b_honest_through_seq);
                }
                let mut v = pol.to_value();
                if let Some(o) = v.as_object_mut() {
                    o.remove("now");
                }
                std::fs::write(&p, format!("{v:#}\n")).unwrap_or_else(|e| fail(format!("{}: {e}", p.display())));
            }
            if let Some(m) = manifest_out {
                std::fs::write(&m, format!("{:#}\n", demo.manifest())).unwrap_or_else(|e| fail(format!("{}: {e}", m.display())));
            }
            eprintln!("wrote {} records to {} (now = {})", demo.records.len(), out.display(), demo.now);
        }
    }
}

fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o.open(path)?;
    f.write_all(text.as_bytes())
}
