//! WebSocket agent chat handler.
//!
//! Connect: `ws://host:port/ws/chat?session_id=ID&name=My+Session`
//!
//! Protocol:
//! ```text
//! Server -> Client: {"type":"session_start","session_id":"...","name":"...","resumed":true,"message_count":42}
//! Client -> Server: {"type":"message","content":"Hello"}
//! Server -> Client: {"type":"chunk","content":"Hi! "}
//! Server -> Client: {"type":"tool_call","name":"shell","args":{...}}
//! Server -> Client: {"type":"tool_result","name":"shell","output":"..."}
//! Server -> Client: {"type":"done","full_response":"..."}
//! ```
//!
//! Query params:
//! - `session_id` — resume or create a session (default: new UUID)
//! - `name` — optional human-readable label for the session
//! - `token` — bearer auth token (alternative to Authorization header)

use super::AppState;
use axum::{
    extract::{
        Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, header},
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use regex::Regex;
use serde::Deserialize;
use std::sync::OnceLock;
use tracing::debug;

/// Optional connection parameters sent as the first WebSocket message.
///
/// If the first message after upgrade is `{"type":"connect",...}`, these
/// parameters are extracted and an acknowledgement is sent back. Old clients
/// that send `{"type":"message",...}` as the first frame still work — the
/// message is processed normally (backward-compatible).
#[derive(Debug, Deserialize)]
struct ConnectParams {
    #[serde(rename = "type")]
    msg_type: String,
    /// Client-chosen session ID for memory persistence
    #[serde(default)]
    session_id: Option<String>,
    /// Device name for device registry tracking
    #[serde(default)]
    device_name: Option<String>,
    /// Client capabilities
    #[serde(default)]
    capabilities: Vec<String>,
}

/// Legacy `[conversationId: <id>]` content prefix used by older clawcraft
/// clients before the typed `threadId` envelope field existed.
///
/// Matches the real emission `[conversationId: <id>]\n<content>`.
///
/// RETIREMENT: This fallback is purely transitional. Bead `rnk-eaja` on the
/// clawcraft repo tracks its removal — scheduled ≥7 days after the clawcraft
/// Phase 4 cutover is verified stable in prod. Once removed, only the typed
/// `threadId` envelope field will be honored.
fn conversation_id_prefix_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\[conversationId:\s*([^\]]+?)\s*\]\s*").unwrap())
}

/// Where the thread identifier was sourced from on an inbound message.
/// Reported in logs during the transition window for observability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThreadIdSource {
    TypedField,
    PrefixFallback,
    None,
}

impl ThreadIdSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::TypedField => "typed_field",
            Self::PrefixFallback => "prefix_fallback",
            Self::None => "none",
        }
    }
}

/// Parse the thread identifier from a `WsClientMessage` envelope.
///
/// Priority:
/// 1. `parsed.threadId` (typed field) — authoritative when present and non-empty.
/// 2. Legacy `[conversationId: <id>]` content prefix — extracted via
///    [`conversation_id_prefix_regex`], stripped from the returned content.
///
/// Other content prefixes (`[SYSTEM]`, `[MEDIA_UPLOAD: …]`) are left intact —
/// the strip is path-explicit to `[conversationId:]`.
///
/// Returns `(thread_id, cleaned_content, source)`.
fn parse_thread_id(
    parsed: &serde_json::Value,
    content: &str,
) -> (Option<String>, String, ThreadIdSource) {
    if let Some(tid) = parsed.get("threadId").and_then(|v| v.as_str()) {
        let trimmed = tid.trim();
        if !trimmed.is_empty() {
            return (
                Some(trimmed.to_string()),
                content.to_string(),
                ThreadIdSource::TypedField,
            );
        }
    }

    if let Some(caps) = conversation_id_prefix_regex().captures(content) {
        let id = caps
            .get(1)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();
        let stripped = content[caps.get(0).unwrap().end()..].to_string();
        if !id.is_empty() {
            return (Some(id), stripped, ThreadIdSource::PrefixFallback);
        }
    }

    (None, content.to_string(), ThreadIdSource::None)
}

/// The sub-protocol we support for the chat WebSocket.
const WS_PROTOCOL: &str = "zeroclaw.v1";

/// Prefix used in `Sec-WebSocket-Protocol` to carry a bearer token.
const BEARER_SUBPROTO_PREFIX: &str = "bearer.";

#[derive(Deserialize)]
pub struct WsQuery {
    pub token: Option<String>,
    pub session_id: Option<String>,
    /// Optional human-readable name for the session.
    pub name: Option<String>,
}

/// Extract a bearer token from WebSocket-compatible sources.
///
/// Precedence (first non-empty wins):
/// 1. `Authorization: Bearer <token>` header
/// 2. `Sec-WebSocket-Protocol: bearer.<token>` subprotocol
/// 3. `?token=<token>` query parameter
///
/// Browsers cannot set custom headers on `new WebSocket(url)`, so the query
/// parameter and subprotocol paths are required for browser-based clients.
fn extract_ws_token<'a>(headers: &'a HeaderMap, query_token: Option<&'a str>) -> Option<&'a str> {
    // 1. Authorization header
    if let Some(t) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|auth| auth.strip_prefix("Bearer "))
    {
        if !t.is_empty() {
            return Some(t);
        }
    }

    // 2. Sec-WebSocket-Protocol: bearer.<token>
    if let Some(t) = headers
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok())
        .and_then(|protos| {
            protos
                .split(',')
                .map(|p| p.trim())
                .find_map(|p| p.strip_prefix(BEARER_SUBPROTO_PREFIX))
        })
    {
        if !t.is_empty() {
            return Some(t);
        }
    }

    // 3. ?token= query parameter
    if let Some(t) = query_token {
        if !t.is_empty() {
            return Some(t);
        }
    }

    None
}

/// GET /ws/chat — WebSocket upgrade for agent chat
pub async fn handle_ws_chat(
    State(state): State<AppState>,
    Query(params): Query<WsQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    // Auth: check header, subprotocol, then query param (precedence order)
    if state.pairing.require_pairing() {
        let token = extract_ws_token(&headers, params.token.as_deref()).unwrap_or("");
        if !state.pairing.is_authenticated(token) {
            return (
                axum::http::StatusCode::UNAUTHORIZED,
                "Unauthorized — provide Authorization header, Sec-WebSocket-Protocol bearer, or ?token= query param",
            )
                .into_response();
        }
    }

    // Echo Sec-WebSocket-Protocol if the client requests our sub-protocol.
    let ws = if headers
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok())
        .map_or(false, |protos| {
            protos.split(',').any(|p| p.trim() == WS_PROTOCOL)
        }) {
        ws.protocols([WS_PROTOCOL])
    } else {
        ws
    };

    let session_id = params.session_id;
    let session_name = params.name;
    ws.on_upgrade(move |socket| handle_socket(socket, state, session_id, session_name))
        .into_response()
}

/// Gateway session key prefix to avoid collisions with channel sessions.
const GW_SESSION_PREFIX: &str = "gw_";

/// Gateway thread key prefix for thread-scoped session storage.
const GW_THREAD_PREFIX: &str = "gw_thread_";

/// Build a session storage key for a given thread id.
fn session_key_for_thread(thread_id: &str) -> String {
    format!("{GW_THREAD_PREFIX}{thread_id}")
}

/// Select the storage key for the current turn.
///
/// Prefers a thread-scoped key when `thread_id` is present and non-empty,
/// otherwise falls back to the connection's session key. Behavior-preserving
/// at every call site as long as thread_id is `None` — which is the case
/// today, since callers still pass `None` until Step 3 wires real ids.
fn pick_session_key(thread_id: Option<&str>, fallback: &str) -> String {
    match thread_id {
        Some(tid) if !tid.is_empty() => session_key_for_thread(tid),
        _ => fallback.to_string(),
    }
}

/// Decision tag for the per-thread hydration branch.
///
/// Captures the three cases the WS handler cares about: first-thread on a
/// socket, same-thread continuation (cheap no-op), and mid-socket thread
/// switch (must clear stale history). Extracted as a pure function so the
/// branch logic is unit-testable without a real WebSocket sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HydrationAction {
    Seed,
    Replace,
    Skip,
}

fn classify_hydration(hydrated_thread: Option<&str>, incoming_tid: &str) -> HydrationAction {
    match hydrated_thread {
        None => HydrationAction::Seed,
        Some(prev) if prev != incoming_tid => HydrationAction::Replace,
        _ => HydrationAction::Skip,
    }
}

/// Resolve the thread id for the current turn under the v1 forgiving policy:
///
/// - Inbound message has a threadId → use it (and emit a warn if it disagrees
///   with the socket's hydrated thread; the new id wins, which triggers a
///   thread switch downstream).
/// - Inbound message has NO threadId but the socket has already hydrated a
///   thread → treat as continuation of that thread, warn so the divergence
///   is observable in production.
/// - Neither → `None` (legacy connection-scoped flow).
///
/// We deliberately chose the forgiving option in v1: dropping such turns
/// would break clients that fall back to the legacy `[conversationId:]`
/// prefix when their typed envelope is misconfigured. Reversible.
fn effective_thread_id(
    inbound: Option<&str>,
    hydrated_thread: Option<&str>,
    session_id: &str,
) -> Option<String> {
    match (inbound, hydrated_thread) {
        (Some(tid), _) => Some(tid.to_string()),
        (None, Some(prev)) => {
            tracing::warn!(
                session_id = %session_id,
                hydrated_thread = %prev,
                "WS message has no threadId but socket already hydrated a thread; \
                 treating as continuation (v1 forgiving policy)"
            );
            Some(prev.to_string())
        }
        (None, None) => None,
    }
}

/// Hydrate the agent for a thread on the first message that carries its id,
/// emit a `thread_resumed` frame, and remember which thread is now loaded.
///
/// See [`classify_hydration`] for the branch logic.
///
/// Also persists the connect-time `session_name` query param against the
/// thread-scoped key (deferred from Step 2 because tid is unknown at
/// connection-open time).
async fn hydrate_thread_if_changed(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    backend: &dyn crate::channels::session_backend::SessionBackend,
    agent: &mut crate::agent::Agent,
    hydrated_thread: &mut Option<String>,
    tid: &str,
    session_name: Option<&str>,
) {
    let action = classify_hydration(hydrated_thread.as_deref(), tid);
    if action == HydrationAction::Skip {
        return;
    }

    let thread_key = session_key_for_thread(tid);
    let messages = backend.load(&thread_key);
    let count = messages.len();
    tracing::debug!(
        thread_id = %tid,
        thread_key = %thread_key,
        action = ?action,
        message_count = count,
        "WS first-message hydration"
    );
    match action {
        HydrationAction::Replace => agent.replace_history(&messages),
        HydrationAction::Seed => agent.seed_history(&messages),
        HydrationAction::Skip => unreachable!(),
    }
    *hydrated_thread = Some(tid.to_string());

    if let Some(name) = session_name {
        if !name.is_empty() {
            let _ = backend.set_session_name(&thread_key, name);
        }
    }
    let stored_name = backend.get_session_name(&thread_key).unwrap_or(None);

    let mut frame = serde_json::json!({
        "type": "thread_resumed",
        "thread_id": tid,
        "message_count": count,
    });
    if let Some(n) = stored_name {
        frame["name"] = serde_json::Value::String(n);
    }
    let _ = sender.send(Message::Text(frame.to_string().into())).await;
}

async fn handle_socket(
    socket: WebSocket,
    state: AppState,
    session_id: Option<String>,
    session_name: Option<String>,
) {
    let (mut sender, mut receiver) = socket.split();

    // Track whether the client passed an explicit session_id BEFORE we
    // unwrap-or-default below. Used to gate the legacy connection-open
    // hydration path: only CLI/test-harness direct connects pass an explicit
    // session_id; browser/clawcraft clients omit it and rely on per-thread
    // hydration on the first message-with-threadId (see `clawcraft:relay.ts:43`).
    let has_explicit_session_id = session_id.is_some();

    // Resolve session ID: use provided or generate a new UUID
    let session_id = session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let session_key = format!("{GW_SESSION_PREFIX}{session_id}");

    // Socket-local memory of which thread is currently hydrated into the
    // agent's history. Updated by `hydrate_thread_if_changed` and used by
    // the no-threadId continuity policy (see `effective_thread_id`).
    let mut hydrated_thread: Option<String> = None;

    // Build a persistent Agent for this connection so history is maintained across turns.
    let config = state.config.lock().clone();
    let mut agent = match crate::agent::Agent::from_config(&config).await {
        Ok(a) => a,
        Err(e) => {
            tracing::error!(error = %e, "Agent initialization failed");
            let err = serde_json::json!({
                "type": "error",
                "message": format!("Failed to initialise agent: {e}"),
                "code": "AGENT_INIT_FAILED"
            });
            let _ = sender.send(Message::Text(err.to_string().into())).await;
            let _ = sender
                .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                    code: 1011,
                    reason: axum::extract::ws::Utf8Bytes::from_static(
                        "Agent initialization failed",
                    ),
                })))
                .await;
            return;
        }
    };
    agent.set_memory_session_id(Some(session_id.clone()));

    // Hydrate agent from persisted session (LEGACY connection-open path).
    //
    // This path runs only when the client passed an explicit `session_id`
    // query param — i.e. CLI/test-harness direct connects that want to resume
    // a connection-scoped session. Browser/clawcraft clients omit session_id
    // and rely on the per-thread hydration that fires on the first message
    // carrying a threadId (see the first-message hydration block below and
    // `clawcraft:relay.ts:43`). For those clients this block is a no-op and
    // `resumed`/`message_count` stay at their defaults.
    let mut resumed = false;
    let mut message_count: usize = 0;
    let mut effective_name: Option<String> = None;
    if has_explicit_session_id {
        if let Some(ref backend) = state.session_backend {
            let messages = backend.load(&session_key);
            if !messages.is_empty() {
                message_count = messages.len();
                agent.seed_history(&messages);
                resumed = true;
            }
            // Set session name if provided (non-empty) on connect
            if let Some(ref name) = session_name {
                if !name.is_empty() {
                    let _ = backend.set_session_name(&session_key, name);
                    effective_name = Some(name.clone());
                }
            }
            // If no name was provided via query param, load the stored name
            if effective_name.is_none() {
                effective_name = backend.get_session_name(&session_key).unwrap_or(None);
            }
        }
    }

    // Send session_start message to client
    let mut session_start = serde_json::json!({
        "type": "session_start",
        "session_id": session_id,
        "resumed": resumed,
        "message_count": message_count,
    });
    if let Some(ref name) = effective_name {
        session_start["name"] = serde_json::Value::String(name.clone());
    }
    let _ = sender
        .send(Message::Text(session_start.to_string().into()))
        .await;

    // ── Optional connect handshake ──────────────────────────────────
    // The first message may be a `{"type":"connect",...}` frame carrying
    // connection parameters.  If it is, we extract the params, send an
    // ack, and proceed to the normal message loop.  If the first message
    // is a regular `{"type":"message",...}` frame, we fall through and
    // process it immediately (backward-compatible).
    let mut first_msg_fallback: Option<String> = None;

    if let Some(first) = receiver.next().await {
        match first {
            Ok(Message::Text(text)) => {
                if let Ok(cp) = serde_json::from_str::<ConnectParams>(&text) {
                    if cp.msg_type == "connect" {
                        debug!(
                            session_id = ?cp.session_id,
                            device_name = ?cp.device_name,
                            capabilities = ?cp.capabilities,
                            "WebSocket connect params received"
                        );
                        // Override session_id if provided in connect params
                        if let Some(sid) = &cp.session_id {
                            agent.set_memory_session_id(Some(sid.clone()));
                        }
                        let ack = serde_json::json!({
                            "type": "connected",
                            "message": "Connection established"
                        });
                        let _ = sender.send(Message::Text(ack.to_string().into())).await;
                    } else {
                        // Not a connect message — fall through to normal processing
                        first_msg_fallback = Some(text.to_string());
                    }
                } else {
                    // Not parseable as ConnectParams — fall through
                    first_msg_fallback = Some(text.to_string());
                }
            }
            Ok(Message::Close(_)) | Err(_) => return,
            _ => {}
        }
    }

    // Process the first message if it was not a connect frame
    if let Some(ref text) = first_msg_fallback {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(text) {
            if parsed["type"].as_str() == Some("message") {
                let raw_content = parsed["content"].as_str().unwrap_or("").to_string();
                let (thread_id, content, src) = parse_thread_id(&parsed, &raw_content);
                match src {
                    ThreadIdSource::None => tracing::warn!(
                        session_id = %session_id,
                        "WS message has no threadId (typed or legacy prefix); spec write will be skipped this turn",
                    ),
                    _ => tracing::info!(
                        session_id = %session_id,
                        thread_id = %thread_id.as_deref().unwrap_or(""),
                        source = src.as_str(),
                        "WS message threadId parsed",
                    ),
                }
                if let (Some(tid), Some(backend)) =
                    (thread_id.as_deref(), state.session_backend.as_ref())
                {
                    hydrate_thread_if_changed(
                        &mut sender,
                        backend.as_ref(),
                        &mut agent,
                        &mut hydrated_thread,
                        tid,
                        session_name.as_deref(),
                    )
                    .await;
                }
                let effective = effective_thread_id(
                    thread_id.as_deref(),
                    hydrated_thread.as_deref(),
                    &session_id,
                );
                let turn_key = pick_session_key(effective.as_deref(), &session_key);
                agent.set_current_thread_id(effective.clone());
                if !content.is_empty() {
                    // Persist user message
                    if let Some(ref backend) = state.session_backend {
                        let user_msg = crate::providers::ChatMessage::user(&content);
                        let _ = backend.append(&turn_key, &user_msg);
                    }
                    process_chat_message(
                        &state,
                        &mut agent,
                        &mut sender,
                        &content,
                        &session_key,
                        effective.as_deref(),
                    )
                    .await;
                }
            } else {
                let unknown_type = parsed["type"].as_str().unwrap_or("unknown");
                let err = serde_json::json!({
                    "type": "error",
                    "message": format!(
                        "Unsupported message type \"{unknown_type}\". Send {{\"type\":\"message\",\"content\":\"your text\"}}"
                    )
                });
                let _ = sender.send(Message::Text(err.to_string().into())).await;
            }
        } else {
            let err = serde_json::json!({
                "type": "error",
                "message": "Invalid JSON. Send {\"type\":\"message\",\"content\":\"your text\"}"
            });
            let _ = sender.send(Message::Text(err.to_string().into())).await;
        }
    }

    // Subscribe to the shared broadcast channel so cron/heartbeat events
    // are forwarded to this WebSocket client.
    let mut broadcast_rx = state.event_tx.subscribe();

    loop {
        tokio::select! {
            // ── Client message ────────────────────────────────────────
            client_msg = receiver.next() => {
                let Some(msg) = client_msg else { break };
                let msg = match msg {
                    Ok(Message::Text(text)) => text,
                    Ok(Message::Close(_)) | Err(_) => break,
                    _ => continue,
                };

                // Parse incoming message
                let parsed: serde_json::Value = match serde_json::from_str(&msg) {
                    Ok(v) => v,
                    Err(e) => {
                        let err = serde_json::json!({
                            "type": "error",
                            "message": format!("Invalid JSON: {}", e),
                            "code": "INVALID_JSON"
                        });
                        let _ = sender.send(Message::Text(err.to_string().into())).await;
                        continue;
                    }
                };

                let msg_type = parsed["type"].as_str().unwrap_or("");
                if msg_type != "message" {
                    let err = serde_json::json!({
                        "type": "error",
                        "message": format!(
                            "Unsupported message type \"{msg_type}\". Send {{\"type\":\"message\",\"content\":\"your text\"}}"
                        ),
                        "code": "UNKNOWN_MESSAGE_TYPE"
                    });
                    let _ = sender.send(Message::Text(err.to_string().into())).await;
                    continue;
                }

                let raw_content = parsed["content"].as_str().unwrap_or("").to_string();
                let (thread_id, content, src) = parse_thread_id(&parsed, &raw_content);
                match src {
                    ThreadIdSource::None => tracing::warn!(
                        session_id = %session_id,
                        "WS message has no threadId (typed or legacy prefix); spec write will be skipped this turn",
                    ),
                    _ => tracing::info!(
                        session_id = %session_id,
                        thread_id = %thread_id.as_deref().unwrap_or(""),
                        source = src.as_str(),
                        "WS message threadId parsed",
                    ),
                }
                if content.is_empty() {
                    let err = serde_json::json!({
                        "type": "error",
                        "message": "Message content cannot be empty",
                        "code": "EMPTY_CONTENT"
                    });
                    let _ = sender.send(Message::Text(err.to_string().into())).await;
                    continue;
                }

                if let (Some(tid), Some(backend)) =
                    (thread_id.as_deref(), state.session_backend.as_ref())
                {
                    hydrate_thread_if_changed(
                        &mut sender,
                        backend.as_ref(),
                        &mut agent,
                        &mut hydrated_thread,
                        tid,
                        session_name.as_deref(),
                    )
                    .await;
                }
                let effective = effective_thread_id(
                    thread_id.as_deref(),
                    hydrated_thread.as_deref(),
                    &session_id,
                );
                let turn_key = pick_session_key(effective.as_deref(), &session_key);

                // Acquire session lock to serialize concurrent turns
                let _session_guard = match state.session_queue.acquire(&turn_key).await {
                    Ok(guard) => guard,
                    Err(e) => {
                        let err = serde_json::json!({
                            "type": "error",
                            "message": e.to_string(),
                            "code": "SESSION_BUSY"
                        });
                        let _ = sender.send(Message::Text(err.to_string().into())).await;
                        continue;
                    }
                };

                agent.set_current_thread_id(effective.clone());

                // Persist user message
                if let Some(ref backend) = state.session_backend {
                    let user_msg = crate::providers::ChatMessage::user(&content);
                    let _ = backend.append(&turn_key, &user_msg);
                }

                process_chat_message(
                    &state,
                    &mut agent,
                    &mut sender,
                    &content,
                    &session_key,
                    effective.as_deref(),
                )
                .await;
            }

            // ── Broadcast event (cron/heartbeat results) ──────────────
            event = broadcast_rx.recv() => {
                if let Ok(event) = event {
                    let _ = sender.send(Message::Text(event.to_string().into())).await;
                }
            }
        }
    }
}

/// Process a single chat message through the agent and send the response.
///
/// Uses [`Agent::turn_streamed`] so that intermediate text chunks, tool calls,
/// and tool results are forwarded to the WebSocket client in real time.
async fn process_chat_message(
    state: &AppState,
    agent: &mut crate::agent::Agent,
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    content: &str,
    session_key: &str,
    thread_id: Option<&str>,
) {
    use crate::agent::TurnEvent;

    // Derive the storage key once per turn. Thread-scoped when a thread id
    // is present, else the connection's session key — see [`pick_session_key`].
    let storage_key = pick_session_key(thread_id, session_key);

    let provider_label = state
        .config
        .lock()
        .default_provider
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    // Broadcast agent_start event
    let _ = state.event_tx.send(serde_json::json!({
        "type": "agent_start",
        "provider": provider_label,
        "model": state.model,
    }));

    // Set session state to running
    let turn_id = uuid::Uuid::new_v4().to_string();
    if let Some(ref backend) = state.session_backend {
        let _ = backend.set_session_state(&storage_key, "running", Some(&turn_id));
    }

    // Channel for streaming turn events from the agent.
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<TurnEvent>(64);

    // Run the streamed turn concurrently: the agent produces events
    // while we forward them to the WebSocket below.  We cannot move
    // `agent` into a spawned task (it is `&mut`), so we use a join
    // instead — `turn_streamed` writes to the channel and we drain it
    // from the other branch.
    let content_owned = content.to_string();
    let turn_fut = async { agent.turn_streamed(&content_owned, event_tx).await };

    // Drive both futures concurrently: the agent turn produces events
    // and we relay them over WebSocket.
    let forward_fut = async {
        while let Some(event) = event_rx.recv().await {
            let ws_msg = match event {
                TurnEvent::Chunk { delta } => {
                    serde_json::json!({ "type": "chunk", "content": delta })
                }
                TurnEvent::Thinking { delta } => {
                    serde_json::json!({ "type": "thinking", "content": delta })
                }
                TurnEvent::ToolCall { name, args } => {
                    serde_json::json!({ "type": "tool_call", "name": name, "args": args })
                }
                TurnEvent::ToolResult { name, output } => {
                    serde_json::json!({ "type": "tool_result", "name": name, "output": output })
                }
            };
            let _ = sender.send(Message::Text(ws_msg.to_string().into())).await;
        }
    };

    let (result, ()) = tokio::join!(turn_fut, forward_fut);

    match result {
        Ok(response) => {
            // Persist assistant response
            if let Some(ref backend) = state.session_backend {
                let assistant_msg = crate::providers::ChatMessage::assistant(&response);
                let _ = backend.append(&storage_key, &assistant_msg);
            }

            // Fire-and-forget memory consolidation so facts from WS sessions
            // are extracted to long-term memory (Daily + Core categories).
            if state.auto_save {
                let mem = state.mem.clone();
                let provider = state.provider.clone();
                let model = state.model.clone();
                let user_msg = content.to_string();
                let assistant_resp = response.clone();
                tokio::spawn(async move {
                    if let Err(e) = crate::memory::consolidation::consolidate_turn(
                        provider.as_ref(),
                        &model,
                        mem.as_ref(),
                        &user_msg,
                        &assistant_resp,
                    )
                    .await
                    {
                        tracing::debug!("WS memory consolidation skipped: {e}");
                    }
                });
            }

            // Send chunk_reset so the client clears any accumulated draft
            // before the authoritative done message.
            let reset = serde_json::json!({ "type": "chunk_reset" });
            let _ = sender.send(Message::Text(reset.to_string().into())).await;

            let done = serde_json::json!({
                "type": "done",
                "full_response": response,
            });
            let _ = sender.send(Message::Text(done.to_string().into())).await;

            // Set session state to idle
            if let Some(ref backend) = state.session_backend {
                let _ = backend.set_session_state(&storage_key, "idle", None);
            }

            // Broadcast agent_end event
            let _ = state.event_tx.send(serde_json::json!({
                "type": "agent_end",
                "provider": provider_label,
                "model": state.model,
            }));
        }
        Err(e) => {
            // Set session state to error
            if let Some(ref backend) = state.session_backend {
                let _ = backend.set_session_state(&storage_key, "error", Some(&turn_id));
            }

            tracing::error!(error = %e, "Agent turn failed");
            let sanitized = crate::providers::sanitize_api_error(&e.to_string());
            let error_code = if sanitized.to_lowercase().contains("api key")
                || sanitized.to_lowercase().contains("authentication")
                || sanitized.to_lowercase().contains("unauthorized")
            {
                "AUTH_ERROR"
            } else if sanitized.to_lowercase().contains("provider")
                || sanitized.to_lowercase().contains("model")
            {
                "PROVIDER_ERROR"
            } else {
                "AGENT_ERROR"
            };
            let err = serde_json::json!({
                "type": "error",
                "message": sanitized,
                "code": error_code,
            });
            let _ = sender.send(Message::Text(err.to_string().into())).await;

            // Broadcast error event
            let _ = state.event_tx.send(serde_json::json!({
                "type": "error",
                "component": "ws_chat",
                "message": sanitized,
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    // ── classify_hydration ──────────────────────────────────────────
    // Pure branch-decision helper used by hydrate_thread_if_changed.

    #[test]
    fn classify_hydration_first_thread_seeds() {
        assert_eq!(classify_hydration(None, "tid_1"), HydrationAction::Seed);
    }

    #[test]
    fn classify_hydration_same_thread_skips() {
        assert_eq!(
            classify_hydration(Some("tid_1"), "tid_1"),
            HydrationAction::Skip
        );
    }

    #[test]
    fn classify_hydration_thread_switch_replaces() {
        assert_eq!(
            classify_hydration(Some("tid_1"), "tid_2"),
            HydrationAction::Replace
        );
    }

    // ── effective_thread_id ─────────────────────────────────────────
    // v1 forgiving no-threadId policy resolver.

    #[test]
    fn effective_thread_id_prefers_inbound_when_present() {
        assert_eq!(
            effective_thread_id(Some("tid_inbound"), Some("tid_hydrated"), "sid_x"),
            Some("tid_inbound".to_string()),
            "inbound threadId must win even when a different thread is hydrated"
        );
    }

    #[test]
    fn effective_thread_id_falls_back_to_hydrated_on_missing_inbound() {
        assert_eq!(
            effective_thread_id(None, Some("tid_hydrated"), "sid_x"),
            Some("tid_hydrated".to_string()),
            "missing inbound threadId must continue the hydrated thread (v1 forgiving policy)"
        );
    }

    #[test]
    fn effective_thread_id_returns_none_when_neither_present() {
        assert_eq!(
            effective_thread_id(None, None, "sid_x"),
            None,
            "no inbound and no hydrated thread means legacy connection-scoped flow"
        );
    }

    #[test]
    fn extract_ws_token_from_authorization_header() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer zc_test123".parse().unwrap());
        assert_eq!(extract_ws_token(&headers, None), Some("zc_test123"));
    }

    #[test]
    fn extract_ws_token_from_subprotocol() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "sec-websocket-protocol",
            "zeroclaw.v1, bearer.zc_sub456".parse().unwrap(),
        );
        assert_eq!(extract_ws_token(&headers, None), Some("zc_sub456"));
    }

    #[test]
    fn extract_ws_token_from_query_param() {
        let headers = HeaderMap::new();
        assert_eq!(
            extract_ws_token(&headers, Some("zc_query789")),
            Some("zc_query789")
        );
    }

    #[test]
    fn extract_ws_token_precedence_header_over_subprotocol() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer zc_header".parse().unwrap());
        headers.insert("sec-websocket-protocol", "bearer.zc_sub".parse().unwrap());
        assert_eq!(
            extract_ws_token(&headers, Some("zc_query")),
            Some("zc_header")
        );
    }

    #[test]
    fn extract_ws_token_precedence_subprotocol_over_query() {
        let mut headers = HeaderMap::new();
        headers.insert("sec-websocket-protocol", "bearer.zc_sub".parse().unwrap());
        assert_eq!(extract_ws_token(&headers, Some("zc_query")), Some("zc_sub"));
    }

    #[test]
    fn extract_ws_token_returns_none_when_empty() {
        let headers = HeaderMap::new();
        assert_eq!(extract_ws_token(&headers, None), None);
    }

    #[test]
    fn extract_ws_token_skips_empty_header_value() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer ".parse().unwrap());
        assert_eq!(
            extract_ws_token(&headers, Some("zc_fallback")),
            Some("zc_fallback")
        );
    }

    #[test]
    fn extract_ws_token_skips_empty_query_param() {
        let headers = HeaderMap::new();
        assert_eq!(extract_ws_token(&headers, Some("")), None);
    }

    #[test]
    fn extract_ws_token_subprotocol_with_multiple_entries() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "sec-websocket-protocol",
            "zeroclaw.v1, bearer.zc_tok, other".parse().unwrap(),
        );
        assert_eq!(extract_ws_token(&headers, None), Some("zc_tok"));
    }

    // ── parse_thread_id ─────────────────────────────────────────────
    // Covers the WsClientMessage envelope parse used by /ws/chat to
    // identify the active clawcraft thread (B2 `thread-spec-binding`).

    #[test]
    fn parse_thread_id_prefers_typed_field_over_prefix() {
        let parsed = serde_json::json!({
            "type": "message",
            "threadId": "tid_typed",
            "content": "[conversationId: tid_legacy]\nhello",
        });
        let raw = parsed["content"].as_str().unwrap();
        let (tid, content, src) = parse_thread_id(&parsed, raw);
        assert_eq!(tid.as_deref(), Some("tid_typed"));
        assert_eq!(src, ThreadIdSource::TypedField);
        // Typed field wins → legacy prefix is NOT stripped (content untouched).
        assert_eq!(content, "[conversationId: tid_legacy]\nhello");
    }

    #[test]
    fn parse_thread_id_falls_back_to_legacy_prefix() {
        let parsed = serde_json::json!({
            "type": "message",
            "content": "[conversationId: tid_legacy]\nhello world",
        });
        let raw = parsed["content"].as_str().unwrap();
        let (tid, content, src) = parse_thread_id(&parsed, raw);
        assert_eq!(tid.as_deref(), Some("tid_legacy"));
        assert_eq!(src, ThreadIdSource::PrefixFallback);
        assert_eq!(content, "hello world");
    }

    #[test]
    fn parse_thread_id_returns_none_when_neither_present() {
        let parsed = serde_json::json!({
            "type": "message",
            "content": "just a plain message",
        });
        let raw = parsed["content"].as_str().unwrap();
        let (tid, content, src) = parse_thread_id(&parsed, raw);
        assert!(tid.is_none());
        assert_eq!(src, ThreadIdSource::None);
        assert_eq!(content, "just a plain message");
    }

    #[test]
    fn parse_thread_id_leaves_unrelated_prefixes_untouched() {
        let parsed = serde_json::json!({
            "type": "message",
            "content": "[SYSTEM] note something\n[MEDIA_UPLOAD: foo.png] continues",
        });
        let raw = parsed["content"].as_str().unwrap();
        let (tid, content, src) = parse_thread_id(&parsed, raw);
        assert!(tid.is_none());
        assert_eq!(src, ThreadIdSource::None);
        assert_eq!(
            content,
            "[SYSTEM] note something\n[MEDIA_UPLOAD: foo.png] continues"
        );
    }

    #[test]
    fn parse_thread_id_trims_captured_group_whitespace() {
        let parsed = serde_json::json!({
            "type": "message",
            "content": "[conversationId:   tid_padded   ]\nhi",
        });
        let raw = parsed["content"].as_str().unwrap();
        let (tid, content, src) = parse_thread_id(&parsed, raw);
        assert_eq!(tid.as_deref(), Some("tid_padded"));
        assert_eq!(src, ThreadIdSource::PrefixFallback);
        assert_eq!(content, "hi");
    }

    #[test]
    fn parse_thread_id_treats_empty_typed_field_as_absent() {
        let parsed = serde_json::json!({
            "type": "message",
            "threadId": "   ",
            "content": "[conversationId: tid_legacy]\nfallback please",
        });
        let raw = parsed["content"].as_str().unwrap();
        let (tid, content, src) = parse_thread_id(&parsed, raw);
        assert_eq!(tid.as_deref(), Some("tid_legacy"));
        assert_eq!(src, ThreadIdSource::PrefixFallback);
        assert_eq!(content, "fallback please");
    }

    #[test]
    fn parse_thread_id_handles_real_emission_format() {
        // The exact emission shape from clawcraft's pre-B2 relay:
        //   `[conversationId: <id>]\n<content>`
        // Verifies the inner `\s*` after `:` and the `\n` separator both match.
        let parsed = serde_json::json!({
            "type": "message",
            "content": "[conversationId: j97abc123]\nWhat's the weather?",
        });
        let raw = parsed["content"].as_str().unwrap();
        let (tid, content, src) = parse_thread_id(&parsed, raw);
        assert_eq!(tid.as_deref(), Some("j97abc123"));
        assert_eq!(src, ThreadIdSource::PrefixFallback);
        assert_eq!(content, "What's the weather?");
    }

    #[test]
    fn parse_thread_id_source_string_labels() {
        assert_eq!(ThreadIdSource::TypedField.as_str(), "typed_field");
        assert_eq!(ThreadIdSource::PrefixFallback.as_str(), "prefix_fallback");
        assert_eq!(ThreadIdSource::None.as_str(), "none");
    }
}
