//! Pod-scoped end-user identity for telemetry attribution.
//!
//! ZeroClaw runs one pod per user; the clawcraft K8s deployment injects the
//! owning user's id as the `CLAW_USER_ID` env var. That id is the stable
//! `user.id` span attribute carried by user-facing activation roots (web,
//! webhook, native channels, channel webhooks) — never CLI/cron, and never a
//! resource attribute (resource attrs are process-global and must not carry
//! PII; see `otel::build_otlp_resource`). It also rides as the `user` field on
//! OpenRouter chat completions (see `zeroclaw-providers::openrouter`).
//!
//! The `CLAW_USER_ID` shape rule (`^[a-z0-9]{32}$`) lives as the single source
//! in [`zeroclaw_api::identity`], imported here so the OpenRouter provider can
//! reuse the exact same validator without duplicating the regex.

use super::{AttrValue, Span, Trigger};
use zeroclaw_api::identity::validate_pod_user_id;

/// Map a delivery-channel label (the `TurnAttribution.channel` / gateway /
/// channel-orchestrator string) to the coarse activation [`Trigger`] recorded
/// on the root span. Native channel listeners (`telegram`, `slack`, `matrix`,
/// `whatsapp`, `wecom_ws`, …) collapse to [`Trigger::Channel`] — the concrete
/// channel name rides separately via [`tag_channel`]. `web`/`wss`/`ws` are web
/// chat; `webhook` is the inbound webhook; `cli` is the local CLI; `cron` /
/// `self_schedule` is the autonomous scheduler. Unknown labels fall back to
/// [`Trigger::Channel`] (a delivery channel we have not enumerated) rather than
/// mislabeling as CLI.
pub fn trigger_for_channel(channel: &str) -> Trigger {
    match channel {
        "web" | "wss" | "ws" | "web_chat" | "rpc" => Trigger::WebChat,
        "webhook" => Trigger::Webhook,
        "cli" => Trigger::Cli,
        "cron" | "self_schedule" | "scheduler" | "heartbeat" => Trigger::SelfSchedule,
        _ => Trigger::Channel,
    }
}

/// Read `CLAW_USER_ID` from the environment (set by the clawcraft K8s
/// deployment) and validate the shape. Returns `None` when unset or malformed —
/// callers must treat the absence as "no user", never synthesize a stand-in.
pub fn pod_user_id() -> Option<String> {
    std::env::var("CLAW_USER_ID")
        .ok()
        .as_deref()
        .and_then(validate_pod_user_id)
}

/// Tag a root activation span with the pod-owner user id for per-user
/// attribution. Sets BOTH attribution keys under one guard, so the five owner
/// call sites stay one-liners and the dual-key contract cannot drift:
///
/// - `user.id` — vendor-neutral OTel semconv key (lands in the raw span
///   attributes; what generic OTLP consumers read).
/// - `lmnr.association.properties.user_id` — Laminar's association-property
///   key, which self-hosted Laminar projects into its typed, indexed `user_id`
///   column (the column that drives the UI's per-user filter). The OTel key
///   alone never populates it. This mirrors the `deployment.environment`
///   dual-emit precedent — self-hosted Laminar keys on its own conventions.
///
/// No-op when `CLAW_USER_ID` is unset/invalid, so absence yields neither key
/// (never synthesized). Call only on user-facing roots — never CLI/cron.
pub fn tag_user_id(span: &dyn Span) {
    if let Some(uid) = pod_user_id() {
        set_user_id_attrs(span, &uid);
    }
}

/// Set both user-id attribution keys on `span`. Split out from [`tag_user_id`]
/// so the dual-key contract is unit-testable without manipulating the
/// process-global `CLAW_USER_ID` env var.
fn set_user_id_attrs(span: &dyn Span, uid: &str) {
    span.set_attr("user.id", AttrValue::Str(uid.to_string()));
    span.set_attr(
        "lmnr.association.properties.user_id",
        AttrValue::Str(uid.to_string()),
    );
}

/// Tag a root activation span with its delivery channel for Laminar's typed
/// Tags trace-list column. Sets BOTH keys under one guard so the four root call
/// sites stay one-liners and the dual-key contract cannot drift:
///
/// - `channel` — vendor-neutral plain attribute (the OTel-native copy that
///   generic OTLP consumers read; the observability doctrine enumerates it).
/// - `lmnr.association.properties.tags` — Laminar's association-property key,
///   which self-hosted Laminar projects into its typed, indexed `tags_array`
///   (`Array(String)`) column — the one that drives the trace-list Tags filter.
///   Emitted as a **native OTLP string array** (`["web"]`); the plain `channel`
///   attr alone never populates it, and a JSON-encoded string would land as one
///   literal element. Mirrors the `user.id` / `lmnr.association.properties.user_id`
///   dual-emit precedent — self-hosted Laminar keys on its own conventions.
///
/// Single-element (`[channel]`) on purpose: adding the trigger as a second tag
/// is gated on a live multi-element render check (see bead zc-khnl). Call only
/// on activation roots that carry a channel — never CLI/cron (no channel), and
/// never the `delivery` child (tags belong on the root for the trace-list).
pub fn tag_channel(span: &dyn Span, channel: &str) {
    span.set_attr("channel", AttrValue::Str(channel.to_string()));
    span.set_attr(
        "lmnr.association.properties.tags",
        AttrValue::Array(vec![channel.to_string()]),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeroclaw_api::identity::validate_pod_user_id;

    #[test]
    fn validate_pod_user_id_accepts_canonical_id() {
        let id = "kd76fb7wr1pba28mavncxb8pnd84r3mn";
        assert_eq!(validate_pod_user_id(id).as_deref(), Some(id));
    }

    #[test]
    fn validate_pod_user_id_rejects_claw_prefix() {
        // A bare-id contract: anything wrapped in `claw-` is rejected so we
        // never accidentally pass the namespace shape upstream.
        assert!(validate_pod_user_id("claw-kd76fb7wr1pba28mavncxb8pnd84r3mn").is_none());
    }

    #[test]
    fn validate_pod_user_id_rejects_wrong_length() {
        // 31 chars
        assert!(validate_pod_user_id("kd76fb7wr1pba28mavncxb8pnd84r3m").is_none());
        // 33 chars
        assert!(validate_pod_user_id("kd76fb7wr1pba28mavncxb8pnd84r3mnx").is_none());
        assert!(validate_pod_user_id("abc").is_none());
        assert!(validate_pod_user_id("").is_none());
    }

    #[test]
    fn validate_pod_user_id_rejects_non_lowercase_alphanumeric() {
        assert!(validate_pod_user_id("KD76FB7WR1PBA28MAVNCXB8PND84R3MN").is_none());
        assert!(validate_pod_user_id("kd76fb7wr1pba28mavncxb8pnd84r-mn").is_none());
        assert!(validate_pod_user_id("kd76fb7wr1pba28mavncxb8pnd84r_mn").is_none());
    }

    #[test]
    fn validate_pod_user_id_rejects_surrounding_whitespace() {
        assert!(validate_pod_user_id(" kd76fb7wr1pba28mavncxb8pnd84r3mn").is_none());
        assert!(validate_pod_user_id("kd76fb7wr1pba28mavncxb8pnd84r3mn\n").is_none());
    }

    /// Records every `set_attr` string call so a test can assert the dual-key
    /// contract without a live OTel span or env manipulation.
    struct RecordingSpan(std::sync::Mutex<Vec<(String, String)>>);

    impl Span for RecordingSpan {
        fn child(&self, _name: &str) -> Box<dyn Span> {
            Box::new(crate::observability::NoopSpan)
        }
        fn set_attr(&self, key: &str, value: AttrValue) {
            // Record Str verbatim and Array as `[a,b]` so both the user_id (Str)
            // and tags (Array) contracts are assertable without a live span.
            let recorded = match value {
                AttrValue::Str(s) => Some(s),
                AttrValue::Array(items) => Some(format!("[{}]", items.join(","))),
                AttrValue::Int(_) | AttrValue::Float(_) | AttrValue::Bool(_) => None,
            };
            if let Some(v) = recorded {
                self.0.lock().unwrap().push((key.to_string(), v));
            }
        }
        fn set_status(&self, _ok: bool) {}
    }

    #[test]
    fn set_user_id_attrs_sets_both_otel_and_laminar_keys() {
        let id = "kd76fb7wr1pba28mavncxb8pnd84r3mn";
        let span = RecordingSpan(std::sync::Mutex::new(Vec::new()));
        set_user_id_attrs(&span, id);
        let attrs = span.0.lock().unwrap();
        // OTel semconv key — what generic OTLP consumers read.
        assert!(attrs.iter().any(|(k, v)| k == "user.id" && v == id));
        // Laminar association-property key — drives the typed `user_id` column.
        assert!(
            attrs
                .iter()
                .any(|(k, v)| k == "lmnr.association.properties.user_id" && v == id)
        );
    }

    #[test]
    fn tag_channel_sets_plain_attr_and_laminar_tags_array() {
        let span = RecordingSpan(std::sync::Mutex::new(Vec::new()));
        tag_channel(&span, "web");
        let attrs = span.0.lock().unwrap();
        // OTel-native plain attr — what generic OTLP consumers read.
        assert!(attrs.iter().any(|(k, v)| k == "channel" && v == "web"));
        // Laminar association-property key — a single-element native array that
        // drives the typed `tags_array` column (recorded as `[web]`).
        assert!(
            attrs
                .iter()
                .any(|(k, v)| k == "lmnr.association.properties.tags" && v == "[web]")
        );
    }

    #[test]
    fn trigger_for_channel_maps_ingress_labels() {
        assert_eq!(trigger_for_channel("web"), Trigger::WebChat);
        assert_eq!(trigger_for_channel("wss"), Trigger::WebChat);
        assert_eq!(trigger_for_channel("rpc"), Trigger::WebChat);
        assert_eq!(trigger_for_channel("webhook"), Trigger::Webhook);
        assert_eq!(trigger_for_channel("cli"), Trigger::Cli);
        assert_eq!(trigger_for_channel("cron"), Trigger::SelfSchedule);
        assert_eq!(trigger_for_channel("heartbeat"), Trigger::SelfSchedule);
        // Native channel listeners collapse to Channel; the concrete name rides
        // on `tag_channel`. Unknown labels are Channel (not mislabeled as Cli).
        assert_eq!(trigger_for_channel("telegram"), Trigger::Channel);
        assert_eq!(trigger_for_channel("slack"), Trigger::Channel);
        assert_eq!(trigger_for_channel("matrix"), Trigger::Channel);
        assert_eq!(trigger_for_channel("some_future_channel"), Trigger::Channel);
    }
}
