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
