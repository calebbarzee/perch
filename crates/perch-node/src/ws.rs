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
use crate::node::{Node, PeerInfo};

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

async fn send_all(tx: &mpsc::Sender<Out>, msgs: Vec<Message>) -> bool {
    for m in msgs {
        if tx.send(Out::Text(m.to_json())).await.is_err() {
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

/// Runs one session until the transport closes or the session ends.
pub async fn drive(
    node: Arc<Node>,
    outbound: bool,
    url: Option<String>,
    remote: Option<SocketAddr>,
    mut rx: mpsc::Receiver<In>,
    tx: mpsc::Sender<Out>,
) -> End {
    let id = node.next_session_id();
    let mut s = Session::new(id, node.sync_cfg.clone(), outbound);
    s.set_shareable_addresses(node.shareable(url.as_deref()));
    let kill = Arc::new(Notify::new());
    node.peers.lock().unwrap_or_else(|e| e.into_inner()).insert(
        id,
        PeerInfo {
            session: id,
            outbound,
            url: url.clone(),
            remote,
            node: None,
            role: None,
            state: s.state().as_str(),
            messages_sent: 0,
            messages_received: 0,
            records_sent: 0,
            records_received: 0,
            behind: 0,
            ahead: 0,
            score: 0,
            kill: kill.clone(),
        },
    );
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
                if ticks % ping_every == 0 && tx.send(Out::Ping).await.is_err() {
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
    node.peers.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
    tracing::debug!(session = id, reason = %end.reason, "session ended");
    end
}

/// Maximum WebSocket message size accepted on sync connections.
pub const MAX_WS_MESSAGE: usize = proto::MAX_MESSAGE_BYTES;

/// Runs a session over an accepted axum WebSocket.
pub async fn serve_axum(node: Arc<Node>, socket: axum::extract::ws::WebSocket, remote: SocketAddr) {
    use axum::extract::ws::Message as M;
    let (mut sink, mut stream) = socket.split();
    let (in_tx, in_rx) = mpsc::channel::<In>(64);
    let (out_tx, mut out_rx) = mpsc::channel::<Out>(256);
    let writer = tokio::spawn(async move {
        while let Some(o) = out_rx.recv().await {
            let r = match o {
                Out::Text(t) => sink.send(M::Text(t.into())).await,
                Out::Ping => sink.send(M::Ping(Default::default())).await,
                Out::Close => {
                    let _ = sink.send(M::Close(None)).await;
                    break;
                }
            };
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
    let driver = drive(node, false, None, Some(remote), in_rx, out_tx);
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

/// Dials `url` and runs a session over it.
pub async fn dial(node: Arc<Node>, url: &str) -> Result<End, String> {
    use tokio_tungstenite::tungstenite::Message as M;
    use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
    let cfg = WebSocketConfig::default().max_message_size(Some(MAX_WS_MESSAGE)).max_frame_size(Some(MAX_WS_MESSAGE));
    let (ws, _) = tokio::time::timeout(Duration::from_secs(10), tokio_tungstenite::connect_async_with_config(url, Some(cfg), false))
        .await
        .map_err(|_| "connect timeout".to_string())?
        .map_err(|e| e.to_string())?;
    let (mut sink, mut stream) = ws.split();
    let (in_tx, in_rx) = mpsc::channel::<In>(64);
    let (out_tx, mut out_rx) = mpsc::channel::<Out>(256);
    let writer = tokio::spawn(async move {
        while let Some(o) = out_rx.recv().await {
            let r = match o {
                Out::Text(t) => sink.send(M::Text(t.into())).await,
                Out::Ping => sink.send(M::Ping(Default::default())).await,
                Out::Close => {
                    let _ = sink.send(M::Close(None)).await;
                    break;
                }
            };
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
    let driver = drive(node, true, Some(url.to_string()), None, in_rx, out_tx);
    tokio::pin!(driver);
    let end = tokio::select! {
        e = &mut driver => e,
        _ = reader => driver.await,
    };
    let _ = writer.await;
    Ok(end)
}
