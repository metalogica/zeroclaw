//! Pod-scoped end-user identity for telemetry attribution.
//!
//! ZeroClaw runs one pod per user; the clawcraft K8s deployment injects the
//! owning user's id as the `CLAW_USER_ID` env var. That id is the stable
//! `user.id` span attribute carried by user-facing activation roots (web,
//! webhook, native channels, channel webhooks) — never CLI/cron, and never a
//! resource attribute (resource attrs are process-global and must not carry
//! PII; see `otel::build_otlp_resource`). It also rides as the `user` field on
//! OpenRouter chat completions (see `providers::openrouter`).

use regex::Regex;
use std::sync::LazyLock;

static USER_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9]{32}$").expect("valid regex"));

/// Pure validation: accept the value only if it's a bare 32-char lowercase
/// alphanumeric identifier (Convex document id shape). Rejects anything with
/// a `claw-` prefix, whitespace, wrong length, or wrong alphabet.
fn validate_pod_user_id(value: &str) -> Option<String> {
    if USER_ID_RE.is_match(value) {
        Some(value.to_string())
    } else {
        None
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
