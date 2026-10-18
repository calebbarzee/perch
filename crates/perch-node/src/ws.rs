//! Drives one `perch_sync::Session` over a WebSocket (inbound via axum,
//! outbound via tokio-tungstenite).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use perch_sync::message::{ErrorCode, Message, limits as proto};
use perch_store::Store;
use perch_sync::{Role, Session};
use tokio::sync::{Notify, broadcast, mpsc};

use crate::config::limits;
use crate::node::Node;

/// A frame from the transport.
pub enum In {
    Text(String),
    Binary,
    /// Ping or pong: proves the connection is alive.
    Activity,
}

/// A frame to the transport.
pub enum Out {
    Text(String),
    Ping,
    Close,
}

/// How a session ended.
#[derive(Debug, Default)]
pub struct End {
    pub reached_live: bool,
    pub self_connection: bool,
    pub reason: String,
}

/// Token bucket for rate limits.
struct Bucket {
    rate: f64,
    burst: f64,
    level: f64,
    at: Instant,
}

impl Bucket {
    fn new(rate: f64, burst: f64) -> Bucket {
        Bucket { rate, burst, level: burst, at: Instant::now() }
    }

    fn take(&mut self, n: f64) -> bool {
        let now = Instant::now();
        self.level = (self.level + now.duration_since(self.at).as_secs_f64() * self.rate).min(self.burst);
        self.at = now;
        if self.level >= n {
            self.level -= n;
            true
        } else {
            false
        }
    }
}

/// The client IP of a session counts as private for address learning.
fn is_private_ip(ip: std::net::IpAddr) -> bool {
    perch_sync::address::is_private_host(&ip.to_string())
}

/// Bytes of text frames queued for the writer per connection (audit #14).
pub const OUT_QUEUE_BYTES: usize = 16 * 1024 * 1024;

/// Sender side of a connection's outbound queue, bounded in bytes.
#[derive(Clone)]
pub struct OutTx {
    tx: mpsc::Sender<(Out, Option<tokio::sync::OwnedSemaphorePermit>)>,
    bytes: Arc<tokio::sync::Semaphore>,
}

/// Receiver side; the permit is released after the frame is written.
pub type OutRx = mpsc::Receiver<(Out, Option<tokio::sync::OwnedSemaphorePermit>)>;

/// An outbound queue of at most [`OUT_QUEUE_BYTES`] of text.
pub fn out_queue() -> (OutTx, OutRx) {
    let (tx, rx) = mpsc::channel(256);
    (OutTx { tx, bytes: Arc::new(tokio::sync::Semaphore::new(OUT_QUEUE_BYTES)) }, rx)
}

impl OutTx {
    pub async fn send(&self, o: Out) -> bool {
        let permit = match &o {
            Out::Text(t) => {
                let n = t.len().clamp(1, OUT_QUEUE_BYTES) as u32;
                match self.bytes.clone().acquire_many_owned(n).await {
                    Ok(p) => Some(p),
                    Err(_) => return false,
                }
            }
            _ => None,
        };
        self.tx.send((o, permit)).await.is_ok()
    }
}

async fn send_all(tx: &OutTx, msgs: Vec<Message>) -> bool {
    for m in msgs {
        if !tx.send(Out::Text(m.to_json())).await {
            return false;
        }
    }
    true
}

fn update_info(node: &Node, s: &Session, behind_ahead: Option<(usize, usize)>) {
    let mut peers = node.peers.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = peers.get_mut(&s.id) {
        p.state = s.state().as_str();
        p.messages_sent = s.stats.messages_sent;
        p.messages_received = s.stats.messages_received;
        p.records_sent = s.stats.records_sent;
        p.records_received = s.stats.records_received;
        p.score = s.score();
        if let Some(h) = s.peer_hello() {
            p.node = Some(h.node.clone());
            p.role = Some(h.role.as_str());
        }
        if let Some((b, a)) = behind_ahead {
            p.behind = b;
            p.ahead = a;
        }
    }
}

/// After `hello`: if another session to the same peer process exists, keep
/// the one dialed by the peer with the lower node ID. Returns true if this
/// session must close.
fn duplicate_check(node: &Node, s: &Session) -> bool {
    let Some(peer) = s.peer_hello().map(|h| h.node.clone()) else { return false };
    let me = node.node_id.clone();
    let peers = node.peers.lock().unwrap_or_else(|e| e.into_inner());
    let dialer = |outbound: bool| if outbound { me.clone() } else { peer.clone() };
    for (id, p) in peers.iter() {
        if *id != s.id && p.node.as_deref() == Some(peer.as_str()) {
            let keep_this = dialer(s.is_outbound()) < dialer(p.outbound);
            if keep_this {
                p.kill.notify_one();
                return false;
            }
            return true;
        }
    }
    false
}

/// Runs one session until the transport closes or the session ends. The
/// session must already be registered ([`Node::register_session`] or
/// [`Node::reserve_inbound`]); it is unregistered on return.
#[allow(clippy::too_many_arguments)]
pub async fn drive(
    node: Arc<Node>,
    id: u64,
    kill: Arc<Notify>,
    outbound: bool,
    url: Option<String>,
    remote: Option<SocketAddr>,
    mut rx: mpsc::Receiver<In>,
    tx: OutTx,
) -> End {
    let mut s = Session::new(id, node.sync_cfg.clone(), outbound);
    s.set_shareable_addresses(node.shareable(url.as_deref()));
    let mut changes_rx = node.changes.subscribe();
    let mut end = End::default();
    let source_private = match remote {
        Some(r) => is_private_ip(r.ip()),
        None => url.as_deref().is_some_and(perch_sync::address::is_private_address),
    };
    let first = s.start();
    if !send_all(&tx, first).await {
        end.reason = "transport closed".into();
    }
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    tick.tick().await;
    let mut ticks: u64 = 0;
    let mut last_activity = Instant::now();
    let mut msg_bucket = Bucket::new(limits::MSG_RATE, limits::MSG_BURST);
    let mut byte_bucket = Bucket::new(limits::BYTE_RATE, limits::BYTE_BURST);
    let mut limited_since: Option<Instant> = None;
    let refresh_every = (node.config.refresh_secs / 5).max(1);
    let ping_every = limits::PING_SECS / 5;
    while end.reason.is_empty() {
        tokio::select! {
            _ = kill.notified() => {
                let m = Message::error(ErrorCode::DuplicateConnection, "another session to this peer exists");
                let _ = tx.send(Out::Text(m.to_json())).await;
                end.reason = "duplicate connection".into();
            }
            m = rx.recv() => match m {
                None => end.reason = "transport closed".into(),
                Some(In::Activity) => last_activity = Instant::now(),
                Some(In::Binary) => {
                    let m = Message::error(ErrorCode::BadMessage, "binary frames are not part of the protocol");
                    let _ = tx.send(Out::Text(m.to_json())).await;
                    end.reason = "binary frame".into();
                }
                Some(In::Text(text)) => {
                    last_activity = Instant::now();
                    if !msg_bucket.take(1.0) || !byte_bucket.take(text.len() as f64) {
                        let since = *limited_since.get_or_insert_with(Instant::now);
                        if since.elapsed().as_secs() >= limits::LIMITED_CLOSE_SECS {
                            let m = Message::error(ErrorCode::Misbehavior, "rate limit exceeded for 10 seconds");
                            let _ = tx.send(Out::Text(m.to_json())).await;
                            end.reason = "rate limited".into();
                        } else {
                            let m = Message::error(ErrorCode::RateLimited, "message dropped: rate limit");
                            let _ = tx.send(Out::Text(m.to_json())).await;
                        }
                        continue;
                    }
                    limited_since = None;
                    let had_hello = s.peer_hello().is_some();
                    let (res, changes, flush_err) = {
                        let mut st = node.write();
                        let res = s.on_message(&mut *st, &text);
                        let changes = st.drain_changes();
                        let flush_err = if changes.is_empty() { None } else { st.flush().err() };
                        (res, changes, flush_err)
                    };
                    if let Some(e) = flush_err {
                        tracing::error!(error = %e, "persistence write failed");
                    }
                    match res {
                        Ok(msgs) => {
                            if !send_all(&tx, msgs).await {
                                end.reason = "transport closed".into();
                            }
                        }
                        Err(e) => {
                            tracing::info!(session = id, url = ?url, remote = ?remote, error = %e, "closing session");
                            let _ = tx.send(Out::Text(e.to_message().to_json())).await;
                            if e.code == ErrorCode::SelfConnection {
                                end.self_connection = true;
                            }
                            end.reason = e.to_string();
                        }
                    }
                    node.publish_changes(Some(id), changes);
                    if s.state() == perch_sync::State::Live {
                        end.reached_live = true;
                    }
                    if !had_hello && let Some(h) = s.peer_hello() {
                        let h = h.clone();
                        update_info(&node, &s, None);
                        if duplicate_check(&node, &s) {
                            let m = Message::error(ErrorCode::DuplicateConnection, "another session to this peer exists");
                            let _ = tx.send(Out::Text(m.to_json())).await;
                            end.reason = "duplicate connection".into();
                        } else if h.role == Role::Node && let Some(l) = &h.listen {
                            node.addresses.lock().unwrap_or_else(|e| e.into_inner()).add_learned(l, source_private);
                        }
                    }
                    let learned = s.take_learned_addresses();
                    if !learned.is_empty() {
                        let mut book = node.addresses.lock().unwrap_or_else(|e| e.into_inner());
                        for a in learned {
                            book.add_learned(&a, source_private);
                        }
                    }
                    if s.is_closed() && end.reason.is_empty() {
                        end.reason = "peer closed the session".into();
                    }
                    update_info(&node, &s, None);
                }
            },
            b = changes_rx.recv() => {
                let msgs = match b {
                    Ok(batch) => {
                        let st = node.read();
                        s.on_changes(&*st, &batch.changes, batch.origin)
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(session = id, missed = n, "session lagged behind store changes; re-exchanging heads");
                        let st = node.read();
                        s.refresh(&*st)
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        end.reason = "node stopping".into();
                        Vec::new()
                    }
                };
                if !send_all(&tx, msgs).await {
                    end.reason = "transport closed".into();
                }
            }
            _ = tick.tick() => {
                ticks += 1;
                if last_activity.elapsed().as_secs() >= limits::IDLE_CLOSE_SECS {
                    end.reason = "idle timeout".into();
                    continue;
                }
                if ticks % ping_every == 0 && !tx.send(Out::Ping).await {
                    end.reason = "transport closed".into();
                }
                if ticks % refresh_every == 0 {
                    s.set_shareable_addresses(node.shareable(url.as_deref()));
                    let msgs = { let st = node.read(); s.refresh(&*st) };
                    if !send_all(&tx, msgs).await {
                        end.reason = "transport closed".into();
                    }
                }
                let ba = { let st = node.read(); s.behind_ahead(&*st) };
                update_info(&node, &s, Some(ba));
            }
        }
    }
    let _ = tx.send(Out::Close).await;
    node.unregister_session(id);
    tracing::debug!(session = id, reason = %end.reason, "session ended");
    end
}

/// Maximum WebSocket message size accepted on sync connections.
pub const MAX_WS_MESSAGE: usize = proto::MAX_MESSAGE_BYTES;

/// Runs a session over an accepted axum WebSocket. `id` was reserved by
/// [`Node::reserve_inbound`].
pub async fn serve_axum(node: Arc<Node>, socket: axum::extract::ws::WebSocket, remote: SocketAddr, id: u64, kill: Arc<Notify>) {
    use axum::extract::ws::Message as M;
    let (mut sink, mut stream) = socket.split();
    let (in_tx, in_rx) = mpsc::channel::<In>(4);
    let (out_tx, mut out_rx) = out_queue();
    let writer = tokio::spawn(async move {
        while let Some((o, permit)) = out_rx.recv().await {
            let r = match o {
                Out::Text(t) => sink.send(M::Text(t.into())).await,
                Out::Ping => sink.send(M::Ping(Default::default())).await,
                Out::Close => {
                    let _ = sink.send(M::Close(None)).await;
                    break;
                }
            };
            drop(permit);
            if r.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });
    let reader = async move {
        while let Some(m) = stream.next().await {
            let f = match m {
                Ok(M::Text(t)) => In::Text(t.as_str().to_owned()),
                Ok(M::Binary(_)) => In::Binary,
                Ok(M::Ping(_) | M::Pong(_)) => In::Activity,
                Ok(M::Close(_)) | Err(_) => break,
            };
            if in_tx.send(f).await.is_err() {
                break;
            }
        }
    };
    let driver = drive(node, id, kill, false, None, Some(remote), in_rx, out_tx);
    tokio::pin!(driver);
    tokio::select! {
        _ = &mut driver => {}
        _ = reader => {
            // Transport closed: let the driver notice and clean up.
            driver.await;
        }
    }
    let _ = writer.await;
}

/// Splits a `ws://host:port/path` URL into host and port.
fn host_port(url: &str) -> Result<(String, u16, bool), String> {
    let tls = url.starts_with("wss://");
    let rest = url.strip_prefix("wss://").or_else(|| url.strip_prefix("ws://")).ok_or("not a ws:// or wss:// URL")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let (h, after) = v6.split_once(']').ok_or("bad IPv6 host")?;
        (h.to_string(), after.strip_prefix(':').map(str::to_string))
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), Some(p.to_string())),
            None => (authority.to_string(), None),
        }
    };
    let port = match port {
        Some(p) => p.parse::<u16>().map_err(|_| "bad port".to_string())?,
        None => if tls { 443 } else { 80 },
    };
    if host.is_empty() {
        return Err("empty host".into());
    }
    Ok((host, port, tls))
}

/// Resolves `host:port`. Unless `private_ok`, refuses if any resolved
/// address is private, loopback, or link-local (audit #15). The caller
/// connects to a returned address, so a second lookup cannot change it.
pub async fn resolve_checked(host: &str, port: u16, private_ok: bool) -> Result<Vec<SocketAddr>, String> {
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| format!("resolve {host}: {e}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("{host} has no addresses"));
    }
    if !private_ok && let Some(a) = addrs.iter().find(|a| perch_sync::address::is_private_ip(a.ip())) {
        return Err(format!("{host} resolves to the non-public address {}", a.ip()));
    }
    Ok(addrs)
}

/// Dials `url` and runs a session over it. `private_ok` allows private
/// resolved addresses (configured peers, or addresses learned on a private network).
pub async fn dial(node: Arc<Node>, url: &str, private_ok: bool) -> Result<End, String> {
    use tokio_tungstenite::tungstenite::Message as M;
    use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
    let (host, port, tls) = host_port(url)?;
    if tls {
        return Err("wss:// peers need TLS support, which this build does not include".into());
    }
    let addrs = resolve_checked(&host, port, private_ok).await?;
    let mut last_err = String::from("no address");
    let mut stream = None;
    for a in addrs.iter().take(4) {
        match tokio::time::timeout(Duration::from_secs(10), tokio::net::TcpStream::connect(a)).await {
            Ok(Ok(s)) => {
                stream = Some(s);
                break;
            }
            Ok(Err(e)) => last_err = e.to_string(),
            Err(_) => last_err = "connect timeout".into(),
        }
    }
    let stream = stream.ok_or(last_err)?;
    let cfg = WebSocketConfig::default().max_message_size(Some(MAX_WS_MESSAGE)).max_frame_size(Some(MAX_WS_MESSAGE));
    let (ws, _) = tokio::time::timeout(Duration::from_secs(10), tokio_tungstenite::client_async_with_config(url, stream, Some(cfg)))
        .await
        .map_err(|_| "handshake timeout".to_string())?
        .map_err(|e| e.to_string())?;
    let (mut sink, mut stream) = ws.split();
    let (in_tx, in_rx) = mpsc::channel::<In>(4);
    let (out_tx, mut out_rx) = out_queue();
    let writer = tokio::spawn(async move {
        while let Some((o, permit)) = out_rx.recv().await {
            let r = match o {
                Out::Text(t) => sink.send(M::Text(t.into())).await,
                Out::Ping => sink.send(M::Ping(Default::default())).await,
                Out::Close => {
                    let _ = sink.send(M::Close(None)).await;
                    break;
                }
            };
            drop(permit);
            if r.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });
    let reader = async move {
        while let Some(m) = stream.next().await {
            let f = match m {
                Ok(M::Text(t)) => In::Text(t.as_str().to_owned()),
                Ok(M::Binary(_)) => In::Binary,
                Ok(M::Ping(_) | M::Pong(_) | M::Frame(_)) => In::Activity,
                Ok(M::Close(_)) | Err(_) => break,
            };
            if in_tx.send(f).await.is_err() {
                break;
            }
        }
    };
    let (id, kill) = node.register_session(true, Some(url.to_string()), None);
    let driver = drive(node, id, kill, true, Some(url.to_string()), None, in_rx, out_tx);
    tokio::pin!(driver);
    let end = tokio::select! {
        e = &mut driver => e,
        _ = reader => driver.await,
    };
    let _ = writer.await;
    Ok(end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_port_parsing() {
        assert_eq!(host_port("ws://127.0.0.1:7701/v1/sync").unwrap(), ("127.0.0.1".into(), 7701, false));
        assert_eq!(host_port("wss://node.example.org/v1/sync").unwrap(), ("node.example.org".into(), 443, true));
        assert_eq!(host_port("ws://[::1]:9/v1/sync").unwrap(), ("::1".into(), 9, false));
        assert!(host_port("http://x").is_err());
    }

    /// Audit #14: the outbound queue holds at most OUT_QUEUE_BYTES of text.
    #[tokio::test]
    async fn outbound_queue_is_bounded_in_bytes() {
        let (tx, mut rx) = out_queue();
        let mb = "x".repeat(1024 * 1024);
        for _ in 0..16 {
            assert!(tx.send(Out::Text(mb.clone())).await);
        }
        let blocked = tokio::time::timeout(Duration::from_millis(200), tx.send(Out::Text(mb.clone()))).await;
        assert!(blocked.is_err(), "17th MiB waits for the writer");
        let (_, permit) = rx.recv().await.unwrap();
        drop(permit);
        let ok = tokio::time::timeout(Duration::from_millis(500), tx.send(Out::Text(mb))).await;
        assert!(matches!(ok, Ok(true)), "space is released after a write");
    }

    /// Audit #15: names and mapped addresses that resolve to private IPs are refused.
    #[tokio::test]
    async fn resolution_refuses_private_addresses() {
        for host in ["localhost", "127.0.0.1", "::ffff:127.0.0.1", "::1"] {
            let e = resolve_checked(host, 80, false).await.unwrap_err();
            assert!(e.contains("non-public"), "{host}: {e}");
            assert!(resolve_checked(host, 80, true).await.is_ok(), "{host} allowed when private_ok");
        }
        assert!(resolve_checked("8.8.8.8", 80, false).await.is_ok());
    }
}
