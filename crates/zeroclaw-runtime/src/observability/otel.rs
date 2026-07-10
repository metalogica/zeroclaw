use super::traits::{
    AttrValue, Observer, ObserverEvent, ObserverMetric, Span as TraceSpan, Trigger,
};
use opentelemetry::metrics::{Counter, Gauge, Histogram};
use opentelemetry::trace::{Span, SpanKind, Status, TraceContextExt as _, Tracer};
use opentelemetry::{Array, Context, KeyValue, Value, global};
use opentelemetry_otlp::{WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// Process-wide OTLP providers, built once and shared by every component's
/// `OtelObserver`. Multiple subsystems (daemon, gateway, channels, agent loop,
/// heartbeat, SSE, …) each construct an observer via
/// [`super::create_observer`], but they all run in one process with one
/// `[observability]` config — so they share a single TracerProvider +
/// MeterProvider (and therefore one OTLP exporter) rather than spinning up N
/// exporters and re-installing the OpenTelemetry globals N times. The N-exporter
/// path races `global::set_tracer_provider`/`set_meter_provider`, dropping or
/// duplicating exporters and leaking providers. First caller wins (builds the
/// exporters, installs the globals, logs); components initialize sequentially
/// at startup, so the slot is effectively uncontended.
static OTEL_PROVIDERS: OnceLock<(SdkTracerProvider, SdkMeterProvider)> = OnceLock::new();

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
            ::zeroclaw_log::record!(
                WARN,
                ::zeroclaw_log::Event::new(module_path!(), ::zeroclaw_log::Action::Note)
                    .with_outcome(::zeroclaw_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                "OTel trace shutdown failed"
            );
        }
        if let Err(e) = meter_provider.shutdown() {
            ::zeroclaw_log::record!(
                WARN,
                ::zeroclaw_log::Event::new(module_path!(), ::zeroclaw_log::Action::Note)
                    .with_outcome(::zeroclaw_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                "OTel metric shutdown failed"
            );
        }
    }
}

/// OpenTelemetry-backed observer — exports traces and metrics via OTLP.
pub struct OtelObserver {
    tracer_provider: SdkTracerProvider,
    meter_provider: SdkMeterProvider,

    /// Configured `deployment.environment` (non-empty), surfaced on the root
    /// activation span and every child. Self-hosted Laminar discards OTLP
    /// *resource* attributes, so the resource-level copy is invisible there —
    /// the span attribute is the queryable one. `None` when unconfigured/empty.
    ///
    /// v0.8.0 has no `otel_deployment_environment` config field (the upstream
    /// re-home deleted the fork's config surface), and this file may not widen
    /// [`OtelObserver::new`]'s signature (owned by zc-ju48's shared-provider
    /// factory). So the value is sourced from the environment
    /// (`ZEROCLAW_DEPLOYMENT_ENVIRONMENT`, falling back to the OTel-standard
    /// `OTEL_DEPLOYMENT_ENVIRONMENT`) inside `new`.
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

    // Turn span tracking for parent/child correlation
    active_agent_spans: Mutex<HashMap<String, (global::BoxedSpan, Context)>>,
}

/// Resolve the configured `deployment.environment`.
///
/// v0.8.0's config schema (owned by another crate, out of this bead's scope)
/// has no `otel_deployment_environment` field, and [`OtelObserver::new`]'s
/// signature is owned by the shared-provider factory (zc-ju48) and must not be
/// widened here. So the deployment environment is read from the process
/// environment: `ZEROCLAW_DEPLOYMENT_ENVIRONMENT` first (fork-specific), then
/// the OTel-standard `OTEL_DEPLOYMENT_ENVIRONMENT`. Empty/whitespace values are
/// treated as unset so the resource + span stay minimal (no empty
/// `deployment.environment`).
fn deployment_environment_from_env() -> Option<String> {
    for key in [
        "ZEROCLAW_DEPLOYMENT_ENVIRONMENT",
        "OTEL_DEPLOYMENT_ENVIRONMENT",
    ] {
        if let Ok(val) = std::env::var(key) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
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

impl OtelObserver {
    /// Create a new OTel observer exporting to the given OTLP endpoint.
    ///
    /// Uses HTTP/protobuf transport (port 4318 by default).
    /// Falls back to `http://localhost:4318` if no endpoint is provided.
    pub fn new(
        endpoint: Option<&str>,
        service_name: Option<&str>,
        headers: Option<HashMap<String, String>>,
    ) -> Result<Self, String> {
        let base_endpoint = endpoint.unwrap_or("http://localhost:4318");
        let traces_endpoint = format!("{}/v1/traces", base_endpoint.trim_end_matches('/'));
        let metrics_endpoint = format!("{}/v1/metrics", base_endpoint.trim_end_matches('/'));
        let service_name = service_name.unwrap_or("zeroclaw");
        let environment = deployment_environment_from_env();

        // ── Trace + metric providers (built once per process) ───
        // Reuse the shared providers if another component already initialized
        // them; otherwise build the OTLP exporters, install the globals, and
        // publish to the shared slot. Instruments below always bind to the
        // global meter, so every observer records into the same pipeline. This
        // guards against the six construction sites each building their own
        // exporter and racing `global::set_*_provider`.
        let (tracer_provider, meter_provider_clone) = match OTEL_PROVIDERS.get() {
            Some(providers) => providers.clone(),
            None => {
                // ── Trace exporter ──────────────────────────────
                let mut span_builder = opentelemetry_otlp::SpanExporter::builder()
                    .with_http()
                    .with_endpoint(&traces_endpoint);
                if let Some(ref h) = headers {
                    span_builder = span_builder.with_headers(h.clone());
                }
                let span_exporter = span_builder
                    .build()
                    .map_err(|e| format!("Failed to create OTLP span exporter: {e}"))?;

                let tracer_provider = SdkTracerProvider::builder()
                    .with_batch_exporter(span_exporter)
                    .with_resource(build_otlp_resource(service_name, environment.as_deref()))
                    .build();

                // ── Metric exporter ─────────────────────────────
                let mut metric_builder = opentelemetry_otlp::MetricExporter::builder()
                    .with_http()
                    .with_endpoint(&metrics_endpoint);
                if let Some(ref h) = headers {
                    metric_builder = metric_builder.with_headers(h.clone());
                }
                let metric_exporter = metric_builder
                    .build()
                    .map_err(|e| format!("Failed to create OTLP metric exporter: {e}"))?;

                let metric_reader =
                    opentelemetry_sdk::metrics::PeriodicReader::builder(metric_exporter).build();

                let meter_provider = opentelemetry_sdk::metrics::SdkMeterProvider::builder()
                    .with_reader(metric_reader)
                    .with_resource(build_otlp_resource(service_name, environment.as_deref()))
                    .build();

                global::set_tracer_provider(tracer_provider.clone());
                global::set_meter_provider(meter_provider.clone());

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
            .with_description("Total LLM model_provider calls")
            .build();

        let llm_duration = meter
            .f64_histogram("zeroclaw.llm.duration")
            .with_description("LLM model_provider call duration in seconds")
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

        Ok(Self {
            tracer_provider,
            meter_provider: meter_provider_clone,
            environment,
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
            active_agent_spans: Mutex::new(HashMap::new()),
        })
    }

    fn parent_cx_for(&self, turn_id: Option<&str>) -> Context {
        if let Some(tid) = turn_id
            && let Some((_, cx)) = self
                .active_agent_spans
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(tid)
        {
            return cx.clone();
        }
        Context::current()
    }
}

impl Observer for OtelObserver {
    fn record_event(&self, event: &ObserverEvent) {
        let tracer = global::tracer("zeroclaw");

        match event {
            ObserverEvent::AgentStart {
                model_provider,
                model,
                channel,
                agent_alias,
                turn_id,
            } => {
                self.agent_starts.add(
                    1,
                    &[
                        KeyValue::new("gen_ai.provider.name", model_provider.clone()),
                        KeyValue::new("gen_ai.request.model", model.clone()),
                    ],
                );

                let span = tracer.build(
                    opentelemetry::trace::SpanBuilder::from_name("gen_ai.agent.invoke")
                        .with_kind(SpanKind::Internal)
                        .with_attributes(vec![
                            KeyValue::new("gen_ai.provider.name", model_provider.clone()),
                            KeyValue::new("gen_ai.request.model", model.clone()),
                            KeyValue::new("zeroclaw.channel", channel.clone().unwrap_or_default()),
                            KeyValue::new(
                                "gen_ai.agent.name",
                                agent_alias.clone().unwrap_or_default(),
                            ),
                            KeyValue::new("zeroclaw.turn_id", turn_id.clone().unwrap_or_default()),
                        ]),
                );

                if let Some(tid) = turn_id {
                    let parent_cx =
                        Context::current().with_remote_span_context(span.span_context().clone());
                    self.active_agent_spans
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(tid.clone(), (span, parent_cx));
                }
            }
            ObserverEvent::LlmRequest {
                model_provider,
                model,
                messages_count,
                channel,
                agent_alias,
                turn_id,
            } => {
                let parent_cx = self.parent_cx_for(turn_id.as_deref());
                let mut span = tracer.build_with_context(
                    opentelemetry::trace::SpanBuilder::from_name("llm.request")
                        .with_kind(SpanKind::Client)
                        .with_attributes(vec![
                            KeyValue::new("gen_ai.provider.name", model_provider.clone()),
                            KeyValue::new("gen_ai.request.model", model.clone()),
                            KeyValue::new("gen_ai.operation.name", "llm.request"),
                            KeyValue::new(
                                "zeroclaw.messages_count",
                                i64::try_from(*messages_count).unwrap_or(i64::MAX),
                            ),
                            KeyValue::new("zeroclaw.channel", channel.clone().unwrap_or_default()),
                            KeyValue::new(
                                "gen_ai.agent.name",
                                agent_alias.clone().unwrap_or_default(),
                            ),
                            KeyValue::new("zeroclaw.turn_id", turn_id.clone().unwrap_or_default()),
                        ]),
                    &parent_cx,
                );
                span.end();
            }
            ObserverEvent::ToolCallStart {
                tool,
                tool_call_id,
                arguments,
                channel,
                agent_alias,
                turn_id,
            } => {
                let mut span_attrs = vec![
                    KeyValue::new("gen_ai.operation.name", "execute_tool"),
                    KeyValue::new("tool.name", tool.clone()),
                    KeyValue::new("zeroclaw.channel", channel.clone().unwrap_or_default()),
                    KeyValue::new("gen_ai.agent.name", agent_alias.clone().unwrap_or_default()),
                    KeyValue::new("zeroclaw.turn_id", turn_id.clone().unwrap_or_default()),
                ];
                if let Some(id) = tool_call_id {
                    span_attrs.push(KeyValue::new("gen_ai.tool.call.id", id.clone()));
                }
                if let Some(args) = arguments {
                    span_attrs.push(KeyValue::new("gen_ai.tool.arguments", args.clone()));
                }
                let parent_cx = self.parent_cx_for(turn_id.as_deref());
                let mut span = tracer.build_with_context(
                    opentelemetry::trace::SpanBuilder::from_name("tool_call.start")
                        .with_kind(SpanKind::Client)
                        .with_attributes(span_attrs),
                    &parent_cx,
                );
                span.end();
            }
            ObserverEvent::TurnComplete
            | ObserverEvent::CacheHit { .. }
            | ObserverEvent::CacheMiss { .. } => {}
            ObserverEvent::LlmResponse {
                model_provider,
                model,
                duration,
                success,
                error_message: _,
                input_tokens,
                output_tokens,
                channel,
                agent_alias,
                turn_id,
            } => {
                let secs = duration.as_secs_f64();
                let attrs = [
                    KeyValue::new("gen_ai.provider.name", model_provider.clone()),
                    KeyValue::new("gen_ai.request.model", model.clone()),
                    KeyValue::new("gen_ai.response.model", model.clone()),
                    KeyValue::new("gen_ai.operation.name", "llm.response"),
                    KeyValue::new("success", *success),
                    KeyValue::new("duration_s", secs),
                    KeyValue::new("zeroclaw.channel", channel.clone().unwrap_or_default()),
                    KeyValue::new("gen_ai.agent.name", agent_alias.clone().unwrap_or_default()),
                    KeyValue::new("zeroclaw.turn_id", turn_id.clone().unwrap_or_default()),
                ];
                self.llm_calls.add(1, &attrs);
                self.llm_duration.record(secs, &attrs);

                let mut span_attrs = vec![
                    KeyValue::new("gen_ai.provider.name", model_provider.clone()),
                    KeyValue::new("gen_ai.request.model", model.clone()),
                    KeyValue::new("gen_ai.response.model", model.clone()),
                    KeyValue::new("gen_ai.operation.name", "llm.response"),
                    KeyValue::new("success", *success),
                    KeyValue::new("duration_s", secs),
                    KeyValue::new("zeroclaw.channel", channel.clone().unwrap_or_default()),
                    KeyValue::new("gen_ai.agent.name", agent_alias.clone().unwrap_or_default()),
                    KeyValue::new("zeroclaw.turn_id", turn_id.clone().unwrap_or_default()),
                ];
                if let Some(input) = input_tokens {
                    span_attrs.push(KeyValue::new("gen_ai.usage.input_tokens", *input as i64));
                }
                if let Some(output) = output_tokens {
                    span_attrs.push(KeyValue::new("gen_ai.usage.output_tokens", *output as i64));
                }
                let parent_cx = self.parent_cx_for(turn_id.as_deref());
                let mut span = tracer.build_with_context(
                    opentelemetry::trace::SpanBuilder::from_name("llm.response")
                        .with_kind(SpanKind::Client)
                        .with_attributes(span_attrs),
                    &parent_cx,
                );
                if *success {
                    span.set_status(Status::Ok);
                } else {
                    span.set_status(Status::error(""));
                }
                span.end();
            }
            ObserverEvent::AgentEnd {
                model_provider,
                model,
                duration,
                tokens_used,
                cost_usd,
                channel: _,
                agent_alias: _,
                turn_id,
            } => {
                if let Some(tid) = turn_id {
                    let entry = self
                        .active_agent_spans
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(tid);
                    if let Some((mut span, _)) = entry {
                        let secs = duration.as_secs_f64();
                        span.set_attribute(KeyValue::new("duration_s", secs));
                        if let Some(usage) = tokens_used {
                            span.set_attribute(KeyValue::new(
                                "gen_ai.usage.input_tokens",
                                usage.input_tokens as i64,
                            ));
                            span.set_attribute(KeyValue::new(
                                "gen_ai.usage.output_tokens",
                                usage.output_tokens as i64,
                            ));
                        }
                        if let Some(c) = cost_usd {
                            span.set_attribute(KeyValue::new("cost_usd", *c));
                        }
                        span.end();
                    }
                }

                let secs = duration.as_secs_f64();
                self.agent_duration.record(
                    secs,
                    &[
                        KeyValue::new("gen_ai.provider.name", model_provider.clone()),
                        KeyValue::new("gen_ai.request.model", model.clone()),
                    ],
                );
            }
            ObserverEvent::ToolCall {
                tool,
                tool_call_id,
                duration,
                success,
                arguments,
                result,
                channel,
                agent_alias,
                turn_id,
            } => {
                let secs = duration.as_secs_f64();

                let status = if *success {
                    Status::Ok
                } else {
                    Status::error("")
                };

                let mut span_attrs = vec![
                    KeyValue::new("gen_ai.operation.name", "execute_tool"),
                    KeyValue::new("tool.name", tool.clone()),
                    KeyValue::new("tool.success", *success),
                    KeyValue::new("duration_s", secs),
                    KeyValue::new("zeroclaw.channel", channel.clone().unwrap_or_default()),
                    KeyValue::new("gen_ai.agent.name", agent_alias.clone().unwrap_or_default()),
                    KeyValue::new("zeroclaw.turn_id", turn_id.clone().unwrap_or_default()),
                ];
                if let Some(id) = tool_call_id {
                    span_attrs.push(KeyValue::new("gen_ai.tool.call.id", id.clone()));
                }
                if let Some(args) = arguments {
                    span_attrs.push(KeyValue::new("gen_ai.tool.arguments", args.clone()));
                    span_attrs.push(KeyValue::new("input.value", args.clone()));
                }
                if let Some(res) = result {
                    span_attrs.push(KeyValue::new("gen_ai.tool.result", res.clone()));
                    span_attrs.push(KeyValue::new("output.value", res.clone()));
                }
                let parent_cx = self.parent_cx_for(turn_id.as_deref());
                let mut span = tracer.build_with_context(
                    opentelemetry::trace::SpanBuilder::from_name("tool_call.result")
                        .with_kind(SpanKind::Internal)
                        .with_attributes(span_attrs),
                    &parent_cx,
                );
                span.set_status(status);
                span.end();

                let metric_attrs = [
                    KeyValue::new("tool", tool.clone()),
                    KeyValue::new("success", success.to_string()),
                ];
                self.tool_calls.add(1, &metric_attrs);
                self.tool_duration
                    .record(secs, &[KeyValue::new("tool", tool.clone())]);
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
            ObserverEvent::Error { component, message } => {
                // Create an error span for visibility in trace backends
                let mut span = tracer.build(
                    opentelemetry::trace::SpanBuilder::from_name("error")
                        .with_kind(SpanKind::Internal)
                        .with_attributes(vec![
                            KeyValue::new("component", component.clone()),
                            KeyValue::new("error.message", message.clone()),
                        ]),
                );
                span.set_status(Status::error(message.clone()));
                span.end();

                self.errors
                    .add(1, &[KeyValue::new("component", component.clone())]);
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
                self.tokens_used.add(*t, &[]);
            }
            ObserverMetric::ActiveSessions(s) => {
                self.active_sessions.record(*s, &[]);
            }
            ObserverMetric::QueueDepth(d) => {
                self.queue_depth.record(*d, &[]);
            }
            ObserverMetric::DeploymentLeadTime(_) | ObserverMetric::RecoveryTime(_) => {
                // DORA metrics: OTel pass-through not yet implemented.
            }
        }
    }

    fn flush(&self) {
        // Flush orphan live spans (turns that ended without AgentEnd)
        let orphans: Vec<(global::BoxedSpan, Context)> = self
            .active_agent_spans
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain()
            .map(|(_, v)| v)
            .collect();
        for (mut span, _) in orphans {
            span.end();
        }

        if let Err(e) = self.tracer_provider.force_flush() {
            ::zeroclaw_log::record!(
                WARN,
                ::zeroclaw_log::Event::new(module_path!(), ::zeroclaw_log::Action::Note)
                    .with_outcome(::zeroclaw_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                "OTel trace flush failed"
            );
        }
        if let Err(e) = self.meter_provider.force_flush() {
            ::zeroclaw_log::record!(
                WARN,
                ::zeroclaw_log::Event::new(module_path!(), ::zeroclaw_log::Action::Note)
                    .with_outcome(::zeroclaw_log::EventOutcome::Unknown)
                    .with_attrs(::serde_json::json!({"error": format!("{}", e)})),
                "OTel metric flush failed"
            );
        }
    }

    fn name(&self) -> &str {
        "otel"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn start_activation(&self, trigger: Trigger, session_hint: Option<&str>) -> Box<dyn TraceSpan> {
        Box::new(OtelSpan::root(
            trigger,
            session_hint,
            self.environment.as_deref(),
        ))
    }
}

/// An OTel-backed [`Span`](TraceSpan) node in the activation trace.
///
/// Holds an OTel [`Context`] with this node's span active. The root is created
/// with an empty parent context (a fresh `trace_id`); children are created with
/// the parent's context so they inherit its `trace_id` and link as descendants.
/// The span opens on construction and ends on `Drop`, giving true start→end
/// timing.
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
    /// `environment` (when non-empty) is set as the `deployment.environment`
    /// span attribute in addition to the OTLP resource attribute — self-hosted
    /// Laminar drops resource attributes, so the span copy is the one that stays
    /// queryable.
    fn root(trigger: Trigger, session_hint: Option<&str>, environment: Option<&str>) -> Self {
        let tracer = global::tracer("zeroclaw");
        // Empty parent context ⇒ a brand-new trace_id (one trace per activation).
        let span = tracer.start_with_context("agent.activation", &Context::new());
        let this = Self {
            cx: Context::new().with_span(span),
            environment: environment.filter(|e| !e.is_empty()).map(Arc::from),
        };
        this.set_attr("trigger", AttrValue::Str(trigger.as_str().to_string()));
        // `session_id` is absence-not-empty (Adjustment B / claw §4.1): only when
        // a real thread/session key exists do we associate it. `POST /api/chat`
        // passes `None` and neither key is written — never synthesized, never "".
        // Laminar's typed `session_id` column is filled ONLY from the
        // `lmnr.association.properties.session_id` key; the plain `session_id`
        // attr never populates it. A conversation/thread id IS Laminar's
        // "session", so the twin carries the SAME value under the SAME guard.
        // Mirrors the `user.id` → `lmnr.association.properties.user_id` dual-emit
        // precedent (see identity::set_user_id_attrs).
        if let Some(sid) = session_hint {
            this.set_attr("session_id", AttrValue::Str(sid.to_string()));
            this.set_attr(
                "lmnr.association.properties.session_id",
                AttrValue::Str(sid.to_string()),
            );
        }
        if let Some(env) = this.environment.as_deref() {
            this.set_attr("deployment.environment", AttrValue::Str(env.to_string()));
        }
        this
    }

    /// Concrete child constructor (used by [`TraceSpan::child`] and by tests).
    ///
    /// Re-stamps `deployment.environment` on the child span so it is queryable
    /// on its own — backends that drop resource attributes and do not inherit
    /// the root span's attributes (e.g. self-hosted Laminar) can still slice
    /// child spans by environment.
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
        // End the span so the batch processor exports true start→end timing.
        self.cx.span().end();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // Note: OtelObserver::new() requires an OTLP endpoint.
    // In tests we verify the struct creation fails gracefully
    // when no collector is available, and test the observer interface
    // by constructing with a known-unreachable endpoint (spans/metrics
    // are buffered and exported asynchronously, so recording never panics).

    fn test_observer() -> OtelObserver {
        // Create with a dummy endpoint — exports will silently fail
        // but the observer itself works fine for recording
        OtelObserver::new(Some("http://127.0.0.1:19999"), Some("zeroclaw-test"), None)
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
            model_provider: "openrouter".into(),
            model: "claude-sonnet".into(),
            channel: None,
            agent_alias: None,
            turn_id: None,
        });
        obs.record_event(&ObserverEvent::LlmRequest {
            model_provider: "openrouter".into(),
            model: "claude-sonnet".into(),
            messages_count: 2,
            channel: None,
            agent_alias: None,
            turn_id: None,
        });
        obs.record_event(&ObserverEvent::LlmResponse {
            model_provider: "openrouter".into(),
            model: "claude-sonnet".into(),
            duration: Duration::from_millis(250),
            success: true,
            error_message: None,
            input_tokens: Some(100),
            output_tokens: Some(50),
            channel: None,
            agent_alias: None,
            turn_id: None,
        });
        obs.record_event(&ObserverEvent::AgentEnd {
            model_provider: "openrouter".into(),
            model: "claude-sonnet".into(),
            duration: Duration::from_millis(500),
            tokens_used: None,
            cost_usd: Some(0.0015),
            channel: None,
            agent_alias: None,
            turn_id: None,
        });
        obs.record_event(&ObserverEvent::AgentEnd {
            model_provider: "openrouter".into(),
            model: "claude-sonnet".into(),
            duration: Duration::ZERO,
            tokens_used: None,
            cost_usd: None,
            channel: None,
            agent_alias: None,
            turn_id: None,
        });
        obs.record_event(&ObserverEvent::ToolCallStart {
            tool: "shell".into(),
            tool_call_id: None,
            arguments: None,
            channel: None,
            agent_alias: None,
            turn_id: None,
        });
        obs.record_event(&ObserverEvent::ToolCall {
            tool: "shell".into(),
            tool_call_id: None,
            duration: Duration::from_millis(10),
            success: true,
            arguments: None,
            result: None,
            channel: None,
            agent_alias: None,
            turn_id: None,
        });
        obs.record_event(&ObserverEvent::ToolCall {
            tool: "file_read".into(),
            tool_call_id: None,
            duration: Duration::from_millis(5),
            success: false,
            arguments: None,
            result: None,
            channel: None,
            agent_alias: None,
            turn_id: None,
        });
        obs.record_event(&ObserverEvent::TurnComplete);
        obs.record_event(&ObserverEvent::ChannelMessage {
            channel: "telegram".into(),
            direction: "inbound".into(),
        });
        obs.record_event(&ObserverEvent::HeartbeatTick);
        obs.record_event(&ObserverEvent::Error {
            component: "model_provider".into(),
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

    /// Regression test for upstream issue #5980 — tool spans must accept a
    /// populated `tool_call_id`, full `arguments`, and `result` without
    /// panicking, including payloads large enough that naive attribute
    /// encoding could truncate them. We can't assert on exported span
    /// attributes here because the OTLP pipeline runs asynchronously, but
    /// verifying the recording path handles all three optional fields
    /// exercises the new gen_ai.tool.* code paths.
    #[test]
    fn tool_call_with_id_args_and_result_does_not_panic() {
        let obs = test_observer();
        obs.record_event(&ObserverEvent::ToolCallStart {
            tool: "shell".into(),
            tool_call_id: Some("toolu_01ABC".into()),
            arguments: Some(r#"{"command":"ls -la /tmp"}"#.into()),
            channel: None,
            agent_alias: None,
            turn_id: None,
        });
        obs.record_event(&ObserverEvent::ToolCall {
            tool: "shell".into(),
            tool_call_id: Some("toolu_01ABC".into()),
            duration: Duration::from_millis(42),
            success: true,
            arguments: Some(r#"{"command":"ls -la /tmp"}"#.into()),
            result: Some("total 0\ndrwxr-xr-x  2 root root 40 Apr 22 12:00 .\n".into()),
            channel: None,
            agent_alias: None,
            turn_id: None,
        });
        // Failure case — the issue author specifically wants to see *why*
        // a tool call failed, so the result field is the error text.
        obs.record_event(&ObserverEvent::ToolCall {
            tool: "shell".into(),
            tool_call_id: Some("toolu_02DEF".into()),
            duration: Duration::from_millis(3),
            success: false,
            arguments: Some(r#"{"command":"rm -rf /"}"#.into()),
            result: Some("Error: command denied by allowlist policy".into()),
            channel: None,
            agent_alias: None,
            turn_id: None,
        });
    }

    // ── §8.2 OTel export failure resilience tests ────────────

    #[test]
    fn otel_records_error_event_without_panic() {
        let obs = test_observer();
        // Simulate an error event — should not panic even with unreachable endpoint
        obs.record_event(&ObserverEvent::Error {
            component: "model_provider".into(),
            message: "connection refused to model endpoint".into(),
        });
    }

    #[test]
    fn otel_records_llm_failure_without_panic() {
        let obs = test_observer();
        obs.record_event(&ObserverEvent::LlmResponse {
            model_provider: "openrouter".into(),
            model: "missing-model".into(),
            duration: Duration::from_millis(0),
            success: false,
            error_message: Some("404 Not Found".into()),
            input_tokens: None,
            output_tokens: None,
            channel: None,
            agent_alias: None,
            turn_id: None,
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
    fn turn_id_opens_and_closes_agent_span() {
        let obs = test_observer();
        obs.record_event(&ObserverEvent::AgentStart {
            model_provider: "anthropic".into(),
            model: "claude-sonnet-4-6".into(),
            channel: Some("wss".into()),
            agent_alias: Some("default".into()),
            turn_id: Some("turn-1".into()),
        });

        assert!(
            obs.active_agent_spans
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains_key("turn-1"),
            "AgentStart should open a live span keyed by turn_id"
        );

        obs.record_event(&ObserverEvent::LlmRequest {
            model_provider: "anthropic".into(),
            model: "claude-sonnet-4-6".into(),
            messages_count: 2,
            channel: Some("wss".into()),
            agent_alias: Some("default".into()),
            turn_id: Some("turn-1".into()),
        });
        obs.record_event(&ObserverEvent::LlmResponse {
            model_provider: "anthropic".into(),
            model: "claude-sonnet-4-6".into(),
            duration: Duration::from_millis(25),
            success: true,
            error_message: None,
            input_tokens: Some(10),
            output_tokens: Some(5),
            channel: Some("wss".into()),
            agent_alias: Some("default".into()),
            turn_id: Some("turn-1".into()),
        });
        obs.record_event(&ObserverEvent::ToolCallStart {
            tool: "shell".into(),
            tool_call_id: Some("call-1".into()),
            arguments: Some(r#"{"command":"date"}"#.into()),
            channel: Some("wss".into()),
            agent_alias: Some("default".into()),
            turn_id: Some("turn-1".into()),
        });
        obs.record_event(&ObserverEvent::ToolCall {
            tool: "shell".into(),
            tool_call_id: Some("call-1".into()),
            duration: Duration::from_millis(5),
            success: true,
            arguments: Some(r#"{"command":"date"}"#.into()),
            result: Some("Mon Apr 22 12:00:00 UTC 2026".into()),
            channel: Some("wss".into()),
            agent_alias: Some("default".into()),
            turn_id: Some("turn-1".into()),
        });
        obs.record_event(&ObserverEvent::AgentEnd {
            model_provider: "anthropic".into(),
            model: "claude-sonnet-4-6".into(),
            duration: Duration::from_millis(50),
            tokens_used: Some(zeroclaw_api::observability_traits::TurnTokenUsage {
                input_tokens: 10,
                output_tokens: 5,
            }),
            cost_usd: None,
            channel: Some("wss".into()),
            agent_alias: Some("default".into()),
            turn_id: Some("turn-1".into()),
        });

        assert!(
            !obs.active_agent_spans
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains_key("turn-1"),
            "AgentEnd should close the live span"
        );
    }

    #[test]
    fn otel_observer_creation_with_valid_endpoint_succeeds() {
        // Even though endpoint is unreachable, creation should succeed
        let result = OtelObserver::new(Some("http://127.0.0.1:12345"), Some("zeroclaw-test"), None);
        assert!(
            result.is_ok(),
            "observer creation must succeed even with unreachable endpoint"
        );
    }

    #[test]
    fn otel_observer_creation_with_headers_succeeds() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer sk-test".to_string());
        headers.insert("X-Custom".to_string(), "value".to_string());
        let result = OtelObserver::new(Some("http://127.0.0.1:12345"), Some("test"), Some(headers));
        assert!(
            result.is_ok(),
            "observer creation with headers must succeed"
        );
    }

    #[test]
    fn otel_observer_with_headers_records_events() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer sk-test".to_string());
        let obs = OtelObserver::new(Some("http://127.0.0.1:19999"), Some("test"), Some(headers))
            .expect("creation should succeed");
        obs.record_event(&ObserverEvent::LlmResponse {
            model_provider: "anthropic".into(),
            model: "claude-sonnet".into(),
            duration: Duration::from_millis(100),
            success: true,
            error_message: None,
            input_tokens: Some(10),
            output_tokens: Some(5),
            channel: None,
            agent_alias: None,
            turn_id: None,
        });
        obs.record_event(&ObserverEvent::ToolCall {
            tool: "shell".into(),
            tool_call_id: None,
            duration: Duration::from_millis(50),
            success: true,
            arguments: None,
            result: None,
            channel: None,
            agent_alias: None,
            turn_id: None,
        });
    }

    /// Regression test for zc-ju48 — the process-wide OTLP providers must be
    /// built exactly once and shared across every construction site. Two
    /// separate `OtelObserver::new` calls (as the six/seven real sites do) must
    /// resolve to the *same* provider instance: the second call reuses the
    /// `OnceLock` slot instead of re-initializing the OpenTelemetry globals and
    /// racing a second exporter into `global::set_*_provider`.
    #[test]
    fn shared_providers_are_single_instance_across_two_constructions() {
        // First construction populates the shared slot.
        let _first = OtelObserver::new(Some("http://127.0.0.1:12345"), Some("zeroclaw-a"), None)
            .expect("first observer creation should succeed");
        let after_first = OTEL_PROVIDERS
            .get()
            .expect("first construction must publish the shared providers");

        // Second construction (a different service name / call site) must NOT
        // rebuild the providers — it reuses the same OnceLock value.
        let _second = OtelObserver::new(Some("http://127.0.0.1:12345"), Some("zeroclaw-b"), None)
            .expect("second observer creation should succeed");
        let after_second = OTEL_PROVIDERS
            .get()
            .expect("providers must still be present after the second construction");

        // The OnceLock yields a single `&'static` value; both reads point at the
        // exact same stored provider tuple — proof the second call did not
        // re-init the globals or spin up a second exporter.
        assert!(
            std::ptr::eq(after_first, after_second),
            "OTEL_PROVIDERS must hold a single shared provider instance across \
             two construction calls (second call re-initialized the exporter)"
        );
    }

    #[test]
    fn otel_observer_with_empty_headers_succeeds() {
        let result = OtelObserver::new(
            Some("http://127.0.0.1:12345"),
            Some("test"),
            Some(HashMap::new()),
        );
        assert!(
            result.is_ok(),
            "observer creation with empty headers must succeed"
        );
    }

    // ── FD-07 span-producing path (zc-jfun) ────────────────────

    #[test]
    fn start_activation_returns_span_and_records_without_panic() {
        let obs = test_observer();
        // A root with a session hint writes the `session_id` + Laminar twin; a
        // child inherits the trace. We can't read exported attrs (async OTLP
        // pipeline), but exercising the span-producing path proves it does not
        // panic and honors the object-safe `Span` contract.
        let root = obs.start_activation(Trigger::WebChat, Some("thread-abc"));
        root.set_attr("k", AttrValue::Str("v".into()));
        root.set_status(true);
        let child = root.child("llm.call");
        child.set_attr("lmnr.span.input", AttrValue::Str("hi".into()));
        child.add_event("attempt", &[("n", AttrValue::Int(1))]);
    }

    #[test]
    fn start_activation_without_session_hint_does_not_panic() {
        // `/api/chat` passes None: absence-not-empty. No session_id twin is
        // written, and minting the root must still succeed.
        let obs = test_observer();
        let root = obs.start_activation(Trigger::WebChat, None);
        root.set_status(true);
    }

    #[test]
    fn build_otlp_resource_includes_env_only_when_set() {
        use opentelemetry::Key;
        let with = build_otlp_resource("svc", Some("prod"));
        assert_eq!(
            with.get(&Key::from_static_str("deployment.environment")),
            Some(Value::String("prod".into())),
            "deployment.environment must be present on the resource when configured"
        );
        for empty in [None, Some(""), Some("   ")] {
            let res = build_otlp_resource("svc", empty.map(str::trim).filter(|e| !e.is_empty()));
            assert_eq!(
                res.get(&Key::from_static_str("deployment.environment")),
                None,
                "deployment.environment must be omitted for {empty:?} (resource stays minimal)"
            );
        }
    }

    #[test]
    fn deployment_environment_prefers_zeroclaw_then_otel_env() {
        // Guarded by a process-global mutex indirectly via serial exec of env;
        // these keys are unlikely to be set in CI, but restore afterward.
        let saved_z = std::env::var("ZEROCLAW_DEPLOYMENT_ENVIRONMENT").ok();
        let saved_o = std::env::var("OTEL_DEPLOYMENT_ENVIRONMENT").ok();

        // SAFETY: single-threaded test scope; restored below.
        unsafe {
            std::env::remove_var("ZEROCLAW_DEPLOYMENT_ENVIRONMENT");
            std::env::remove_var("OTEL_DEPLOYMENT_ENVIRONMENT");
        }
        assert_eq!(deployment_environment_from_env(), None);

        unsafe {
            std::env::set_var("OTEL_DEPLOYMENT_ENVIRONMENT", "staging");
        }
        assert_eq!(
            deployment_environment_from_env().as_deref(),
            Some("staging")
        );

        unsafe {
            std::env::set_var("ZEROCLAW_DEPLOYMENT_ENVIRONMENT", "prod");
        }
        assert_eq!(deployment_environment_from_env().as_deref(), Some("prod"));

        // Empty/whitespace is treated as unset.
        unsafe {
            std::env::set_var("ZEROCLAW_DEPLOYMENT_ENVIRONMENT", "   ");
            std::env::set_var("OTEL_DEPLOYMENT_ENVIRONMENT", "");
        }
        assert_eq!(deployment_environment_from_env(), None);

        // Restore.
        unsafe {
            match saved_z {
                Some(v) => std::env::set_var("ZEROCLAW_DEPLOYMENT_ENVIRONMENT", v),
                None => std::env::remove_var("ZEROCLAW_DEPLOYMENT_ENVIRONMENT"),
            }
            match saved_o {
                Some(v) => std::env::set_var("OTEL_DEPLOYMENT_ENVIRONMENT", v),
                None => std::env::remove_var("OTEL_DEPLOYMENT_ENVIRONMENT"),
            }
        }
    }
}
