//! Tool execution helpers extracted from `loop_`.
//!
//! Contains the functions responsible for invoking tools (single, parallel,
//! sequential) and the decision logic for choosing between parallel and
//! sequential execution.

use anyhow::Result;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

use std::sync::Arc;

use crate::approval::ApprovalManager;
use crate::observability::{AttrValue, Observer, ObserverEvent, Span, current_span, scope_span};
use crate::tools::Tool;
use crate::util::truncate_with_ellipsis;

// Items that still live in `loop_` — import via the parent module.
use super::loop_::{ParsedToolCall, ToolLoopCancelled, scrub_credentials};

// ── Helpers ──────────────────────────────────────────────────────────────

/// Look up a tool by name in a slice of boxed `dyn Tool` values.
pub(crate) fn find_tool<'a>(tools: &'a [Box<dyn Tool>], name: &str) -> Option<&'a dyn Tool> {
    tools.iter().find(|t| t.name() == name).map(|t| t.as_ref())
}

// ── Outcome ──────────────────────────────────────────────────────────────

pub(crate) struct ToolExecutionOutcome {
    pub(crate) output: String,
    pub(crate) success: bool,
    pub(crate) error_reason: Option<String>,
    pub(crate) duration: Duration,
}

// ── Single tool execution ────────────────────────────────────────────────

pub(crate) async fn execute_one_tool(
    call_name: &str,
    call_arguments: serde_json::Value,
    tools_registry: &[Box<dyn Tool>],
    activated_tools: Option<&std::sync::Arc<std::sync::Mutex<crate::tools::ActivatedToolSet>>>,
    observer: &dyn Observer,
    cancellation_token: Option<&CancellationToken>,
) -> Result<ToolExecutionOutcome> {
    let args_summary = truncate_with_ellipsis(&call_arguments.to_string(), 300);
    observer.record_event(&ObserverEvent::ToolCallStart {
        tool: call_name.to_string(),
        arguments: Some(args_summary),
    });
    let start = Instant::now();

    let static_tool = find_tool(tools_registry, call_name);
    let activated_arc = if static_tool.is_none() {
        activated_tools.and_then(|at| at.lock().unwrap().get_resolved(call_name))
    } else {
        None
    };
    let Some(tool) = static_tool.or(activated_arc.as_deref()) else {
        let reason = format!("Unknown tool: {call_name}");
        let duration = start.elapsed();
        observer.record_event(&ObserverEvent::ToolCall {
            tool: call_name.to_string(),
            duration,
            success: false,
        });
        return Ok(ToolExecutionOutcome {
            output: reason.clone(),
            success: false,
            error_reason: Some(scrub_credentials(&reason)),
            duration,
        });
    };

    // Open a `tool.call` span and make it the ambient parent while the tool runs, so any
    // nested agent work (e.g. the delegate tool's sub-loop) parents beneath it.
    let tool_span: Option<Arc<dyn Span>> = current_span().map(|s| Arc::from(s.child("tool.call")));
    if let Some(ts) = &tool_span {
        ts.set_attr("tool.name", AttrValue::Str(call_name.to_string()));
        // tool.input = the invocation args, scrubbed + truncated to 16k (mirrors tool.output).
        // Surfaces WHAT the agent asked the tool to do, not just the result. Args routinely
        // carry secrets, so scrub_credentials is mandatory here. Legalized under §7.1.
        ts.set_attr(
            "tool.input",
            AttrValue::Str(truncate_with_ellipsis(
                &scrub_credentials(&call_arguments.to_string()),
                16_000,
            )),
        );
        // Composio is a single tool; differentiate by toolkit/action from the args.
        // (The Composio `log_…` id is not returned by the v3 execute API — unavailable.)
        if call_name == "composio" {
            if let Some(action) = call_arguments.get("action_name").and_then(|v| v.as_str()) {
                ts.set_attr("composio.action", AttrValue::Str(action.to_string()));
            }
            if let Some(app) = call_arguments.get("app").and_then(|v| v.as_str()) {
                ts.set_attr("composio.toolkit", AttrValue::Str(app.to_string()));
            }
        }
    }

    let tool_future = tool.execute(call_arguments);
    let exec = async move {
        if let Some(token) = cancellation_token {
            tokio::select! {
                () = token.cancelled() => None,
                result = tool_future => Some(result),
            }
        } else {
            Some(tool_future.await)
        }
    };
    let tool_result = match &tool_span {
        Some(ts) => scope_span(ts.clone(), exec).await,
        None => exec.await,
    };
    let tool_result = match tool_result {
        Some(r) => r,
        None => return Err(ToolLoopCancelled.into()),
    };

    match tool_result {
        Ok(r) => {
            let duration = start.elapsed();
            if let Some(ts) = &tool_span {
                ts.set_status(r.success);
            }
            observer.record_event(&ObserverEvent::ToolCall {
                tool: call_name.to_string(),
                duration,
                success: r.success,
            });
            if r.success {
                let output = scrub_credentials(&r.output);
                if let Some(ts) = &tool_span {
                    ts.set_attr(
                        "tool.output",
                        AttrValue::Str(truncate_with_ellipsis(&output, 16_000)),
                    );
                }
                Ok(ToolExecutionOutcome {
                    output,
                    success: true,
                    error_reason: None,
                    duration,
                })
            } else {
                let reason = r.error.unwrap_or(r.output);
                let scrubbed = scrub_credentials(&reason);
                if let Some(ts) = &tool_span {
                    ts.set_attr(
                        "tool.error",
                        AttrValue::Str(truncate_with_ellipsis(&scrubbed, 16_000)),
                    );
                }
                Ok(ToolExecutionOutcome {
                    output: format!("Error: {reason}"),
                    success: false,
                    error_reason: Some(scrubbed),
                    duration,
                })
            }
        }
        Err(e) => {
            let duration = start.elapsed();
            if let Some(ts) = &tool_span {
                ts.set_status(false);
            }
            observer.record_event(&ObserverEvent::ToolCall {
                tool: call_name.to_string(),
                duration,
                success: false,
            });
            let reason = format!("Error executing {call_name}: {e}");
            let scrubbed = scrub_credentials(&reason);
            if let Some(ts) = &tool_span {
                ts.set_attr(
                    "tool.error",
                    AttrValue::Str(truncate_with_ellipsis(&scrubbed, 16_000)),
                );
            }
            Ok(ToolExecutionOutcome {
                output: reason,
                success: false,
                error_reason: Some(scrubbed),
                duration,
            })
        }
    }
}

// ── Parallel / sequential decision ───────────────────────────────────────

pub(crate) fn should_execute_tools_in_parallel(
    tool_calls: &[ParsedToolCall],
    approval: Option<&ApprovalManager>,
) -> bool {
    if tool_calls.len() <= 1 {
        return false;
    }

    // tool_search activates deferred MCP tools into ActivatedToolSet.
    // Running tool_search in parallel with the tools it activates causes a
    // race condition where the tool lookup happens before activation completes.
    // Force sequential execution whenever tool_search is in the batch.
    if tool_calls.iter().any(|call| call.name == "tool_search") {
        return false;
    }

    if let Some(mgr) = approval {
        if tool_calls.iter().any(|call| mgr.needs_approval(&call.name)) {
            // Approval-gated calls must keep sequential handling so the caller can
            // enforce CLI prompt/deny policy consistently.
            return false;
        }
    }

    true
}

// ── Parallel execution ───────────────────────────────────────────────────

pub(crate) async fn execute_tools_parallel(
    tool_calls: &[ParsedToolCall],
    tools_registry: &[Box<dyn Tool>],
    activated_tools: Option<&std::sync::Arc<std::sync::Mutex<crate::tools::ActivatedToolSet>>>,
    observer: &dyn Observer,
    cancellation_token: Option<&CancellationToken>,
) -> Result<Vec<ToolExecutionOutcome>> {
    let futures: Vec<_> = tool_calls
        .iter()
        .map(|call| {
            execute_one_tool(
                &call.name,
                call.arguments.clone(),
                tools_registry,
                activated_tools,
                observer,
                cancellation_token,
            )
        })
        .collect();

    let results = futures_util::future::join_all(futures).await;
    results.into_iter().collect()
}

// ── Sequential execution ─────────────────────────────────────────────────

pub(crate) async fn execute_tools_sequential(
    tool_calls: &[ParsedToolCall],
    tools_registry: &[Box<dyn Tool>],
    activated_tools: Option<&std::sync::Arc<std::sync::Mutex<crate::tools::ActivatedToolSet>>>,
    observer: &dyn Observer,
    cancellation_token: Option<&CancellationToken>,
) -> Result<Vec<ToolExecutionOutcome>> {
    let mut outcomes = Vec::with_capacity(tool_calls.len());

    for call in tool_calls {
        outcomes.push(
            execute_one_tool(
                &call.name,
                call.arguments.clone(),
                tools_registry,
                activated_tools,
                observer,
                cancellation_token,
            )
            .await?,
        );
    }

    Ok(outcomes)
}
