use std::time::Duration;

/// Discrete events emitted by the agent runtime for observability.
///
/// Each variant represents a lifecycle event that observers can record,
/// aggregate, or forward to external monitoring systems. Events carry
/// just enough context for tracing and diagnostics without exposing
/// sensitive prompt or response content.
#[derive(Debug, Clone)]
pub enum ObserverEvent {
    /// The agent orchestration loop has started a new session.
    AgentStart { provider: String, model: String },
    /// A request is about to be sent to an LLM provider.
    ///
    /// This is emitted immediately before a provider call so observers can print
    /// user-facing progress without leaking prompt contents.
    LlmRequest {
        provider: String,
        model: String,
        messages_count: usize,
    },
    /// Result of a single LLM provider call.
    LlmResponse {
        provider: String,
        model: String,
        duration: Duration,
        success: bool,
        error_message: Option<String>,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    },
    /// The agent session has finished.
    ///
    /// Carries aggregate usage data (tokens, cost) when the provider reports it.
    AgentEnd {
        provider: String,
        model: String,
        duration: Duration,
        tokens_used: Option<u64>,
        cost_usd: Option<f64>,
    },
    /// A tool call is about to be executed.
    ToolCallStart {
        tool: String,
        arguments: Option<String>,
    },
    /// A tool call has completed with a success/failure outcome.
    ToolCall {
        tool: String,
        duration: Duration,
        success: bool,
    },
    /// The agent produced a final answer for the current user message.
    TurnComplete,
    /// A message was sent or received through a channel.
    ChannelMessage {
        /// Channel name (e.g., `"telegram"`, `"discord"`).
        channel: String,
        /// `"inbound"` or `"outbound"`.
        direction: String,
    },
    /// Periodic heartbeat tick from the runtime keep-alive loop.
    HeartbeatTick,
    /// Response cache hit — an LLM call was avoided.
    CacheHit {
        /// `"hot"` (in-memory) or `"warm"` (SQLite).
        cache_type: String,
        /// Estimated tokens saved by this cache hit.
        tokens_saved: u64,
    },
    /// Response cache miss — the prompt was not found in cache.
    CacheMiss {
        /// `"response"` cache layer that was checked.
        cache_type: String,
    },
    /// An error occurred in a named component.
    Error {
        /// Subsystem where the error originated (e.g., `"provider"`, `"gateway"`).
        component: String,
        /// Human-readable error description. Must not contain secrets or tokens.
        message: String,
    },
    /// A hand has started execution.
    HandStarted { hand_name: String },
    /// A hand has completed execution successfully.
    HandCompleted {
        hand_name: String,
        duration_ms: u64,
        findings_count: usize,
    },
    /// A hand has failed during execution.
    HandFailed {
        hand_name: String,
        error: String,
        duration_ms: u64,
    },
    /// A deployment has started.
    DeploymentStarted {
        /// Identifier for the deployment (e.g., commit SHA or release tag).
        deploy_id: String,
    },
    /// A deployment has completed successfully.
    DeploymentCompleted {
        deploy_id: String,
        /// Commit SHA that was deployed.
        commit_sha: String,
    },
    /// A deployment has failed.
    DeploymentFailed {
        deploy_id: String,
        /// Human-readable failure reason.
        reason: String,
    },
    /// Recovery from a failed deployment has completed.
    RecoveryCompleted { deploy_id: String },
}

/// Numeric metrics emitted by the agent runtime.
///
/// Observers can aggregate these into dashboards, alerts, or structured logs.
/// Each variant carries a single scalar value with implicit units.
#[derive(Debug, Clone)]
pub enum ObserverMetric {
    /// Time elapsed for a single LLM or tool request.
    RequestLatency(Duration),
    /// Number of tokens consumed by an LLM call.
    TokensUsed(u64),
    /// Current number of active concurrent sessions.
    ActiveSessions(u64),
    /// Current depth of the inbound message queue.
    QueueDepth(u64),
    /// Duration of a single hand run.
    HandRunDuration {
        hand_name: String,
        duration: Duration,
    },
    /// Number of findings produced by a hand run.
    HandFindingsCount { hand_name: String, count: u64 },
    /// Records a hand run outcome for success-rate tracking.
    HandSuccessRate { hand_name: String, success: bool },
    /// Time elapsed from commit to deployment (lead time for changes).
    DeploymentLeadTime(Duration),
    /// Time elapsed to recover from a failed deployment.
    RecoveryTime(Duration),
}

/// What triggered an agent activation.
///
/// Recorded as the root span's `trigger` attribute. Kept coarse on purpose — the
/// concrete channel name (e.g. `"telegram"`, `"discord"`) is carried separately as a
/// `channel` attribute rather than expanding this enum per channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// Web chat over the WebSocket gateway (`/ws/chat`).
    WebChat,
    /// Inbound webhook (`/webhook`).
    Webhook,
    /// A native channel listener (Telegram / SMS / Discord / …). See the `channel` attr.
    Channel,
    /// Autonomous self-scheduled (cron) activation.
    SelfSchedule,
    /// Local CLI invocation.
    Cli,
}

impl Trigger {
    /// Stable lowercase string used as the span `trigger` attribute value.
    pub fn as_str(&self) -> &'static str {
        match self {
            Trigger::WebChat => "web_chat",
            Trigger::Webhook => "webhook",
            Trigger::Channel => "channel",
            Trigger::SelfSchedule => "self_schedule",
            Trigger::Cli => "cli",
        }
    }
}

/// A span attribute value. Non-generic on purpose so [`Span`] stays object-safe.
#[derive(Debug, Clone)]
pub enum AttrValue {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    /// A homogeneous array of strings, mapped to a native OTLP `ArrayValue`.
    ///
    /// Needed for Laminar association properties whose backing ClickHouse column
    /// is `Array(String)` (e.g. `lmnr.association.properties.tags` →
    /// `tags_array`): a JSON-encoded string would land as a single literal
    /// element, not a parsed array. The scalar variants above cannot express it.
    Array(Vec<String>),
}

/// A span in the activation trace tree.
///
/// The **root** span (returned by [`Observer::start_activation`]) owns the trace; every
/// LLM / tool / hand span for the activation is opened — directly or transitively — as a
/// descendant via [`child`](Span::child). A span opens when created and ends when the
/// handle is dropped, giving true start→end timing.
///
/// The trait is **recursive**: any span can open children, so a nested sub-agent's spans
/// parent beneath the parent's `delegate` span to arbitrary depth.
///
/// Implementations must be `Send + Sync` so a `&dyn Span` can be threaded across `.await`
/// points and shared between concurrently-executing tool calls.
pub trait Span: Send + Sync {
    /// Open a child span beneath this one. The child ends when its handle is dropped.
    fn child(&self, name: &str) -> Box<dyn Span>;

    /// Set (or overwrite) an attribute on this span.
    fn set_attr(&self, key: &str, value: AttrValue);

    /// Mark the span's terminal status (`true` = ok, `false` = error).
    fn set_status(&self, ok: bool);

    /// Record a span event (an OTel span event) with point-in-time attributes.
    ///
    /// Unlike [`set_attr`](Span::set_attr) (one current value per key), an event is a
    /// timestamped record — the reliability layer emits one per retry attempt, provider
    /// fallback, and exhausted-exception so they survive in the trace instead of
    /// collapsing to the final attempt. Defaults to a no-op so non-tracing observers
    /// (log, prometheus, noop) and [`NoopSpan`] stay zero-behavior with no code change.
    fn add_event(&self, _name: &str, _attrs: &[(&str, AttrValue)]) {}
}

/// A [`Span`] that records nothing.
///
/// Returned by the default [`Observer::start_activation`], so observers that do not
/// implement tracing (log, prometheus, noop, …) stay zero-behavior with no code change.
pub struct NoopSpan;

impl Span for NoopSpan {
    fn child(&self, _name: &str) -> Box<dyn Span> {
        Box::new(NoopSpan)
    }
    fn set_attr(&self, _key: &str, _value: AttrValue) {}
    fn set_status(&self, _ok: bool) {}
}

/// Core observability trait for recording agent runtime telemetry.
///
/// Implement this trait to integrate with any monitoring backend (structured
/// logging, Prometheus, OpenTelemetry, etc.). The agent runtime holds one or
/// more `Observer` instances and calls [`record_event`](Observer::record_event)
/// and [`record_metric`](Observer::record_metric) at key lifecycle points.
///
/// Implementations must be `Send + Sync + 'static` because the observer is
/// shared across async tasks via `Arc`.
pub trait Observer: Send + Sync + 'static {
    /// Record a discrete lifecycle event.
    ///
    /// Called synchronously on the hot path; implementations should avoid
    /// blocking I/O. Buffer events internally and flush asynchronously
    /// when possible.
    fn record_event(&self, event: &ObserverEvent);

    /// Record a numeric metric sample.
    ///
    /// Called synchronously; same non-blocking guidance as
    /// [`record_event`](Observer::record_event).
    fn record_metric(&self, metric: &ObserverMetric);

    /// Flush any buffered telemetry data to the backend.
    ///
    /// The runtime calls this during graceful shutdown. The default
    /// implementation is a no-op, which is appropriate for backends
    /// that write synchronously.
    fn flush(&self) {}

    /// Return the human-readable name of this observer backend.
    ///
    /// Used in logs and diagnostics (e.g., `"console"`, `"prometheus"`,
    /// `"opentelemetry"`).
    fn name(&self) -> &str;

    /// Downcast to `Any` for backend-specific operations.
    ///
    /// Enables callers to access concrete observer types when needed
    /// (e.g., retrieving a Prometheus registry handle for custom metrics).
    fn as_any(&self) -> &dyn std::any::Any;

    /// Begin a new agent activation and return its **root span**.
    ///
    /// One activation = one trace. The returned span owns the trace; all LLM / tool / hand
    /// spans for the activation are opened as its descendants. The span ends when the
    /// returned handle is dropped — which the runtime arranges to coincide with
    /// terminal-reply dispatch, **not** inbound ACK.
    ///
    /// `thread_id` groups activations into a conversation; it is never the trace identity.
    /// Autonomous ([`Trigger::SelfSchedule`]) activations pass a synthetic group key.
    ///
    /// The default returns a [`NoopSpan`], so non-tracing observers are unaffected; only
    /// backends that emit traces (OpenTelemetry) override this.
    fn start_activation(&self, _trigger: Trigger, _thread_id: Option<&str>) -> Box<dyn Span> {
        Box::new(NoopSpan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use std::time::Duration;

    #[derive(Default)]
    struct DummyObserver {
        events: Mutex<u64>,
        metrics: Mutex<u64>,
    }

    impl Observer for DummyObserver {
        fn record_event(&self, _event: &ObserverEvent) {
            let mut guard = self.events.lock();
            *guard += 1;
        }

        fn record_metric(&self, _metric: &ObserverMetric) {
            let mut guard = self.metrics.lock();
            *guard += 1;
        }

        fn name(&self) -> &str {
            "dummy-observer"
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[test]
    fn observer_records_events_and_metrics() {
        let observer = DummyObserver::default();

        observer.record_event(&ObserverEvent::HeartbeatTick);
        observer.record_event(&ObserverEvent::Error {
            component: "test".into(),
            message: "boom".into(),
        });
        observer.record_metric(&ObserverMetric::TokensUsed(42));

        assert_eq!(*observer.events.lock(), 2);
        assert_eq!(*observer.metrics.lock(), 1);
    }

    #[test]
    fn observer_default_flush_and_as_any_work() {
        let observer = DummyObserver::default();

        observer.flush();
        assert_eq!(observer.name(), "dummy-observer");
        assert!(observer.as_any().downcast_ref::<DummyObserver>().is_some());
    }

    #[test]
    fn observer_event_and_metric_are_cloneable() {
        let event = ObserverEvent::ToolCall {
            tool: "shell".into(),
            duration: Duration::from_millis(10),
            success: true,
        };
        let metric = ObserverMetric::RequestLatency(Duration::from_millis(8));

        let cloned_event = event.clone();
        let cloned_metric = metric.clone();

        assert!(matches!(cloned_event, ObserverEvent::ToolCall { .. }));
        assert!(matches!(cloned_metric, ObserverMetric::RequestLatency(_)));
    }

    #[test]
    fn noop_span_add_event_is_silent() {
        // The default trait method is a no-op; NoopSpan inherits it (no override)
        // so non-tracing observers stay zero-behavior. Must not panic.
        let span = NoopSpan;
        span.add_event(
            "retry-attempt",
            &[
                ("attempt", AttrValue::Int(1)),
                ("retryable", AttrValue::Bool(true)),
            ],
        );
    }

    #[test]
    fn span_add_event_can_be_overridden_and_records() {
        // A recording Span proves the trait method is dispatchable and overridable
        // through `&dyn Span` — mirrors how OtelSpan forwards events to its OTel span.
        #[derive(Default)]
        struct RecordingSpan {
            events: Mutex<Vec<String>>,
        }
        impl Span for RecordingSpan {
            fn child(&self, _name: &str) -> Box<dyn Span> {
                Box::new(NoopSpan)
            }
            fn set_attr(&self, _key: &str, _value: AttrValue) {}
            fn set_status(&self, _ok: bool) {}
            fn add_event(&self, name: &str, attrs: &[(&str, AttrValue)]) {
                self.events.lock().push(format!("{name}:{}", attrs.len()));
            }
        }

        let recording = RecordingSpan::default();
        let span: &dyn Span = &recording;
        span.add_event(
            "fallback-fired",
            &[
                ("from", AttrValue::Str("openrouter/gemini-3.1-pro".into())),
                ("to", AttrValue::Str("openrouter/qwen3-max".into())),
            ],
        );
        span.add_event("exception", &[("exception.escaped", AttrValue::Bool(true))]);
        assert_eq!(
            recording.events.lock().as_slice(),
            ["fallback-fired:2", "exception:1"]
        );
    }

    #[test]
    fn hand_events_recordable() {
        let observer = DummyObserver::default();

        observer.record_event(&ObserverEvent::HandStarted {
            hand_name: "review".into(),
        });
        observer.record_event(&ObserverEvent::HandCompleted {
            hand_name: "review".into(),
            duration_ms: 1500,
            findings_count: 3,
        });
        observer.record_event(&ObserverEvent::HandFailed {
            hand_name: "review".into(),
            error: "timeout".into(),
            duration_ms: 5000,
        });

        assert_eq!(*observer.events.lock(), 3);
    }

    #[test]
    fn hand_metrics_recordable() {
        let observer = DummyObserver::default();

        observer.record_metric(&ObserverMetric::HandRunDuration {
            hand_name: "review".into(),
            duration: Duration::from_millis(1500),
        });
        observer.record_metric(&ObserverMetric::HandFindingsCount {
            hand_name: "review".into(),
            count: 3,
        });
        observer.record_metric(&ObserverMetric::HandSuccessRate {
            hand_name: "review".into(),
            success: true,
        });

        assert_eq!(*observer.metrics.lock(), 3);
    }

    #[test]
    fn hand_event_and_metric_are_cloneable() {
        let event = ObserverEvent::HandCompleted {
            hand_name: "review".into(),
            duration_ms: 500,
            findings_count: 2,
        };
        let metric = ObserverMetric::HandRunDuration {
            hand_name: "review".into(),
            duration: Duration::from_millis(500),
        };

        let cloned_event = event.clone();
        let cloned_metric = metric.clone();

        assert!(matches!(cloned_event, ObserverEvent::HandCompleted { .. }));
        assert!(matches!(
            cloned_metric,
            ObserverMetric::HandRunDuration { .. }
        ));
    }
}
