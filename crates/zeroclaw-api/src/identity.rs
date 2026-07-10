//! Shared pod-user-id validation — the single source of the `CLAW_USER_ID`
//! shape rule.
//!
//! ZeroClaw runs one pod per user; the clawcraft K8s deployment injects the
//! owning user's id as the `CLAW_USER_ID` env var. That id must be a bare
//! 32-char lowercase alphanumeric identifier (Convex document id shape),
//! historically expressed as the regex `^[a-z0-9]{32}$`.
//!
//! This validator lives in `zeroclaw-api` (the lowest, dependency-free layer)
//! so that **both** the runtime crate's telemetry attribution
//! (`observability::identity`) and the providers crate's OpenRouter `user`
//! field can import the *same* shape rule instead of duplicating the regex.
//! `zeroclaw-providers` does not depend on `zeroclaw-runtime` (the dependency
//! runs the other way), but both depend on `zeroclaw-api` — so this is their
//! shared home.
//!
//! The check is expressed as a pure `char` scan rather than via the `regex`
//! crate, keeping the API layer free of a heavy dependency while remaining
//! behaviourally identical to `^[a-z0-9]{32}$`.

/// True iff `value` is a bare 32-char lowercase-alphanumeric identifier
/// (equivalent to matching `^[a-z0-9]{32}$`). This is the single source of the
/// pod-user-id shape rule — every caller that needs the `CLAW_USER_ID` shape
/// must go through this function rather than re-encoding the regex.
///
/// Rejects anything with a `claw-` prefix, surrounding whitespace, wrong
/// length, uppercase, or a non-`[a-z0-9]` character.
pub fn is_valid_pod_user_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// Pure validation of a **bare** id: accept the value only if it is a bare
/// 32-char lowercase alphanumeric identifier. Rejects anything with a `claw-`
/// prefix, whitespace, wrong length, or wrong alphabet. Returns the id verbatim
/// on success, `None` otherwise — callers must treat `None` as "no user", never
/// synthesize a stand-in (absence-not-empty).
pub fn validate_pod_user_id(value: &str) -> Option<String> {
    if is_valid_pod_user_id(value) {
        Some(value.to_string())
    } else {
        None
    }
}

/// Validation for a `CLAW_USER_ID` value that may carry the `claw-` namespace
/// prefix: strip a single leading `claw-`, then validate the bare id shape.
/// Returns the **bare** id (prefix removed) on success, `None` otherwise.
///
/// Used by the OpenRouter provider, which accepts the namespaced form and must
/// forward only the bare id as the `user` field. Shares the exact same shape
/// gate ([`is_valid_pod_user_id`]) as the bare-id path so the two never drift.
pub fn validate_claw_user_id(value: &str) -> Option<String> {
    let bare = value.strip_prefix("claw-").unwrap_or(value);
    validate_pod_user_id(bare)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CANONICAL: &str = "kd76fb7wr1pba28mavncxb8pnd84r3mn";

    #[test]
    fn validate_pod_user_id_accepts_canonical_id() {
        assert_eq!(validate_pod_user_id(CANONICAL).as_deref(), Some(CANONICAL));
        assert!(is_valid_pod_user_id(CANONICAL));
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

    #[test]
    fn validate_claw_user_id_strips_prefix_and_returns_bare() {
        // The namespaced form is accepted and the bare id is returned.
        assert_eq!(
            validate_claw_user_id("claw-kd76fb7wr1pba28mavncxb8pnd84r3mn").as_deref(),
            Some(CANONICAL)
        );
        // A bare id (no prefix) is accepted verbatim.
        assert_eq!(validate_claw_user_id(CANONICAL).as_deref(), Some(CANONICAL));
    }

    #[test]
    fn validate_claw_user_id_rejects_malformed_after_strip() {
        // Wrong shape after stripping the prefix ⇒ None (absence, not empty).
        assert!(validate_claw_user_id("claw-abc").is_none());
        assert!(validate_claw_user_id("claw-").is_none());
        assert!(validate_claw_user_id("").is_none());
    }
}
