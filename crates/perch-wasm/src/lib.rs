//! `PerchClient`: the Perch JavaScript API (ARCHITECTURE 8.2) in WebAssembly.
//!
//! Layout:
//! - `client.rs`: keys, publish guard, ingest, policy. Plain Rust, tested natively.
//! - `backend.rs`: the only file that calls perch-core, -store, -sync, -views.
//! - `guard.rs`, `backup.rs`, `backoff.rs`: pure helpers.
//! - `net.rs`: browser WebSocket driver (wasm only).
//! - this file: JavaScript bindings (wasm only).
//!
//! Persistence (IndexedDB) is done by the web app through `exportJsonl()`,
//! `ingest()`, `exportKey()`, and `importKey()`, so the client holds no
//! storage policy of its own.
//!
//! Non-contract helpers:
//! - `setWasmSha256(hex)`. A module cannot hash its own bytes, so the loader
//!   hashes the bytes it instantiates and passes the hash in;
//!   `buildInfo().wasm_sha256` then reports it.
//! - `retryNode(url)`. Gives a node whose head claim is unproven another 60 s
//!   and asks every node for its heads again (guard.rs). Publishing stays
//!   blocked until the records arrive or `disconnect(url)` is called.
//!
//! Panics. The module is built with `panic=abort`, so a panic traps and
//! cannot be caught in Rust. Read-only entry points (views, lookups) use
//! `PerchClient::read`, which sets no `RefCell` borrow flag, so a trap in a
//! view throws a JavaScript error and later calls still work. Entry points
//! that change state use `PerchClient::write`; a trap there leaves the state
//! possibly half-changed, and every later call throws [`STOPPED`].

pub mod backend;
pub mod backoff;
pub mod backup;
pub mod client;
pub mod guard;

#[cfg(target_arch = "wasm32")]
mod net;

#[cfg(target_arch = "wasm32")]
pub use wasm::{PerchClient, set_wasm_sha256};
#[cfg(target_arch = "wasm32")]
mod wasm {
    use crate::backend::{RecordInput, SessionId};
    use crate::client::{Client, split_records};
    use crate::guard::HeadRef;
    use crate::net::{self, Peer, PeerState};
    use serde::Serialize;
    use serde_json::{Value, json};
    use std::cell::RefCell;
    use std::collections::{BTreeMap, HashMap};
    use std::rc::Rc;
    use wasm_bindgen::prelude::*;

    thread_local! {
        static WASM_SHA256: RefCell<Option<String>> = const { RefCell::new(None) };
    }

    /// Records the SHA-256 of the module bytes the loader instantiated.
    #[wasm_bindgen(js_name = setWasmSha256)]
    pub fn set_wasm_sha256(hex: String) {
        if hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            WASM_SHA256.with(|h| *h.borrow_mut() = Some(hex.to_ascii_lowercase()));
        }
    }

    #[wasm_bindgen(start)]
    pub fn start() {
        console_error_panic_hook::set_once();
    }

    /// Thrown by every call after a trap during a state change.
    pub const STOPPED: &str = "The Perch client stopped after an internal error. Reload the page.";

    pub struct Inner {
        pub client: Client,
        pub peers: BTreeMap<String, Peer>,
        pub session_urls: HashMap<SessionId, String>,
        listeners: Vec<(u32, js_sys::Function)>,
        next_listener: u32,
        notifying: bool,
    }

    impl Inner {
        /// Feeds the publish guard after a session advanced.
        pub fn after_session_update(&mut self, sid: SessionId, now_ms: u64) {
            let Some(url) = self.session_urls.get(&sid).cloned() else { return };
            let Some(author) = self.client.author().map(String::from) else { return };
            let info = self.client.backend.session_info(sid);
            let head = self.client.backend.peer_head(sid, &author);
            // A node's head is a claim, not proof (guard.rs). A new session
            // has no claim until the node's heads arrive; until the heads
            // exchange completes, a missing claim keeps the previous one, so a
            // reconnect cannot clear a higher head.
            if head.is_some() || info.live {
                self.client.guard.node_reported(&url, head, now_ms);
            }
            if info.live {
                self.client.guard.node_checked(&url);
            }
        }
    }

    /// Calls every onChange listener with `{ reason }`. Re-entrant calls are
    /// dropped: a listener that changes the client gets the next event.
    pub fn notify(inner: &Rc<RefCell<Inner>>, reason: &str) {
        let listeners: Vec<js_sys::Function> = {
            let Ok(mut me) = inner.try_borrow_mut() else { return };
            if me.notifying {
                return;
            }
            me.notifying = true;
            me.listeners.iter().map(|(_, f)| f.clone()).collect()
        };
        let ev = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&ev, &"reason".into(), &JsValue::from_str(reason));
        for f in listeners {
            if let Err(e) = f.call1(&JsValue::NULL, &ev) {
                web_sys_console_error(&e);
            }
        }
        if let Ok(mut me) = inner.try_borrow_mut() {
            me.notifying = false;
        }
    }

    fn web_sys_console_error(e: &JsValue) {
        let console = js_sys::Reflect::get(&js_sys::global(), &"console".into()).ok();
        if let Some(c) = console {
            if let Ok(err) = js_sys::Reflect::get(&c, &"error".into()) {
                let f: js_sys::Function = err.into();
                let _ = f.call1(&c, e);
            }
        }
    }

    fn to_js<T: Serialize + ?Sized>(v: &T) -> Result<JsValue, JsError> {
        // Plain objects, and u64 within 2^53 as numbers.
        let ser = serde_wasm_bindgen::Serializer::json_compatible();
        v.serialize(&ser).map_err(|e| JsError::new(&e.to_string()))
    }

    /// JSON text or a JavaScript value, parsed with the strict parser
    /// (perch_core::json): no duplicate keys, integers only, within 2^53 − 1.
    fn from_js(v: &JsValue) -> Result<Value, JsError> {
        let text: String = match v.as_string() {
            Some(s) => s,
            // JSON.stringify returns undefined for undefined and functions.
            None => js_sys::JSON::stringify(v)
                .ok()
                .and_then(|s| JsValue::from(s).as_string())
                .ok_or_else(|| JsError::new("Value cannot be converted to JSON"))?,
        };
        crate::backend::parse_strict(&text).map_err(|e| JsError::new(&format!("Not valid JSON: {e}")))
    }

    /// A JavaScript number that is a whole number from 0 to 2^53 − 1.
    fn safe_u64(v: &JsValue) -> Option<u64> {
        let f = v.as_f64()?;
        (f.fract() == 0.0 && (0.0..=9_007_199_254_740_991.0).contains(&f)).then_some(f as u64)
    }

    fn now_ms() -> u64 {
        js_sys::Date::now() as u64
    }

    /// 32 random hex characters: this client's sync node ID.
    fn node_id() -> String {
        let mut b = [0u8; 16];
        let _ = getrandom::fill(&mut b);
        crate::backup::encode_hex(&b)
    }

    fn random32() -> Result<[u8; 32], JsError> {
        let mut b = [0u8; 32];
        getrandom::fill(&mut b).map_err(|e| JsError::new(&format!("No secure random source: {e}")))?;
        Ok(b)
    }

    #[wasm_bindgen]
    pub struct PerchClient {
        inner: Rc<RefCell<Inner>>,
    }

    impl PerchClient {
        /// Shared access that sets no borrow flag (see the module docs).
        fn read(&self) -> Result<&Inner, JsError> {
            // SAFETY: callers use the reference only inside one synchronous
            // entry point that neither calls JavaScript nor borrows `inner`
            // mutably, so no `RefMut` exists while the reference lives. The
            // call fails if a `RefMut` is held, which after a trap means the
            // state may be half-changed.
            unsafe { self.inner.try_borrow_unguarded() }.map_err(|_| JsError::new(STOPPED))
        }

        fn write(&self) -> Result<std::cell::RefMut<'_, Inner>, JsError> {
            self.inner.try_borrow_mut().map_err(|_| JsError::new(STOPPED))
        }
    }

    #[wasm_bindgen]
    impl PerchClient {
        /// `PerchClient.create({ sparse })`.
        pub fn create(options: JsValue) -> PerchClient {
            let sparse = js_sys::Reflect::get(&options, &"sparse".into()).ok().and_then(|v| v.as_bool()).unwrap_or(false);
            let inner = Rc::new(RefCell::new(Inner {
                client: Client::with_node(sparse, &node_id()),
                peers: BTreeMap::new(),
                session_urls: HashMap::new(),
                listeners: Vec::new(),
                next_listener: 0,
                notifying: false,
            }));
            if !sparse {
                net::start_refresh_timer(&inner);
            }
            PerchClient { inner }
        }

        // Keys.

        #[wasm_bindgen(js_name = generateKey)]
        pub fn generate_key(&self) -> Result<String, JsError> {
            let seed = random32()?;
            let a = self.write()?.client.generate_key(seed);
            notify(&self.inner, "key");
            Ok(a)
        }

        #[wasm_bindgen(js_name = importKey)]
        pub fn import_key(&self, backup_or_secret: String) -> Result<String, JsError> {
            let a = {
                let mut guard = self.write()?;
                let me = &mut *guard;
                let a = me.client.import_key(&backup_or_secret).map_err(|e| JsError::new(&e))?;
                for url in me.peers.keys() {
                    me.client.guard.add_node(url);
                }
                a
            };
            notify(&self.inner, "key");
            Ok(a)
        }

        #[wasm_bindgen(js_name = exportKey)]
        pub fn export_key(&self) -> Result<String, JsError> {
            self.write()?.client.export_key().map_err(|e| JsError::new(&e))
        }

        pub fn author(&self) -> Result<Option<String>, JsError> {
            Ok(self.read()?.client.author().map(String::from))
        }

        #[wasm_bindgen(js_name = publishStatus)]
        pub fn publish_status(&self) -> Result<JsValue, JsError> {
            let s = self.write()?.client.publish_status(now_ms());
            let head = |h: &Option<HeadRef>| h.as_ref().map(|h| json!({ "seq": h.seq, "id": h.id })).unwrap_or(Value::Null);
            to_js(&json!({
                "ready": s.ready,
                "reason": s.reason,
                "local_head": head(&s.local_head),
                "nodes": s.nodes.iter().map(|n| json!({ "url": n.url, "head": head(&n.head), "checked": n.checked, "unproven": n.unproven })).collect::<Vec<_>>(),
            }))
        }

        pub fn publish(&self, kind: String, body: JsValue, opts: JsValue) -> Result<JsValue, JsError> {
            let body = from_js(&body)?;
            if !body.is_object() {
                return Err(JsError::new("Body must be an object."));
            }
            let get = |k: &str| js_sys::Reflect::get(&opts, &k.into()).ok().filter(|v| !v.is_undefined() && !v.is_null());
            let replaces = get("replaces").and_then(|v| v.as_string());
            let ts = match get("ts") {
                None => None,
                Some(v) => Some(safe_u64(&v).ok_or_else(|| JsError::new("ts must be a whole number of milliseconds from 0 to 2^53 - 1."))?),
            };
            let signed = self
                .write()?
                .client
                .publish(&kind, &body.to_string(), replaces.as_deref(), ts, now_ms())
                .map_err(|e| JsError::new(&e))?;
            net::flush(&self.inner);
            notify(&self.inner, "records");
            let record: Value = serde_json::from_str(&signed.wire).map_err(|e| JsError::new(&e.to_string()))?;
            to_js(&json!({ "id": signed.id, "record": record }))
        }

        #[wasm_bindgen(js_name = issueReceipt)]
        pub fn issue_receipt(&self, listing_root_id: String, buyer: String, purchased_at: f64) -> Result<JsValue, JsError> {
            let purchased_at = safe_u64(&JsValue::from_f64(purchased_at))
                .ok_or_else(|| JsError::new("The purchase time must be a whole number of milliseconds from 0 to 2^53 - 1."))?;
            let r = self
                .read()?
                .client
                .issue_receipt(&listing_root_id, &buyer, purchased_at, random32()?)
                .map_err(|e| JsError::new(&e))?;
            to_js(&r)
        }

        #[wasm_bindgen(js_name = verifyReceipt)]
        pub fn verify_receipt(&self, receipt: JsValue) -> Result<JsValue, JsError> {
            let text = match receipt.as_string() {
                Some(s) => s,
                None => from_js(&receipt)?.to_string(),
            };
            to_js(&self.read()?.client.backend.verify_receipt(&text))
        }

        // Records.

        pub fn ingest(&self, records_or_jsonl: JsValue) -> Result<JsValue, JsError> {
            // Strings are record text; JavaScript objects go through
            // JSON.stringify and the strict parser (from_js).
            let inputs: Vec<RecordInput> = if let Some(s) = records_or_jsonl.as_string() {
                split_records(&s).map_err(|e| JsError::new(&e))?
            } else if js_sys::Array::is_array(&records_or_jsonl) {
                let arr: js_sys::Array = records_or_jsonl.into();
                let mut out = Vec::with_capacity(arr.length() as usize);
                for v in arr.iter() {
                    out.push(match v.as_string() {
                        Some(s) => RecordInput::Text(s),
                        None => RecordInput::Value(from_js(&v)?),
                    });
                }
                out
            } else {
                vec![RecordInput::Value(from_js(&records_or_jsonl)?)]
            };
            let report = self.write()?.client.ingest(&inputs);
            if report.changed() {
                net::flush(&self.inner);
                notify(&self.inner, "records");
            }
            to_js(&report.to_json())
        }

        pub fn record(&self, id: String) -> Result<JsValue, JsError> {
            match self.read()?.client.backend.record(&id) {
                Some(wire) => to_js(&serde_json::from_str::<Value>(&wire).map_err(|e| JsError::new(&e.to_string()))?),
                None => Ok(JsValue::NULL),
            }
        }

        #[wasm_bindgen(js_name = authorLog)]
        pub fn author_log(&self, author: String) -> Result<JsValue, JsError> {
            let lines = self.read()?.client.backend.author_log(&author);
            let vals: Vec<Value> = lines.iter().filter_map(|l| serde_json::from_str(l).ok()).collect();
            to_js(&vals)
        }

        #[wasm_bindgen(js_name = verifyRecord)]
        pub fn verify_record(&self, json: JsValue) -> Result<JsValue, JsError> {
            let text = match json.as_string() {
                Some(s) => s,
                None => from_js(&json)?.to_string(),
            };
            to_js(&self.read()?.client.backend.verify_record(&text))
        }

        #[wasm_bindgen(js_name = exportJsonl)]
        pub fn export_jsonl(&self) -> Result<String, JsError> {
            Ok(self.read()?.client.backend.export_jsonl())
        }

        // Sync.

        pub fn connect(&self, ws_url: String) -> js_sys::Promise {
            let inner = self.inner.clone();
            let fresh = {
                let Ok(mut me) = inner.try_borrow_mut() else {
                    return js_sys::Promise::reject(&JsValue::from_str(STOPPED));
                };
                let fresh = !me.peers.contains_key(&ws_url);
                if fresh {
                    me.peers.insert(ws_url.clone(), Peer::new(&ws_url));
                    me.client.guard.add_node(&ws_url);
                } else if let Some(p) = me.peers.get_mut(&ws_url) {
                    p.wanted = true;
                }
                fresh
            };
            let promise = js_sys::Promise::new(&mut |resolve, reject| net::add_waiter(&inner, &ws_url, resolve, reject));
            if fresh {
                net::open(&inner, &ws_url);
                notify(&inner, "peers");
            }
            promise
        }

        pub fn disconnect(&self, ws_url: String) -> js_sys::Promise {
            net::close(&self.inner, &ws_url);
            let Ok(mut me) = self.inner.try_borrow_mut() else {
                return js_sys::Promise::reject(&JsValue::from_str(STOPPED));
            };
            me.client.guard.remove_node(&ws_url);
            drop(me);
            notify(&self.inner, "peers");
            js_sys::Promise::resolve(&JsValue::UNDEFINED)
        }

        /// Non-contract helper (see the module docs).
        #[wasm_bindgen(js_name = retryNode)]
        pub fn retry_node(&self, ws_url: String) -> Result<(), JsError> {
            {
                let mut me = self.write()?;
                me.client.guard.retry(&ws_url, now_ms());
                me.client.backend.refresh();
            }
            net::retry_now(&self.inner, &ws_url);
            net::flush(&self.inner);
            notify(&self.inner, "peers");
            Ok(())
        }

        pub fn peers(&self) -> Result<JsValue, JsError> {
            let me = self.read()?;
            let list: Vec<Value> = me
                .peers
                .values()
                .map(|p| {
                    let info = p.session.map(|sid| me.client.backend.session_info(sid)).unwrap_or_default();
                    let state = if p.state == PeerState::Syncing && info.live { PeerState::Live } else { p.state };
                    json!({
                        "url": p.url, "state": state.as_str(), "sent": p.sent, "received": p.received,
                        "behind": info.behind, "ahead": info.ahead, "error": p.last_error,
                    })
                })
                .collect();
            to_js(&list)
        }

        // Policy and views.

        #[wasm_bindgen(js_name = setPolicy)]
        pub fn set_policy(&self, policy: JsValue) -> Result<(), JsError> {
            let p = from_js(&policy)?;
            self.write()?.client.set_policy(p).map_err(|e| JsError::new(&e))?;
            notify(&self.inner, "policy");
            Ok(())
        }

        pub fn policy(&self) -> Result<JsValue, JsError> {
            to_js(self.read()?.client.policy())
        }

        #[wasm_bindgen(js_name = policyHash)]
        pub fn policy_hash(&self) -> Result<String, JsError> {
            Ok(self.read()?.client.policy_hash())
        }

        pub fn search(&self, q: String, opts: JsValue) -> Result<JsValue, JsError> {
            let get = |k: &str| js_sys::Reflect::get(&opts, &k.into()).ok().and_then(|v| v.as_string());
            let sort = get("sort").unwrap_or_else(|| "relevance".into());
            let currency = get("currency").filter(|c| !c.is_empty());
            let limit = js_sys::Reflect::get(&opts, &"limit".into()).ok().and_then(|v| v.as_f64()).map(|f| f as usize);
            let me = self.read()?;
            let p = me.client.effective_policy(now_ms());
            to_js(&me.client.backend.search(&p, &q, &sort, currency.as_deref(), limit))
        }

        pub fn product(&self, key: String) -> Result<JsValue, JsError> {
            let me = self.read()?;
            to_js(&me.client.backend.product(&me.client.effective_policy(now_ms()), &key))
        }

        pub fn listing(&self, id: String) -> Result<JsValue, JsError> {
            let me = self.read()?;
            to_js(&me.client.backend.listing(&me.client.effective_policy(now_ms()), &id))
        }

        #[wasm_bindgen(js_name = authorView)]
        pub fn author_view(&self, id: String) -> Result<JsValue, JsError> {
            let me = self.read()?;
            to_js(&me.client.backend.author_view(&me.client.effective_policy(now_ms()), &id))
        }

        #[wasm_bindgen(js_name = authorRecords)]
        pub fn author_records(&self, author: String, opts: JsValue) -> Result<JsValue, JsError> {
            let o = if opts.is_undefined() || opts.is_null() { json!({}) } else { from_js(&opts)? };
            let kinds: Option<Vec<String>> = o.get("kinds").and_then(Value::as_array).map(|a| a.iter().filter_map(|k| k.as_str().map(String::from)).collect());
            let limit = o.get("limit").and_then(Value::as_u64).map(|n| n as usize);
            let from_seq = o.get("from_seq").and_then(Value::as_u64);
            let me = self.read()?;
            let p = me.client.effective_policy(now_ms());
            to_js(&me.client.backend.author_records(&p, &author, kinds.as_deref(), limit, from_seq))
        }

        pub fn flags(&self, root_id: String) -> Result<JsValue, JsError> {
            let me = self.read()?;
            to_js(&me.client.backend.flags(&me.client.effective_policy(now_ms()), &root_id))
        }

        #[wasm_bindgen(js_name = recordStatus)]
        pub fn record_status(&self, id: String) -> Result<JsValue, JsError> {
            let me = self.read()?;
            to_js(&me.client.backend.record_status(&me.client.effective_policy(now_ms()), &id))
        }

        pub fn currencies(&self) -> Result<JsValue, JsError> {
            to_js(&self.read()?.client.backend.currencies())
        }

        pub fn stats(&self) -> Result<JsValue, JsError> {
            to_js(&self.read()?.client.backend.stats())
        }

        #[wasm_bindgen(js_name = buildInfo)]
        pub fn build_info(&self) -> Result<JsValue, JsError> {
            let sha = WASM_SHA256.with(|h| h.borrow().clone());
            to_js(&json!({ "version": env!("CARGO_PKG_VERSION"), "wasm_sha256": sha }))
        }

        /// Returns an unsubscribe function.
        #[wasm_bindgen(js_name = onChange)]
        pub fn on_change(&self, callback: js_sys::Function) -> Result<js_sys::Function, JsError> {
            let id = {
                let mut me = self.write()?;
                me.next_listener += 1;
                let id = me.next_listener;
                me.listeners.push((id, callback));
                id
            };
            let weak = Rc::downgrade(&self.inner);
            let unsub = Closure::<dyn FnMut()>::new(move || {
                if let Some(inner) = weak.upgrade() {
                    if let Ok(mut me) = inner.try_borrow_mut() {
                        me.listeners.retain(|(i, _)| *i != id);
                    }
                }
            });
            Ok(unsub.into_js_value().into())
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) use wasm::{Inner, notify};

#[cfg(target_arch = "wasm32")]
pub(crate) fn wasm_stopped() -> &'static str {
    wasm::STOPPED
}
