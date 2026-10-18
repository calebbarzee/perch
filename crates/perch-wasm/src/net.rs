//! WebSocket driver for the browser. Each peer URL has one socket at a time.
//! Frames go to the perch-sync session through `Backend`; outgoing frames
//! are flushed after every inbound frame and every local change. Closed
//! sockets reconnect with exponential backoff (backoff.rs) until
//! `disconnect()` is called.

#![cfg(target_arch = "wasm32")]

use crate::backend::SessionId;
use crate::backoff;
use crate::Inner;
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{CloseEvent, MessageEvent, WebSocket};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerState {
    Connecting,
    Syncing,
    Live,
    Backoff,
    Closed,
}

impl PeerState {
    pub fn as_str(self) -> &'static str {
        match self {
            PeerState::Connecting => "connecting",
            PeerState::Syncing => "syncing",
            PeerState::Live => "live",
            PeerState::Backoff => "backoff",
            PeerState::Closed => "closed",
        }
    }
}

pub struct Peer {
    pub url: String,
    pub state: PeerState,
    pub sent: u64,
    pub received: u64,
    pub session: Option<SessionId>,
    pub attempt: u32,
    pub wanted: bool,
    pub last_error: Option<String>,
    socket: Option<WebSocket>,
    closures: Vec<Closure<dyn FnMut(JsValue)>>,
    timer: Option<i32>,
    /// Resolvers of connect() promises waiting for the first open or failure.
    waiters: Vec<(js_sys::Function, js_sys::Function)>,
}

impl Peer {
    pub fn new(url: &str) -> Peer {
        Peer {
            url: url.to_string(),
            state: PeerState::Connecting,
            sent: 0,
            received: 0,
            session: None,
            attempt: 0,
            wanted: true,
            last_error: None,
            socket: None,
            closures: Vec::new(),
            timer: None,
            waiters: Vec::new(),
        }
    }
}

fn set_timeout(f: &Closure<dyn FnMut()>, ms: f64) -> i32 {
    let global = js_sys::global();
    let set: js_sys::Function = js_sys::Reflect::get(&global, &"setTimeout".into()).unwrap().unchecked_into();
    set.call2(&global, f.as_ref().unchecked_ref(), &JsValue::from_f64(ms)).ok().and_then(|v| v.as_f64()).unwrap_or(0.0) as i32
}

fn clear_timeout(id: i32) {
    let global = js_sys::global();
    if let Ok(clear) = js_sys::Reflect::get(&global, &"clearTimeout".into()) {
        let clear: js_sys::Function = clear.unchecked_into();
        let _ = clear.call1(&global, &JsValue::from_f64(id as f64));
    }
}

/// Heads re-exchange on every open session every 10 minutes
/// (docs/sync-protocol.md 9.2.3, rule L7). It repairs anything a lost
/// message left unsynced. The timer stops when the client is dropped.
pub const REFRESH_MS: f64 = 600_000.0;

pub fn start_refresh_timer(inner: &Rc<RefCell<Inner>>) {
    let weak: Weak<RefCell<Inner>> = Rc::downgrade(inner);
    let handle: Rc<RefCell<Option<i32>>> = Rc::new(RefCell::new(None));
    let h2 = handle.clone();
    let cb = Closure::<dyn FnMut()>::new(move || {
        let Some(inner) = weak.upgrade() else {
            if let Some(id) = h2.borrow_mut().take() {
                clear_interval(id);
            }
            return;
        };
        if let Ok(mut me) = inner.try_borrow_mut() {
            me.client.backend.refresh();
        }
        flush(&inner);
    });
    let global = js_sys::global();
    if let Ok(set) = js_sys::Reflect::get(&global, &"setInterval".into()) {
        let set: js_sys::Function = set.unchecked_into();
        let id = set.call2(&global, cb.as_ref().unchecked_ref(), &JsValue::from_f64(REFRESH_MS)).ok().and_then(|v| v.as_f64()).unwrap_or(0.0) as i32;
        *handle.borrow_mut() = Some(id);
    }
    cb.forget();
}

fn clear_interval(id: i32) {
    let global = js_sys::global();
    if let Ok(clear) = js_sys::Reflect::get(&global, &"clearInterval".into()) {
        let clear: js_sys::Function = clear.unchecked_into();
        let _ = clear.call1(&global, &JsValue::from_f64(id as f64));
    }
}

/// Opens (or reopens) the socket for `url`.
pub fn open(inner: &Rc<RefCell<Inner>>, url: &str) {
    let ws = match WebSocket::new(url) {
        Ok(ws) => ws,
        Err(e) => {
            let msg = e.as_string().unwrap_or_else(|| format!("{e:?}"));
            fail_and_retry(inner, url, &msg);
            return;
        }
    };
    let weak = Rc::downgrade(inner);
    let u = url.to_string();

    let on_open = {
        let weak = weak.clone();
        let u = u.clone();
        Closure::<dyn FnMut(JsValue)>::new(move |_ev: JsValue| {
            if let Some(inner) = weak.upgrade() {
                on_open(&inner, &u);
            }
        })
    };
    let on_message = {
        let weak = weak.clone();
        let u = u.clone();
        Closure::<dyn FnMut(JsValue)>::new(move |ev: JsValue| {
            let Some(inner) = weak.upgrade() else { return };
            let ev: MessageEvent = ev.unchecked_into();
            if let Some(text) = ev.data().as_string() {
                on_frame(&inner, &u, &text);
            }
        })
    };
    let on_close = {
        let weak = weak.clone();
        let u = u.clone();
        Closure::<dyn FnMut(JsValue)>::new(move |ev: JsValue| {
            let Some(inner) = weak.upgrade() else { return };
            let ev: CloseEvent = ev.unchecked_into();
            let reason = if ev.reason().is_empty() { format!("closed ({})", ev.code()) } else { ev.reason() };
            on_closed(&inner, &u, &reason);
        })
    };
    ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));

    let Ok(mut me) = inner.try_borrow_mut() else { return };
    if let Some(p) = me.peers.get_mut(url) {
        p.state = PeerState::Connecting;
        p.socket = Some(ws);
        p.closures = vec![on_open, on_message, on_close];
    }
}

fn on_open(inner: &Rc<RefCell<Inner>>, url: &str) {
    {
        let Ok(mut guard) = inner.try_borrow_mut() else { return };
        let me = &mut *guard;
        let sid = me.client.backend.open_session(url);
        me.session_urls.insert(sid, url.to_string());
        if let Some(p) = me.peers.get_mut(url) {
            p.session = Some(sid);
            p.state = PeerState::Syncing;
            p.attempt = 0;
            p.last_error = None;
            for (resolve, _) in p.waiters.drain(..) {
                let _ = resolve.call0(&JsValue::NULL);
            }
        }
    }
    flush(inner);
    crate::notify(inner, "peers");
}

fn on_frame(inner: &Rc<RefCell<Inner>>, url: &str, text: &str) {
    let changed = {
        let Ok(mut guard) = inner.try_borrow_mut() else { return };
        let me = &mut *guard;
        let Some(sid) = me.peers.get(url).and_then(|p| p.session) else { return };
        let (records, changed) = me.client.backend.on_message(sid, text);
        let info = me.client.backend.session_info(sid);
        if let Some(p) = me.peers.get_mut(url) {
            p.received += records;
            if info.live && p.state == PeerState::Syncing {
                p.state = PeerState::Live;
            }
        }
        me.after_session_update(sid, js_sys::Date::now() as u64);
        changed
    };
    flush(inner);
    if changed {
        crate::notify(inner, "records");
    } else {
        crate::notify(inner, "peers");
    }
}

fn on_closed(inner: &Rc<RefCell<Inner>>, url: &str, reason: &str) {
    {
        let Ok(mut guard) = inner.try_borrow_mut() else { return };
        let me = &mut *guard;
        if let Some(p) = me.peers.get_mut(url) {
            if let Some(sid) = p.session.take() {
                me.client.backend.on_closed(sid);
                me.session_urls.remove(&sid);
            }
            p.socket = None;
            p.closures.clear();
            p.last_error = Some(reason.to_string());
        }
    }
    fail_and_retry(inner, url, reason);
}

fn fail_and_retry(inner: &Rc<RefCell<Inner>>, url: &str, reason: &str) {
    let (wanted, attempt) = {
        let Ok(mut me) = inner.try_borrow_mut() else { return };
        let Some(p) = me.peers.get_mut(url) else { return };
        p.last_error = Some(reason.to_string());
        for (_, reject) in p.waiters.drain(..) {
            let _ = reject.call1(&JsValue::NULL, &JsValue::from_str(&format!("{url}: {reason}")));
        }
        if !p.wanted {
            p.state = PeerState::Closed;
            (false, 0)
        } else {
            p.state = PeerState::Backoff;
            let a = p.attempt;
            p.attempt += 1;
            (true, a)
        }
    };
    crate::notify(inner, "peers");
    if !wanted {
        return;
    }
    let ms = backoff::delay_ms(attempt, js_sys::Math::random());
    let weak: Weak<RefCell<Inner>> = Rc::downgrade(inner);
    let u = url.to_string();
    let cb = Closure::<dyn FnMut()>::once(move || {
        if let Some(inner) = weak.upgrade() {
            let still = inner.try_borrow().is_ok_and(|me| me.peers.get(&u).is_some_and(|p| p.wanted && p.socket.is_none()));
            if still {
                open(&inner, &u);
            }
        }
    });
    let id = set_timeout(&cb, ms);
    cb.forget();
    if let Ok(mut me) = inner.try_borrow_mut() {
        if let Some(p) = me.peers.get_mut(url) {
            p.timer = Some(id);
        }
    }
}

/// Sends every queued frame and applies close requests.
pub fn flush(inner: &Rc<RefCell<Inner>>) {
    let (out, closes) = {
        let Ok(mut guard) = inner.try_borrow_mut() else { return };
        let me = &mut *guard;
        me.client.backend.take_outgoing()
    };
    let mut to_close = Vec::new();
    {
        let Ok(mut me) = inner.try_borrow_mut() else { return };
        for o in out {
            let Some(url) = me.session_urls.get(&o.session).cloned() else { continue };
            if let Some(p) = me.peers.get_mut(&url) {
                if let Some(ws) = &p.socket {
                    if ws.send_with_str(&o.text).is_ok() {
                        p.sent += o.records;
                    }
                }
            }
        }
        for c in closes {
            if let Some(url) = me.session_urls.get(&c.session).cloned() {
                to_close.push((url, c.reason));
            }
        }
    }
    for (url, reason) in to_close {
        let ws = inner.try_borrow().ok().and_then(|me| me.peers.get(&url).and_then(|p| p.socket.clone()));
        if let Some(ws) = ws {
            let _ = ws.close_with_code_and_reason(1000, &reason.chars().take(100).collect::<String>());
        }
    }
}

/// Stops reconnecting and closes the socket.
pub fn close(inner: &Rc<RefCell<Inner>>, url: &str) {
    let (ws, timer, sid) = {
        let Ok(mut me) = inner.try_borrow_mut() else { return };
        let Some(p) = me.peers.get_mut(url) else { return };
        p.wanted = false;
        p.state = PeerState::Closed;
        (p.socket.take(), p.timer.take(), p.session.take())
    };
    if let Some(t) = timer {
        clear_timeout(t);
    }
    if let Some(ws) = ws {
        ws.set_onclose(None);
        ws.set_onmessage(None);
        ws.set_onopen(None);
        let _ = ws.close();
    }
    let Ok(mut guard) = inner.try_borrow_mut() else { return };
    let me = &mut *guard;
    if let Some(sid) = sid {
        me.client.backend.on_closed(sid);
        me.session_urls.remove(&sid);
    }
    me.peers.remove(url);
}

/// Reconnects now when the peer is waiting out a backoff.
pub fn retry_now(inner: &Rc<RefCell<Inner>>, url: &str) {
    let timer = {
        let Ok(mut me) = inner.try_borrow_mut() else { return };
        let Some(p) = me.peers.get_mut(url) else { return };
        if !p.wanted || p.socket.is_some() {
            return;
        }
        p.attempt = 0;
        p.timer.take()
    };
    if let Some(t) = timer {
        clear_timeout(t);
    }
    open(inner, url);
}

pub fn add_waiter(inner: &Rc<RefCell<Inner>>, url: &str, resolve: js_sys::Function, reject: js_sys::Function) {
    let Ok(mut me) = inner.try_borrow_mut() else {
        let _ = reject.call1(&JsValue::NULL, &JsValue::from_str(crate::wasm_stopped()));
        return;
    };
    match me.peers.get_mut(url) {
        Some(p) if p.state == PeerState::Syncing || p.state == PeerState::Live => {
            let _ = resolve.call0(&JsValue::NULL);
        }
        Some(p) => p.waiters.push((resolve, reject)),
        None => {
            let _ = reject.call1(&JsValue::NULL, &JsValue::from_str("not connecting"));
        }
    }
}
