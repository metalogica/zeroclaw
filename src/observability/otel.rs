use super::traits::{
    AttrValue, Observer, ObserverEvent, ObserverMetric, Span as TraceSpan, Trigger,
};
use opentelemetry::metrics::{Counter, Gauge, Histogram};
use opentelemetry::trace::{Status, TraceContextExt, Tracer};
use opentelemetry::{Array, Context, KeyValue, Value, global};
use opentelemetry_otlp::{WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

/// Process-wide OTLP providers, built once and shared by every component's
/// `OtelObserver`. Multiple subsystems (daemon, gateway, channels, scheduler,
/// heartbeat, …) each construct an observer, but they all run in one process
/// with one `[observability]` config — so they share a single TracerProvider +
/// OTLP exporter rather than spinning up N exporters and re-installing the
/// OpenTelemetry globals N times. First caller wins (installs globals + logs);
/// components initialize sequentially at startup, so the slot is uncontended.
static OTEL_PROVIDERS: OnceLock<(SdkTracerProvider, SdkMeterProvider)> = OnceLock::new();

/// OpenTelemetry-backed observer — exports traces and metrics via OTLP.
pub struct OtelObserver {
    tracer_provider: SdkTracerProvider,
    meter_provider: SdkMeterProvider,

    /// Configured `deployment.environment` (non-empty), surfaced on the root
    /// activation span. Self-hosted Laminar discards OTLP *resource* attributes,
    /// so the resource-level copy is invisible there — the span attribute is the
    /// queryable one. `None` when unconfigured/empty.
    environment: Option<String>,

    // Metrics instruments
    agent_starts: Counter<u64>,
    agent_duration: Histogram<f64>,
    llm_calls: Counter<u64>,
    llm_duration: Histogram<f64>,
    tool_calls: Counter<u64>,
    tool_duration: Histogram<f64>,
    channel_messages: Counter<u64>,
    heartbeat_ticks: Counter<u64>,
    errors: Counter<u64>,
    request_latency: Histogram<f64>,
    tokens_used: Counter<u64>,
    active_sessions: Gauge<u64>,
    queue_depth: Gauge<u64>,
    hand_runs: Counter<u64>,
    hand_duration: Histogram<f64>,
    hand_findings: Counter<u64>,
}

/// Parse an `OTEL_EXPORTER_OTLP_HEADERS`-style string into a header map.
///
/// Format: comma-separated `key=value` pairs (`k1=v1,k2=v2`). Keys and values
/// are trimmed. Pairs missing a `=` or with an empty key are skipped so a
/// malformed entry never blocks observer initialization. Header values may
/// contain `=` (e.g. base64) — only the first `=` is treated as the separator.
fn parse_otlp_headers(raw: &str) -> HashMap<String, String> {
    raw.split(',')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            let key = key.trim();
            if key.is_empty() {
                return None;
            }
            Some((key.to_string(), value.trim().to_string()))
        })
        .collect()
}

/// Build the OTLP `Resource` shared by the trace and metric providers.
///
/// Carries `service.name` always and `deployment.environment` when configured.
/// Kept deliberately minimal — resource attributes are process-global and must
/// never carry credentials or PII (`user.id` and request content ride on spans,
/// not the resource).
fn build_otlp_resource(
    service_name: &str,
    environment: Option<&str>,
) -> opentelemetry_sdk::Resource {
    let mut builder =
        opentelemetry_sdk::Resource::builder().with_service_name(service_name.to_string());
    if let Some(env) = environment.filter(|e| !e.is_empty()) {
        builder = builder.with_attribute(KeyValue::new("deployment.environment", env.to_string()));
    }
    builder.build()
}

/// Terminally shut down the process-wide OTLP providers, if they were built.
///
/// Ends and drains the batch span/metric processors and **blocks** until export
/// completes (or the SDK's internal timeout). Unlike [`OtelObserver::flush`],
/// which `force_flush`es but only ships spans that have already ended, this is
/// the right call on process exit: a short-lived one-shot (`zeroclaw agent -m …`)
/// would otherwise terminate before the batch processor's periodic tick, and a
/// bare `force_flush` does not reliably drain on a tearing-down runtime.
///
/// No-op when no OTel backend was ever initialized (the shared slot is empty).
/// **Terminal** — the providers are unusable afterward, so only call when the
/// process is actually exiting (never from the daemon/gateway/cron mid-life).
pub fn shutdown_shared_providers() {
    if let Some((tracer_provider, meter_provider)) = OTEL_PROVIDERS.get() {
        if let Err(e) = tracer_provider.shutdown() {
            tracing::warn!("OTel trace shutdown failed: {e}");
        }
        if let Err(e) = meter_provider.shutdown() {
            tracing::warn!("OTel metric shutdown failed: {e}");
        }
    }
}

impl OtelObserver {
    /// Create a new OTel observer exporting to the given OTLP endpoint.
    ///
    /// Uses HTTP/protobuf transport (port 4318 by default).
    /// Falls back to `http://localhost:4318` if no endpoint is provided.
    ///
    /// `headers` is the OTel-standard `OTEL_EXPORTER_OTLP_HEADERS` string
    /// (`key=value,key=value`), applied to both the trace and metric exporters.
    /// Pass `None` to export without headers. Used for collectors that gate
    /// ingest on an auth header (e.g. `authorization=Bearer <token>`).
    ///
    /// `environment` is reported as the `deployment.environment` resource
    /// attribute (e.g. "dev", "prod"); `None` omits it (unchanged behavior).
    pub fn new(
        endpoint: Option<&str>,
        service_name: Option<&str>,
        headers: Option<&str>,
        environment: Option<&str>,
    ) -> Result<Self, String> {
        let base_endpoint = endpoint.unwrap_or("http://localhost:4318");
        let traces_endpoint = format!("{}/v1/traces", base_endpoint.trim_end_matches('/'));
        let metrics_endpoint = format!("{}/v1/metrics", base_endpoint.trim_end_matches('/'));
        let service_name = service_name.unwrap_or("zeroclaw");
        let header_map = headers.map(parse_otlp_headers).unwrap_or_default();

        // ── Trace + metric providers (built once per process) ───
        // Reuse the shared providers if another component already initialized
        // them; otherwise build the OTLP exporters, install the globals, and
        // publish to the shared slot. Instruments below always bind to the
        // global meter, so every observer records into the same pipeline.
        let (tracer_provider, meter_provider_clone) = match OTEL_PROVIDERS.get() {
            Some(providers) => providers.clone(),
            None => {
                // ── Trace exporter ──────────────────────────────
                let mut span_builder = opentelemetry_otlp::SpanExporter::builder()
                    .with_http()
                    .with_endpoint(&traces_endpoint);
                if !header_map.is_empty() {
                    span_builder = span_builder.with_headers(header_map.clone());
                }
                let span_exporter = span_builder
                    .build()
                    .map_err(|e| format!("Failed to create OTLP span exporter: {e}"))?;

                let tracer_provider = SdkTracerProvider::builder()
                    .with_batch_exporter(span_exporter)
                    .with_resource(build_otlp_resource(service_name, environment))
                    .build();

                // ── Metric exporter ─────────────────────────────
                let mut metric_builder = opentelemetry_otlp::MetricExporter::builder()
                    .with_http()
                    .with_endpoint(&metrics_endpoint);
                if !header_map.is_empty() {
                    metric_builder = metric_builder.with_headers(header_map.clone());
                }
                let metric_exporter = metric_builder
                    .build()
                    .map_err(|e| format!("Failed to create OTLP metric exporter: {e}"))?;

                let metric_reader =
                    opentelemetry_sdk::metrics::PeriodicReader::builder(metric_exporter).build();

                let meter_provider = opentelemetry_sdk::metrics::SdkMeterProvider::builder()
                    .with_reader(metric_reader)
                    .with_resource(build_otlp_resource(service_name, environment))
                    .build();

                global::set_tracer_provider(tracer_provider.clone());
                global::set_meter_provider(meter_provider.clone());
                tracing::info!(
                    endpoint = %base_endpoint,
                    service_name,
                    environment = environment.unwrap_or("<unset>"),
                    "OpenTelemetry OTLP exporter initialized"
                );

                let pair = (tracer_provider, meter_provider);
                // Publish for later components; if a concurrent caller won the
                // race, keep the winner's providers for consistency.
                let _ = OTEL_PROVIDERS.set(pair.clone());
                OTEL_PROVIDERS.get().cloned().unwrap_or(pair)
            }
        };

        // ── Create metric instruments ────────────────────────────
        let meter = global::meter("zeroclaw");

        let agent_starts = meter
            .u64_counter("zeroclaw.agent.starts")
            .with_description("Total agent invocations")
            .build();

        let agent_duration = meter
            .f64_histogram("zeroclaw.agent.duration")
            .with_description("Agent invocation duration in seconds")
            .with_unit("s")
            .build();

        let llm_calls = meter
            .u64_counter("zeroclaw.llm.calls")
            .with_description("Total LLM provider calls")
            .build();

        let llm_duration = meter
            .f64_histogram("zeroclaw.llm.duration")
            .with_description("LLM provider call duration in seconds")
            .with_unit("s")
            .build();

        let tool_calls = meter
            .u64_counter("zeroclaw.tool.calls")
            .with_description("Total tool calls")
            .build();

        let tool_duration = meter
            .f64_histogram("zeroclaw.tool.duration")
            .with_description("Tool execution duration in seconds")
            .with_unit("s")
            .build();

        let channel_messages = meter
            .u64_counter("zeroclaw.channel.messages")
            .with_description("Total channel messages")
            .build();

        let heartbeat_ticks = meter
            .u64_counter("zeroclaw.heartbeat.ticks")
            .with_description("Total heartbeat ticks")
            .build();

        let errors = meter
            .u64_counter("zeroclaw.errors")
            .with_description("Total errors by component")
            .build();

        let request_latency = meter
            .f64_histogram("zeroclaw.request.latency")
            .with_description("Request latency in seconds")
            .with_unit("s")
            .build();

        let tokens_used = meter
            .u64_counter("zeroclaw.tokens.used")
            .with_description("Total tokens consumed (monotonic)")
            .build();

        let active_sessions = meter
            .u64_gauge("zeroclaw.sessions.active")
            .with_description("Current number of active sessions")
            .build();

        let queue_depth = meter
            .u64_gauge("zeroclaw.queue.depth")
            .with_description("Current message queue depth")
            .build();

        let hand_runs = meter
            .u64_counter("zeroclaw.hand.runs")
            .with_description("Total hand runs")
            .build();

        let hand_duration = meter
            .f64_histogram("zeroclaw.hand.duration")
            .with_description("Hand run duration in seconds")
            .with_unit("s")
            .build();

        let hand_findings = meter
            .u64_counter("zeroclaw.hand.findings")
            .with_description("Total findings produced by hand runs")
            .build();

        Ok(Self {
            tracer_provider,
            meter_provider: meter_provider_clone,
            environment: environment.filter(|e| !e.is_empty()).map(str::to_string),
            agent_starts,
            agent_duration,
            llm_calls,
            llm_duration,
            tool_calls,
            tool_duration,
            channel_messages,
            heartbeat_ticks,
            errors,
            request_latency,
            tokens_used,
            active_sessions,
            queue_depth,
            hand_runs,
            hand_duration,
            hand_findings,
        })
    }
}

impl Observer for OtelObserver {
    fn record_event(&self, event: &ObserverEvent) {
        // Spans are built by `OtelSpan` (see `start_activation`); `record_event`
        // is metrics-only so events and the connected trace stay decoupled.
        match event {
            ObserverEvent::AgentStart { provider, model } => {
                self.agent_starts.add(
                    1,
                    &[
                        KeyValue::new("provider", provider.clone()),
                        KeyValue::new("model", model.clone()),
                    ],
                );
            }
            ObserverEvent::LlmRequest { .. }
            | ObserverEvent::ToolCallStart { .. }
            | ObserverEvent::TurnComplete
            | ObserverEvent::CacheHit { .. }
            | ObserverEvent::CacheMiss { .. } => {}
            ObserverEvent::LlmResponse {
                provider,
                model,
                duration,
                success,
                error_message: _,
                input_tokens: _,
                output_tokens: _,
            } => {
                let secs = duration.as_secs_f64();
                let attrs = [
                    KeyValue::new("provider", provider.clone()),
                    KeyValue::new("model", model.clone()),
                    KeyValue::new("success", success.to_string()),
                ];
                self.llm_calls.add(1, &attrs);
                self.llm_duration.record(secs, &attrs);
            }
            ObserverEvent::AgentEnd {
                provider,
                model,
                duration,
                tokens_used: _,
                cost_usd: _,
            } => {
                self.agent_duration.record(
                    duration.as_secs_f64(),
                    &[
                        KeyValue::new("provider", provider.clone()),
                        KeyValue::new("model", model.clone()),
                    ],
                );
                // Note: tokens are recorded via record_metric(TokensUsed) to avoid
                // double-counting. AgentEnd only records duration.
            }
            ObserverEvent::ToolCall {
                tool,
                duration,
                success,
            } => {
                let attrs = [
                    KeyValue::new("tool", tool.clone()),
                    KeyValue::new("success", success.to_string()),
                ];
                self.tool_calls.add(1, &attrs);
                self.tool_duration.record(
                    duration.as_secs_f64(),
                    &[KeyValue::new("tool", tool.clone())],
                );
            }
            ObserverEvent::ChannelMessage { channel, direction } => {
                self.channel_messages.add(
                    1,
                    &[
                        KeyValue::new("channel", channel.clone()),
                        KeyValue::new("direction", direction.clone()),
                    ],
                );
            }
            ObserverEvent::HeartbeatTick => {
                self.heartbeat_ticks.add(1, &[]);
            }
            ObserverEvent::Error {
                component,
                message: _,
            } => {
                self.errors
                    .add(1, &[KeyValue::new("component", component.clone())]);
            }
            ObserverEvent::HandStarted { .. } => {}
            ObserverEvent::HandCompleted {
                hand_name,
                duration_ms,
                findings_count,
            } => {
                let secs = *duration_ms as f64 / 1000.0;
                let attrs = [
                    KeyValue::new("hand", hand_name.clone()),
                    KeyValue::new("success", "true"),
                ];
                self.hand_runs.add(1, &attrs);
                self.hand_duration
                    .record(secs, &[KeyValue::new("hand", hand_name.clone())]);
                self.hand_findings.add(
                    *findings_count as u64,
                    &[KeyValue::new("hand", hand_name.clone())],
                );
            }
            ObserverEvent::HandFailed {
                hand_name,
                error: _,
                duration_ms,
            } => {
                let secs = *duration_ms as f64 / 1000.0;
                let attrs = [
                    KeyValue::new("hand", hand_name.clone()),
                    KeyValue::new("success", "false"),
                ];
                self.hand_runs.add(1, &attrs);
                self.hand_duration
                    .record(secs, &[KeyValue::new("hand", hand_name.clone())]);
            }
            ObserverEvent::DeploymentStarted { .. }
            | ObserverEvent::DeploymentCompleted { .. }
            | ObserverEvent::DeploymentFailed { .. }
            | ObserverEvent::RecoveryCompleted { .. } => {
                // DORA deployment events: OTel pass-through not yet implemented.
            }
        }
    }

    fn record_metric(&self, metric: &ObserverMetric) {
        match metric {
            ObserverMetric::RequestLatency(d) => {
                self.request_latency.record(d.as_secs_f64(), &[]);
            }
            ObserverMetric::TokensUsed(t) => {
                self.tokens_used.add(*t as u64, &[]);
            }
            ObserverMetric::ActiveSessions(s) => {
                self.active_sessions.record(*s as u64, &[]);
            }
            ObserverMetric::QueueDepth(d) => {
                self.queue_depth.record(*d as u64, &[]);
            }
            ObserverMetric::HandRunDuration {
                hand_name,
                duration,
            } => {
                self.hand_duration.record(
                    duration.as_secs_f64(),
                    &[KeyValue::new("hand", hand_name.clone())],
                );
            }
            ObserverMetric::HandFindingsCount { hand_name, count } => {
                self.hand_findings
                    .add(*count, &[KeyValue::new("hand", hand_name.clone())]);
            }
            ObserverMetric::HandSuccessRate { hand_name, success } => {
                let success_str = if *success { "true" } else { "false" };
                self.hand_runs.add(
                    1,
                    &[
                        KeyValue::new("hand", hand_name.clone()),
                        KeyValue::new("success", success_str),
                    ],
                );
            }
            ObserverMetric::DeploymentLeadTime(_) | ObserverMetric::RecoveryTime(_) => {
                // DORA metrics: OTel pass-through not yet implemented.
            }
        }
    }

    fn flush(&self) {
        if let Err(e) = self.tracer_provider.force_flush() {
            tracing::warn!("OTel trace flush failed: {e}");
        }
        if let Err(e) = self.meter_provider.force_flush() {
            tracing::warn!("OTel metric flush failed: {e}");
        }
    }

    fn name(&self) -> &str {
        "otel"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn start_activation(&self, trigger: Trigger, thread_id: Option<&str>) -> Box<dyn TraceSpan> {
        Box::new(OtelSpan::root(
            trigger,
            thread_id,
            self.environment.as_deref(),
        ))
    }
}

/// Convert a backend-neutral [`AttrValue`] into an OTel attribute value.
fn attr_to_value(v: AttrValue) -> Value {
    match v {
        AttrValue::Str(s) => Value::String(s.into()),
        AttrValue::Int(i) => Value::I64(i),
        AttrValue::Float(f) => Value::F64(f),
        AttrValue::Bool(b) => Value::Bool(b),
        AttrValue::Array(items) => {
            Value::Array(Array::String(items.into_iter().map(Into::into).collect()))
        }
    }
}

/// An OTel-backed [`Span`](TraceSpan) node in the activation trace.
///
/// Holds an OTel [`Context`] with this node's span active. The root is created with an
/// empty parent context (a fresh `trace_id`); children are created with the parent's
/// context so they inherit its `trace_id` and link as descendants. The span opens on
/// construction and ends on `Drop`, giving true start→end timing.
pub struct OtelSpan {
    cx: Context,
    /// Configured `deployment.environment`, propagated to *every* span in the
    /// trace (not just the root) so each span is independently sliceable by env
    /// in backends that do not inherit the root span's attribute. `Arc` so child
    /// spans share the value without re-allocating. `None` when unconfigured.
    environment: Option<Arc<str>>,
}

impl OtelSpan {
    /// Open the root activation span (`agent.activation`) on a fresh trace.
    ///
    /// `environment` (when non-empty) is set as the `deployment.environment` span
    /// attribute in addition to the OTLP resource attribute — self-hosted Laminar
    /// drops resource attributes, so the span copy is the one that stays queryable.
    fn root(trigger: Trigger, thread_id: Option<&str>, environment: Option<&str>) -> Self {
        let tracer = global::tracer("zeroclaw");
        // Empty parent context ⇒ a brand-new trace_id (one trace per activation).
        let span = tracer.start_with_context("agent.activation", &Context::new());
        let this = Self {
            cx: Context::new().with_span(span),
            environment: environment.filter(|e| !e.is_empty()).map(Arc::from),
        };
        this.set_attr("trigger", AttrValue::Str(trigger.as_str().to_string()));
        if let Some(tid) = thread_id {
            this.set_attr("thread_id", AttrValue::Str(tid.to_string()));
            // Laminar's typed `session_id` column is filled only from the
            // `lmnr.association.properties.session_id` key — the plain
            // `thread_id` attr above never populates it. A conversation/thread
            // id IS Laminar's "session", so the twin carries the SAME value
            // under the SAME guard: absent when no thread id (never synthesized,
            // never ""). One edit covers every root (CLI/channel/webhook/WS).
            // Mirrors the `user.id` → `lmnr.association.properties.user_id`
            // dual-emit precedent (see identity::set_user_id_attrs).
            this.set_attr(
                "lmnr.association.properties.session_id",
                AttrValue::Str(tid.to_string()),
            );
        }
        if let Some(env) = this.environment.as_deref() {
            this.set_attr("deployment.environment", AttrValue::Str(env.to_string()));
        }
        this
    }

    /// Concrete child constructor (used by [`TraceSpan::child`] and by tests).
    ///
    /// Re-stamps `deployment.environment` on the child span so it is queryable on
    /// its own — backends that drop resource attributes and do not inherit the
    /// root span's attributes (e.g. self-hosted Laminar) can still slice child
    /// spans by environment.
    fn child_span(&self, name: &str) -> Self {
        let tracer = global::tracer("zeroclaw");
        let span = tracer.start_with_context(name.to_string(), &self.cx);
        let child = Self {
            cx: self.cx.with_span(span),
            environment: self.environment.clone(),
        };
        if let Some(env) = child.environment.as_deref() {
            child.set_attr("deployment.environment", AttrValue::Str(env.to_string()));
        }
        child
    }
}

impl TraceSpan for OtelSpan {
    fn child(&self, name: &str) -> Box<dyn TraceSpan> {
        Box::new(self.child_span(name))
    }

    fn set_attr(&self, key: &str, value: AttrValue) {
        self.cx
            .span()
            .set_attribute(KeyValue::new(key.to_string(), attr_to_value(value)));
    }

    fn set_status(&self, ok: bool) {
        self.cx
            .span()
            .set_status(if ok { Status::Ok } else { Status::error("") });
    }

    fn add_event(&self, name: &str, attrs: &[(&str, AttrValue)]) {
        let kvs: Vec<KeyValue> = attrs
            .iter()
            .map(|(k, v)| KeyValue::new(k.to_string(), attr_to_value(v.clone())))
            .collect();
        self.cx.span().add_event(name.to_string(), kvs);
    }
}

impl Drop for OtelSpan {
    fn drop(&mut self) {
        self.cx.span().end();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::Key;
    use std::time::Duration;

    #[test]
    fn resource_includes_deployment_environment_when_set() {
        let resource = build_otlp_resource("zeroclaw-test", Some("prod"));
        assert_eq!(
            resource.get(&Key::from_static_str("deployment.environment")),
            Some(Value::from("prod")),
            "deployment.environment must be present when configured"
        );
        assert_eq!(
            resource.get(&Key::from_static_str("service.name")),
            Some(Value::from("zeroclaw-test")),
            "service.name must always be present"
        );
    }

    #[test]
    fn resource_omits_deployment_environment_when_unset_or_empty() {
        for env in [None, Some("")] {
            let resource = build_otlp_resource("zeroclaw-test", env);
            assert_eq!(
                resource.get(&Key::from_static_str("deployment.environment")),
                None,
                "deployment.environment must be omitted for {env:?} (resource stays minimal)"
            );
        }
    }

    // Note: OtelObserver::new() requires an OTLP endpoint.
    // In tests we verify the struct creation fails gracefully
    // when no collector is available, and test the observer interface
    // by constructing with a known-unreachable endpoint (spans/metrics
    // are buffered and exported asynchronously, so recording never panics).

    fn test_observer() -> OtelObserver {
        // Create with a dummy endpoint — exports will silently fail
        // but the observer itself works fine for recording
        OtelObserver::new(
            Some("http://127.0.0.1:19999"),
            Some("zeroclaw-test"),
            None,
            None,
        )
        .expect("observer creation should not fail with valid endpoint format")
    }

    #[test]
    fn otel_observer_name() {
        let obs = test_observer();
        assert_eq!(obs.name(), "otel");
    }

    #[test]
    fn records_all_events_without_panic() {
        let obs = test_observer();
        obs.record_event(&ObserverEvent::AgentStart {
            provider: "openrouter".into(),
            model: "claude-sonnet".into(),
        });
        obs.record_event(&ObserverEvent::LlmRequest {
            provider: "openrouter".into(),
            model: "claude-sonnet".into(),
            messages_count: 2,
        });
        obs.record_event(&ObserverEvent::LlmResponse {
            provider: "openrouter".into(),
            model: "claude-sonnet".into(),
            duration: Duration::from_millis(250),
            success: true,
            error_message: None,
            input_tokens: Some(100),
            output_tokens: Some(50),
        });
        obs.record_event(&ObserverEvent::AgentEnd {
            provider: "openrouter".into(),
            model: "claude-sonnet".into(),
            duration: Duration::from_millis(500),
            tokens_used: Some(100),
            cost_usd: Some(0.0015),
        });
        obs.record_event(&ObserverEvent::AgentEnd {
            provider: "openrouter".into(),
            model: "claude-sonnet".into(),
            duration: Duration::ZERO,
            tokens_used: None,
            cost_usd: None,
        });
        obs.record_event(&ObserverEvent::ToolCallStart {
            tool: "shell".into(),
            arguments: None,
        });
        obs.record_event(&ObserverEvent::ToolCall {
            tool: "shell".into(),
            duration: Duration::from_millis(10),
            success: true,
        });
        obs.record_event(&ObserverEvent::ToolCall {
            tool: "file_read".into(),
            duration: Duration::from_millis(5),
            success: false,
        });
        obs.record_event(&ObserverEvent::TurnComplete);
        obs.record_event(&ObserverEvent::ChannelMessage {
            channel: "telegram".into(),
            direction: "inbound".into(),
        });
        obs.record_event(&ObserverEvent::HeartbeatTick);
        obs.record_event(&ObserverEvent::Error {
            component: "provider".into(),
            message: "timeout".into(),
        });
    }

    #[test]
    fn records_all_metrics_without_panic() {
        let obs = test_observer();
        obs.record_metric(&ObserverMetric::RequestLatency(Duration::from_secs(2)));
        obs.record_metric(&ObserverMetric::TokensUsed(500));
        obs.record_metric(&ObserverMetric::TokensUsed(0));
        obs.record_metric(&ObserverMetric::ActiveSessions(3));
        obs.record_metric(&ObserverMetric::QueueDepth(42));
    }

    #[test]
    fn flush_does_not_panic() {
        let obs = test_observer();
        obs.record_event(&ObserverEvent::HeartbeatTick);
        obs.flush();
    }

    // ── §8.2 OTel export failure resilience tests ────────────

    #[test]
    fn otel_records_error_event_without_panic() {
        let obs = test_observer();
        // Simulate an error event — should not panic even with unreachable endpoint
        obs.record_event(&ObserverEvent::Error {
            component: "provider".into(),
            message: "connection refused to model endpoint".into(),
        });
    }

    #[test]
    fn otel_records_llm_failure_without_panic() {
        let obs = test_observer();
        obs.record_event(&ObserverEvent::LlmResponse {
            provider: "openrouter".into(),
            model: "missing-model".into(),
            duration: Duration::from_millis(0),
            success: false,
            error_message: Some("404 Not Found".into()),
            input_tokens: None,
            output_tokens: None,
        });
    }

    #[test]
    fn otel_flush_idempotent_with_unreachable_endpoint() {
        let obs = test_observer();
        // Multiple flushes should not panic even when endpoint is unreachable
        obs.flush();
        obs.flush();
        obs.flush();
    }

    #[test]
    fn otel_records_zero_duration_metrics() {
        let obs = test_observer();
        obs.record_metric(&ObserverMetric::RequestLatency(Duration::ZERO));
        obs.record_metric(&ObserverMetric::TokensUsed(0));
        obs.record_metric(&ObserverMetric::ActiveSessions(0));
        obs.record_metric(&ObserverMetric::QueueDepth(0));
    }

    #[test]
    fn otel_hand_events_do_not_panic() {
        let obs = test_observer();
        obs.record_event(&ObserverEvent::HandStarted {
            hand_name: "review".into(),
        });
        obs.record_event(&ObserverEvent::HandCompleted {
            hand_name: "review".into(),
            duration_ms: 1500,
            findings_count: 3,
        });
        obs.record_event(&ObserverEvent::HandFailed {
            hand_name: "review".into(),
            error: "timeout".into(),
            duration_ms: 5000,
        });
    }

    #[test]
    fn otel_hand_metrics_do_not_panic() {
        let obs = test_observer();
        obs.record_metric(&ObserverMetric::HandRunDuration {
            hand_name: "review".into(),
            duration: Duration::from_millis(1500),
        });
        obs.record_metric(&ObserverMetric::HandFindingsCount {
            hand_name: "review".into(),
            count: 5,
        });
        obs.record_metric(&ObserverMetric::HandSuccessRate {
            hand_name: "review".into(),
            success: true,
        });
    }

    #[test]
    fn parse_otlp_headers_handles_pairs_whitespace_and_malformed() {
        let map = parse_otlp_headers(" authorization=Bearer abc123 , x-tenant = acme ");
        assert_eq!(
            map.get("authorization").map(String::as_str),
            Some("Bearer abc123")
        );
        assert_eq!(map.get("x-tenant").map(String::as_str), Some("acme"));

        // A value containing '=' (e.g. base64) keeps everything after the first '='.
        let b64 = parse_otlp_headers("authorization=Basic dXNlcjpwYXNz==");
        assert_eq!(
            b64.get("authorization").map(String::as_str),
            Some("Basic dXNlcjpwYXNz==")
        );

        // Malformed entries (no '=', empty key) are skipped, not fatal.
        let partial = parse_otlp_headers("garbage,=novalue,good=1");
        assert_eq!(partial.len(), 1);
        assert_eq!(partial.get("good").map(String::as_str), Some("1"));

        assert!(parse_otlp_headers("").is_empty());
    }

    #[test]
    fn otel_observer_creation_with_headers_succeeds() {
        let result = OtelObserver::new(
            Some("http://127.0.0.1:12345"),
            Some("zeroclaw-test"),
            Some("authorization=Bearer test-token"),
            None,
        );
        assert!(
            result.is_ok(),
            "observer creation must succeed with auth headers set"
        );
    }

    #[test]
    fn otel_observer_creation_with_valid_endpoint_succeeds() {
        // Even though endpoint is unreachable, creation should succeed
        let result = OtelObserver::new(
            Some("http://127.0.0.1:12345"),
            Some("zeroclaw-test"),
            None,
            None,
        );
        assert!(
            result.is_ok(),
            "observer creation must succeed even with unreachable endpoint"
        );
    }

    // ── Connected-trace invariants (the point of this feature) ────────

    #[test]
    fn activation_children_share_trace_id() {
        // Constructing the observer sets the global tracer provider.
        let _obs = test_observer();
        let root = OtelSpan::root(Trigger::Cli, Some("thread-1"), None);
        let child = root.child_span("llm.call");
        let grandchild = child.child_span("tool.call");

        let root_tid = root.cx.span().span_context().trace_id();
        // Every descendant shares the activation's trace_id.
        assert_eq!(root_tid, child.cx.span().span_context().trace_id());
        assert_eq!(root_tid, grandchild.cx.span().span_context().trace_id());

        // …but each is a distinct span (real parent→child links).
        assert_ne!(
            root.cx.span().span_context().span_id(),
            child.cx.span().span_context().span_id()
        );
        assert_ne!(
            child.cx.span().span_context().span_id(),
            grandchild.cx.span().span_context().span_id()
        );
    }

    #[test]
    fn root_with_environment_sets_span_attr_without_panic() {
        // OTel spans don't expose their attributes for read-back, so we can only
        // assert the env branch executes cleanly (set_attr on the live span).
        let _obs = test_observer();
        let root = OtelSpan::root(Trigger::WebChat, Some("thread-1"), Some("dev"));
        // Empty env must be treated as unset (no attribute written, no panic).
        let _empty = OtelSpan::root(Trigger::WebChat, None, Some(""));
        root.set_status(true);
    }

    #[test]
    fn root_session_id_twin_follows_thread_id_guard_without_panic() {
        // OTel spans don't expose attributes for read-back, so as with the env
        // branch we can only assert both code paths execute cleanly: a root WITH
        // a thread id writes both `thread_id` and the `session_id` twin; a root
        // WITHOUT one writes neither (the `if let Some(tid)` guard is skipped).
        let _obs = test_observer();
        let with_tid = OtelSpan::root(Trigger::WebChat, Some("thread-1"), None);
        let without_tid = OtelSpan::root(Trigger::Cli, None, None);
        with_tid.set_status(true);
        without_tid.set_status(true);
    }

    #[test]
    fn attr_to_value_maps_array_to_native_otlp_string_array() {
        // The Array variant must become a native OTLP string array (not a JSON
        // string) so Laminar's `Array(String)` tags column ingests it.
        let v = attr_to_value(AttrValue::Array(vec!["web".into(), "web_chat".into()]));
        match v {
            Value::Array(Array::String(items)) => {
                let got: Vec<String> = items.iter().map(|s| s.as_str().to_string()).collect();
                assert_eq!(got, vec!["web".to_string(), "web_chat".to_string()]);
            }
            other => panic!("expected Value::Array(Array::String), got {other:?}"),
        }
    }

    #[test]
    fn separate_activations_get_separate_traces() {
        let _obs = test_observer();
        let a = OtelSpan::root(Trigger::WebChat, Some("thread-1"), None);
        let b = OtelSpan::root(Trigger::WebChat, Some("thread-1"), None);
        // Same thread_id, but two activations ⇒ two distinct traces (never merged).
        assert_ne!(
            a.cx.span().span_context().trace_id(),
            b.cx.span().span_context().trace_id()
        );
    }

    #[test]
    fn start_activation_through_trait_records_without_panic() {
        let obs = test_observer();
        let root = obs.start_activation(Trigger::Webhook, None);
        let child = root.child("llm.call");
        child.set_attr("gen_ai.system", AttrValue::Str("openrouter".into()));
        child.set_status(true);
        root.set_status(true);
        // Drop ends spans even with an unreachable exporter endpoint.
    }

    #[test]
    fn default_noop_span_is_inert() {
        // The default Observer::start_activation returns a NoopSpan that does nothing.
        let span = super::super::traits::NoopSpan;
        let child = TraceSpan::child(&span, "llm.call");
        child.set_attr("k", AttrValue::Int(1));
        child.set_status(false);
    }

    /// The configured environment must propagate to *every* span in the trace —
    /// root, child, and grandchild — not just the root. This is the per-span
    /// delta this bead adds: each `OtelSpan` carries the value so `child_span`
    /// can re-stamp `deployment.environment` on backends that don't inherit the
    /// root's attributes (self-hosted Laminar). Asserting on the threaded field
    /// keeps the test deterministic and dep-free; end-to-end attribute emission
    /// is covered by the live Laminar verification gate (the existing convention
    /// here is that span export is verified at runtime, not in unit tests).
    #[test]
    fn environment_propagates_to_every_span() {
        let root = OtelSpan::root(Trigger::Cli, Some("thread-1"), Some("prod"));
        let child = root.child_span("agent.step");
        let grandchild = child.child_span("llm.call");

        assert_eq!(root.environment.as_deref(), Some("prod"), "root carries env");
        assert_eq!(
            child.environment.as_deref(),
            Some("prod"),
            "child inherits env"
        );
        assert_eq!(
            grandchild.environment.as_deref(),
            Some("prod"),
            "grandchild inherits env transitively"
        );
    }

    /// An empty or absent environment must not propagate a value — the per-span
    /// stamping must not invent `deployment.environment` when unconfigured.
    #[test]
    fn environment_absent_does_not_propagate() {
        for env in [None, Some("")] {
            let root = OtelSpan::root(Trigger::Cli, None, env);
            let child = root.child_span("agent.step");
            assert!(
                root.environment.is_none(),
                "root must not carry env for {env:?}"
            );
            assert!(
                child.environment.is_none(),
                "child must not carry env for {env:?}"
            );
        }
    }
}
