//! Ambient activation span — the parent for spans created deep in the agent loop
//! without threading a handle through every function signature.
//!
//! An activation owner (CLI `run`, channel dispatch, webhook, WS message, cron) mints a
//! root [`Span`] and runs its work inside [`scope_span`]. Code anywhere below —
//! `run_tool_call_loop`, `execute_one_tool` — opens children of [`current_span`]. Because
//! owners sit on the far side of every task spawn, the ambient span is visible across the
//! whole awaited call chain; tasks spawned for side effects (typing indicators, draft
//! updates) simply observe no ambient span, which is fine — they carry no agent intent.
//!
//! This mirrors the existing `TOOL_LOOP_COST_TRACKING_CONTEXT` task-local in the agent
//! loop. The stored value is our own `Arc<dyn Span>` (`Send + Sync`), so it has none of the
//! `!Send` hazards of OpenTelemetry's `ContextGuard`.

use std::future::Future;
use std::sync::Arc;

use super::traits::{AttrValue, Span};

tokio::task_local! {
    static ACTIVE_SPAN: Arc<dyn Span>;
}

/// Run `fut` with `span` as the ambient parent span for everything it awaits.
///
/// Nesting is supported: calling this again inside `fut` (e.g. to scope a `tool.call`
/// span around a tool's execution) overrides the ambient span for that inner scope, so a
/// delegated sub-agent's spans parent beneath the `tool.call` span automatically.
pub async fn scope_span<F>(span: Arc<dyn Span>, fut: F) -> F::Output
where
    F: Future,
{
    ACTIVE_SPAN.scope(span, fut).await
}

/// The current ambient parent span, if inside an activation scope; `None` otherwise
/// (e.g. tests, or code paths not yet wrapped by an owner).
pub fn current_span() -> Option<Arc<dyn Span>> {
    ACTIVE_SPAN.try_with(|s| s.clone()).ok()
}

/// Stamp the queryable turn-outcome (`agent.turn.exit_reason` + `agent.turn.iterations`)
/// on the ambient root span at a loop terminal point (zc-ug3w).
///
/// Set from inside the tool-call loops (`loop_::run_tool_call_loop`,
/// `Agent::turn`/`turn_streamed`) because the iteration count and the
/// final_answer-vs-max_iterations distinction live there, not at the root
/// `set_status` sites where both surface as `Ok(String)`. `current_span()` at a
/// loop-body terminal resolves to the activation root (`llm.call`/`tool.call`
/// children are transient and out of scope here). No-op outside an activation
/// scope (tests, untraced paths). Both values are structural enum/int — non-PII,
/// prod-safe, no scrub/truncation/gate. The `agent.turn.status` twin is set at
/// the root sites alongside the native OTel `set_status`.
pub fn stamp_turn_exit(reason: &str, iterations: usize) {
    if let Some(sp) = current_span() {
        sp.set_attr("agent.turn.exit_reason", AttrValue::Str(reason.to_string()));
        sp.set_attr("agent.turn.iterations", AttrValue::Int(iterations as i64));
    }
}

/// Root I/O mirrors are truncated to this many chars before export — matches the
/// `llm.call` mirror budget in the agent loop and the 0.6.9 fork's root sites.
const ROOT_IO_MAX_CHARS: usize = 16_000;

/// Mirror the turn's triggering user message onto the ambient activation root as
/// `lmnr.span.input` (FD-07 follow-up, zc-gnpx). Laminar derives its root-span
/// *input* view from this attribute (its manual-override path); a bare
/// `gen_ai.prompt` is never read for the root, so without this the root's typed
/// `input` column is silently empty. Unlike [`stamp_turn_exit`], the payload is
/// PII-bearing content, so it is credential-scrubbed and truncated once here.
/// Called at each tool loop's entry, where the triggering message is known; a
/// no-op outside an activation scope (tests, untraced paths).
pub fn stamp_root_input(raw: &str) {
    if let Some(sp) = current_span() {
        let value = crate::util::truncate_with_ellipsis(
            &crate::agent::loop_::scrub_credentials(raw),
            ROOT_IO_MAX_CHARS,
        );
        sp.set_attr("lmnr.span.input", AttrValue::Str(value));
    }
}

/// Mirror the turn's final assistant response onto the ambient activation root as
/// `lmnr.span.output` (FD-07 follow-up, zc-gnpx). Laminar reads its root-span
/// *output* view from this attribute. Scrubbed + truncated exactly like
/// [`stamp_root_input`]. Called at each tool loop's final-answer terminals (the
/// success returns, never the error/`max_iterations` bail — mirroring the fork's
/// `Ok(text)`-only rule); a no-op outside an activation scope.
pub fn stamp_root_output(raw: &str) {
    if let Some(sp) = current_span() {
        let value = crate::util::truncate_with_ellipsis(
            &crate::agent::loop_::scrub_credentials(raw),
            ROOT_IO_MAX_CHARS,
        );
        sp.set_attr("lmnr.span.output", AttrValue::Str(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Span that records every `set_attr` so a test can assert the ambient-span
    /// wiring the FD-07 ingress owners depend on: an owner mints a root and runs
    /// its turn inside `scope_span`; code deep in the loop reaches it via
    /// `current_span()` and `stamp_turn_exit`.
    struct RecordingSpan(Mutex<Vec<(String, AttrValue)>>);

    impl Span for RecordingSpan {
        fn child(&self, _name: &str) -> Box<dyn Span> {
            Box::new(crate::observability::NoopSpan)
        }
        fn set_attr(&self, key: &str, value: AttrValue) {
            self.0.lock().unwrap().push((key.to_string(), value));
        }
        fn set_status(&self, _ok: bool) {}
    }

    #[test]
    fn current_span_is_none_outside_a_scope() {
        assert!(
            current_span().is_none(),
            "no ambient span outside an activation scope"
        );
    }

    #[tokio::test]
    async fn scope_span_makes_current_span_and_stamp_turn_exit_reach_the_root() {
        let root = Arc::new(RecordingSpan(Mutex::new(Vec::new())));
        let root_dyn: Arc<dyn Span> = root.clone();

        scope_span(root_dyn, async {
            // A deep call site resolves the ambient root and stamps the outcome
            // — this is exactly what `run_tool_call_loop`/`Agent::turn` do at
            // their terminal points.
            assert!(current_span().is_some(), "ambient root visible under scope");
            stamp_turn_exit("final_answer", 3);
        })
        .await;

        let attrs = root.0.lock().unwrap();
        assert!(
            attrs.iter().any(|(k, v)| k == "agent.turn.exit_reason"
                && matches!(v, AttrValue::Str(s) if s == "final_answer")),
            "stamp_turn_exit must write exit_reason onto the ambient root"
        );
        assert!(
            attrs
                .iter()
                .any(|(k, v)| k == "agent.turn.iterations" && matches!(v, AttrValue::Int(3))),
            "stamp_turn_exit must write the iteration count onto the ambient root"
        );
    }

    #[tokio::test]
    async fn stamp_root_io_writes_scrubbed_input_output_onto_the_ambient_root() {
        let root = Arc::new(RecordingSpan(Mutex::new(Vec::new())));
        let root_dyn: Arc<dyn Span> = root.clone();

        scope_span(root_dyn, async {
            // Loop entry mirrors the triggering message; the final-answer
            // terminal mirrors the response — both reach the ambient root.
            stamp_root_input("what is the capital of france?");
            stamp_root_output("The capital of France is Paris.");
        })
        .await;

        let attrs = root.0.lock().unwrap();
        assert!(
            attrs.iter().any(|(k, v)| k == "lmnr.span.input"
                && matches!(v, AttrValue::Str(s) if s.contains("capital of france"))),
            "stamp_root_input must write the triggering message to lmnr.span.input on the root"
        );
        assert!(
            attrs.iter().any(|(k, v)| k == "lmnr.span.output"
                && matches!(v, AttrValue::Str(s) if s.contains("Paris"))),
            "stamp_root_output must write the final response to lmnr.span.output on the root"
        );
    }

    #[test]
    fn stamp_root_io_are_noops_without_an_activation_scope() {
        // Outside a scope there is no ambient root; the mirrors must not panic.
        stamp_root_input("no scope here");
        stamp_root_output("no scope here");
        assert!(current_span().is_none());
    }

    #[tokio::test]
    async fn nested_scope_span_overrides_inner_ambient_span() {
        let outer = Arc::new(RecordingSpan(Mutex::new(Vec::new())));
        let inner = Arc::new(RecordingSpan(Mutex::new(Vec::new())));
        let outer_dyn: Arc<dyn Span> = outer.clone();
        let inner_dyn: Arc<dyn Span> = inner.clone();

        scope_span(outer_dyn, async move {
            scope_span(inner_dyn, async {
                stamp_turn_exit("final_answer", 1);
            })
            .await;
        })
        .await;

        assert!(
            !inner.0.lock().unwrap().is_empty(),
            "the innermost scope wins as the ambient span"
        );
        assert!(
            outer.0.lock().unwrap().is_empty(),
            "the outer span must not receive the inner scope's stamp"
        );
    }
}
