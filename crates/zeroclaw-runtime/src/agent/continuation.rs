//! NextAction continuation auto-drive (rnk-h6g3).
//!
//! When `PRAXIS_AGENT_INVOCATION=1`, every agent-safe praxis verb wraps its
//! `--json` output in a `JsonEnvelope { data, next_action }` (clawcraft
//! praxis-doctrine §6.5). The continuation contract:
//!
//! - `kind: "call"` — invoke `tool` with `args` as-is. Fully mechanical.
//! - `kind: "agent_work_then_call"` — do the `agent_hint` work producing
//!   output matching `output_contract`, then invoke `then.tool` with
//!   `then.args_template` filled in from the work output.
//! - `null` — terminal; no further step.
//!
//! Doctrine finding (traces f3246a74 / e54257b3): weak root reasoning models
//! reliably stall on `agent_work_then_call` — they announce completion in
//! chat and end the turn while a non-null `next_action` is pending,
//! abandoning the execution DAG mid-walk. The binding fix is runtime-side:
//! the agent loop MUST auto-drive continuations rather than trusting the
//! model to follow prose.
//!
//! This module owns the per-turn pending-continuation state machine; the
//! loop integration (mechanical drive of `call`, forcing directives and the
//! turn-termination guard for `agent_work_then_call`, safety-net routing on
//! exhaustion) lives in `loop_.rs`.

use serde::Deserialize;
use std::hash::{Hash, Hasher};

/// Cap on runtime-driven `kind:"call"` invocations per turn. The execution
/// DAG is finite (one `execute` + one `update`-continuation per bead plus the
/// closing `verify`), so a healthy walk stays well under this.
pub(crate) const MAX_AUTO_DRIVE_CALLS: usize = 32;

/// Cap on forcing re-drives for a single `agent_work_then_call` continuation
/// (model stop-attempts that get pushed back into the loop) before the bead
/// is routed to the failure safety net.
pub(crate) const MAX_AGENT_WORK_REDRIVES: usize = 3;

/// The continuation an agent must follow after a praxis verb call.
/// Mirrors `packages/praxis/src/cli/manifest/types.ts:NextAction`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind")]
pub(crate) enum NextAction {
    #[serde(rename = "call")]
    Call {
        tool: String,
        #[serde(default)]
        args: serde_json::Map<String, serde_json::Value>,
    },
    #[serde(rename = "agent_work_then_call")]
    AgentWorkThenCall {
        factory_id: String,
        agent_hint: String,
        #[serde(default)]
        output_contract: serde_json::Value,
        then: ThenCall,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct ThenCall {
    pub(crate) tool: String,
    #[serde(default)]
    pub(crate) args_template: serde_json::Map<String, serde_json::Value>,
}

/// Classification of a tool output with respect to the praxis envelope.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EnvelopeNextAction {
    /// Not an envelope (bare-mode / non-praxis output): no effect.
    NotAnEnvelope,
    /// Envelope with terminal `next_action: null`.
    Terminal,
    /// Envelope with a pending continuation.
    Pending(NextAction),
}

/// Parse a tool output as a praxis `JsonEnvelope`, classifying its
/// `next_action`.
///
/// Detection is shape-based (a JSON object carrying both `data` and
/// `next_action` keys) so it is independent of which tool ran the CLI. The
/// envelope is the whole trimmed output or, failing that, a single line of
/// it (tolerates sync chatter around the JSON line).
pub(crate) fn parse_envelope_next_action(output: &str) -> EnvelopeNextAction {
    // Cheap reject: arbitrary tool output should not pay for JSON parses.
    if !output.contains("next_action") {
        return EnvelopeNextAction::NotAnEnvelope;
    }
    let trimmed = output.trim();
    let candidates = std::iter::once(trimmed).chain(output.lines().rev().map(str::trim));
    for candidate in candidates {
        if !candidate.starts_with('{') {
            continue;
        }
        let Ok(serde_json::Value::Object(obj)) = serde_json::from_str(candidate) else {
            continue;
        };
        if !obj.contains_key("data") || !obj.contains_key("next_action") {
            continue;
        }
        let next_action = &obj["next_action"];
        if next_action.is_null() {
            return EnvelopeNextAction::Terminal;
        }
        return match serde_json::from_value::<NextAction>(next_action.clone()) {
            Ok(action) => EnvelopeNextAction::Pending(action),
            Err(e) => {
                // §6.5 keeps NextAction a closed discriminated union; an
                // unknown kind means the CLI is ahead of this runtime. It
                // cannot be driven — treat as terminal (loudly) rather than
                // wedging the turn open on an unactionable continuation.
                ::zeroclaw_log::record!(
                    WARN,
                    ::zeroclaw_log::Event::new(module_path!(), ::zeroclaw_log::Action::Note)
                        .with_outcome(::zeroclaw_log::EventOutcome::Unknown)
                        .with_attrs(::serde_json::json!({"error": format!("{e}")})),
                    "praxis envelope carried an unparseable next_action; treating as terminal"
                );
                EnvelopeNextAction::Terminal
            }
        };
    }
    EnvelopeNextAction::NotAnEnvelope
}

/// True when an executed tool call corresponds to a continuation target such
/// as `"praxis update"`. Direct tools match by exact name; `shell` calls
/// match when the target's tokens appear as a contiguous token run inside
/// the command (tolerates env-var prefixes, `cd dir && …`, extra flags).
pub(crate) fn call_matches_target(tool_name: &str, args: &serde_json::Value, target: &str) -> bool {
    if tool_name == target {
        return true;
    }
    if tool_name != "shell" {
        return false;
    }
    let Some(command) = args.get("command").and_then(|v| v.as_str()) else {
        return false;
    };
    let cmd_tokens: Vec<&str> = command.split_whitespace().collect();
    let target_tokens: Vec<&str> = target.split_whitespace().collect();
    if target_tokens.is_empty() || cmd_tokens.len() < target_tokens.len() {
        return false;
    }
    cmd_tokens
        .windows(target_tokens.len())
        .any(|w| w == target_tokens.as_slice())
}

/// Single-quote a string for POSIX shells when it contains anything beyond
/// conservative safe characters.
pub(crate) fn shell_quote(raw: &str) -> String {
    let safe = !raw.is_empty()
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/' | '@'));
    if safe {
        raw.to_string()
    } else {
        format!("'{}'", raw.replace('\'', r"'\''"))
    }
}

fn shell_quote_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => shell_quote(s),
        other => shell_quote(&other.to_string()),
    }
}

/// Render a continuation target + args into a praxis CLI command line.
///
/// Mapping mirrors the manifest conventions of the four execution-path
/// emitters (§10.5): the `id` key is the positional bead id, every other key
/// becomes `--<key> <value>`, and `--json` is appended so the reply is
/// enveloped even when `PRAXIS_AGENT_INVOCATION` is unset.
pub(crate) fn render_cli_command(
    tool: &str,
    args: &serde_json::Map<String, serde_json::Value>,
) -> String {
    let mut cmd = tool.to_string();
    if let Some(id) = args.get("id") {
        cmd.push(' ');
        cmd.push_str(&shell_quote_value(id));
    }
    for (key, value) in args {
        if key == "id" || key == "json" {
            continue;
        }
        cmd.push_str(" --");
        cmd.push_str(key);
        cmd.push(' ');
        cmd.push_str(&shell_quote_value(value));
    }
    cmd.push_str(" --json");
    cmd
}

/// The safety-net route for a bead whose continuation could not be
/// completed: `waiting_for` + `assignee user`, with the failure reason in
/// the working notes (praxis-doctrine §10.5 failure posture).
pub(crate) fn safety_net_command(bead_id: &str, reason: &str) -> String {
    format!(
        "praxis update {} --state waiting_for --assignee user --notes {} --json",
        shell_quote(bead_id),
        shell_quote(reason)
    )
}

/// Whether a runtime-driven call advanced the walk.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DriveProgress {
    Progressed,
    /// Identical `(tool, args)` reproduced an identical output — the FSM is
    /// not advancing and re-driving would loop forever.
    Stalled,
}

/// Per-turn pending-continuation state (praxis-doctrine §6.5).
///
/// The loop feeds every tool result through [`observe_tool_result`]; praxis
/// envelopes update the pending continuation, bare outputs are ignored. The
/// loop MUST NOT end the turn while [`has_pending`] — `loop_.rs` enforces
/// that via the mechanical drive path and the turn-termination guard.
pub(crate) struct ContinuationDriver {
    pending: Option<NextAction>,
    drive_calls_used: usize,
    redrives_used: usize,
    /// Last runtime-driven call: (tool, canonical args, output hash).
    last_driven: Option<(String, String, u64)>,
    directive_injected: bool,
}

impl ContinuationDriver {
    pub(crate) fn new() -> Self {
        Self {
            pending: None,
            drive_calls_used: 0,
            redrives_used: 0,
            last_driven: None,
            directive_injected: false,
        }
    }

    pub(crate) fn pending(&self) -> Option<&NextAction> {
        self.pending.as_ref()
    }

    pub(crate) fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Observe a model-initiated tool result.
    ///
    /// An envelope carrying a non-null `next_action` always becomes the new
    /// pending continuation (the FSM re-emitted from authoritative state —
    /// last writer wins). A terminal envelope (`next_action: null`) only
    /// clears the pending state when it comes from the continuation's own
    /// target: a side read like `praxis show --json` returns `null` and
    /// must not clear a pending continuation it has nothing to do with.
    pub(crate) fn observe_tool_result(
        &mut self,
        tool_name: &str,
        args: &serde_json::Value,
        output: &str,
        success: bool,
    ) {
        if !success {
            return;
        }
        let next_action = match parse_envelope_next_action(output) {
            EnvelopeNextAction::NotAnEnvelope => return,
            EnvelopeNextAction::Terminal => None,
            EnvelopeNextAction::Pending(action) => Some(action),
        };
        let matches_pending_target = match &self.pending {
            None => true,
            Some(NextAction::Call { tool, .. }) => call_matches_target(tool_name, args, tool),
            Some(NextAction::AgentWorkThenCall { then, .. }) => {
                call_matches_target(tool_name, args, &then.tool)
            }
        };
        if matches_pending_target || next_action.is_some() {
            self.set_pending(next_action);
        }
    }

    /// Observe the result of a call the runtime itself drove. The driven
    /// call is by construction the pending target, so its envelope always
    /// replaces the pending state. A successful non-envelope output is
    /// terminal (nothing further to drive).
    pub(crate) fn observe_driven_result(&mut self, output: &str, success: bool) {
        if !success {
            // The failure is surfaced by the loop; the continuation is
            // cleared there so the model can recover explicitly.
            return;
        }
        let next_action = match parse_envelope_next_action(output) {
            EnvelopeNextAction::NotAnEnvelope => {
                ::zeroclaw_log::record!(
                    WARN,
                    ::zeroclaw_log::Event::new(module_path!(), ::zeroclaw_log::Action::Note)
                        .with_outcome(::zeroclaw_log::EventOutcome::Unknown),
                    "runtime-driven continuation call returned a non-envelope output; \
                     treating as terminal"
                );
                None
            }
            EnvelopeNextAction::Terminal => None,
            EnvelopeNextAction::Pending(action) => Some(action),
        };
        self.set_pending(next_action);
    }

    fn set_pending(&mut self, next_action: Option<NextAction>) {
        // A re-emission of the same agent_work continuation (the model
        // re-ran `praxis execute` on an unchanged ready-set) keeps the
        // redrive counter and directive state — resetting them would let a
        // stalling model dodge the retry cap forever.
        let same_agent_work = matches!(
            (&self.pending, &next_action),
            (
                Some(NextAction::AgentWorkThenCall { factory_id: a, .. }),
                Some(NextAction::AgentWorkThenCall { factory_id: b, .. }),
            ) if a == b
        );
        if !same_agent_work {
            self.redrives_used = 0;
            self.directive_injected = false;
        }
        self.pending = next_action;
    }

    /// Clear the pending continuation after it has been explicitly
    /// surfaced/routed (never call this silently).
    pub(crate) fn clear_pending(&mut self) {
        self.pending = None;
    }

    pub(crate) fn drive_budget_exhausted(&self) -> bool {
        self.drive_calls_used >= MAX_AUTO_DRIVE_CALLS
    }

    /// Record a runtime-driven call for budget + stall accounting.
    pub(crate) fn register_driven(
        &mut self,
        tool: &str,
        args: &serde_json::Map<String, serde_json::Value>,
        output: &str,
    ) -> DriveProgress {
        self.drive_calls_used += 1;
        let canonical_args = serde_json::Value::Object(args.clone()).to_string();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        output.hash(&mut hasher);
        let output_hash = hasher.finish();
        let stalled = self
            .last_driven
            .as_ref()
            .is_some_and(|(t, a, h)| t == tool && *a == canonical_args && *h == output_hash);
        self.last_driven = Some((tool.to_string(), canonical_args, output_hash));
        if stalled {
            DriveProgress::Stalled
        } else {
            DriveProgress::Progressed
        }
    }

    /// Account one forcing re-drive for the pending `agent_work_then_call`.
    /// Returns false once the retry cap is exhausted.
    pub(crate) fn try_redrive(&mut self) -> bool {
        if self.redrives_used >= MAX_AGENT_WORK_REDRIVES {
            return false;
        }
        self.redrives_used += 1;
        true
    }

    /// The bead id the failure safety net should route, taken from the
    /// pending continuation's `then.args_template.id` (the bead short id in
    /// the §10.5 emitters).
    pub(crate) fn pending_bead_id(&self) -> Option<String> {
        match &self.pending {
            Some(NextAction::AgentWorkThenCall { then, .. }) => then
                .args_template
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            _ => None,
        }
    }

    /// One-line description of the pending continuation for notices/traces.
    pub(crate) fn pending_summary(&self) -> String {
        match &self.pending {
            None => "none".to_string(),
            Some(NextAction::Call { tool, .. }) => format!("call {tool}"),
            Some(NextAction::AgentWorkThenCall {
                factory_id, then, ..
            }) => {
                format!("agent work for {factory_id}, then {}", then.tool)
            }
        }
    }

    /// The forcing directive for the pending `agent_work_then_call`,
    /// returned at most once per distinct continuation (the first
    /// injection; stop-attempt re-drives build their own copy via
    /// [`forcing_directive`]).
    pub(crate) fn directive_to_inject(&mut self) -> Option<String> {
        if self.directive_injected {
            return None;
        }
        let directive = self.forcing_directive()?;
        self.directive_injected = true;
        Some(directive)
    }

    /// Build the forcing directive for the pending `agent_work_then_call`.
    pub(crate) fn forcing_directive(&self) -> Option<String> {
        let Some(NextAction::AgentWorkThenCall {
            agent_hint,
            output_contract,
            then,
            ..
        }) = &self.pending
        else {
            return None;
        };
        let contract = serde_json::to_string(output_contract).unwrap_or_else(|_| "{}".to_string());
        let then_invocation = if then.tool.contains(' ') {
            format!("`{}`", render_cli_command(&then.tool, &then.args_template))
        } else {
            format!(
                "the `{}` tool with arguments {}",
                then.tool,
                serde_json::Value::Object(then.args_template.clone())
            )
        };
        Some(format!(
            "[NextAction continuation — MANDATORY] A praxis continuation is pending; \
             this turn cannot end until it is completed.\n\
             1. Do the work now, in this turn, using your tools (delegate if appropriate): \
             {agent_hint}\n\
             2. The work must produce a JSON output matching this contract: {contract}\n\
             3. Then invoke {then_invocation} — replace any `<…>` placeholder values with \
             the actual work output and keep all other arguments exactly as given.\n\
             Do NOT announce completion, summarize, or reply in chat before that call has \
             been made. Ending the turn without it abandons the execution mid-walk."
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(next_action: serde_json::Value) -> String {
        serde_json::json!({"data": {"ok": true}, "next_action": next_action}).to_string()
    }

    fn call_action(tool: &str, args: serde_json::Value) -> serde_json::Value {
        serde_json::json!({"kind": "call", "tool": tool, "args": args})
    }

    fn agent_work_action(factory_id: &str) -> serde_json::Value {
        serde_json::json!({
            "kind": "agent_work_then_call",
            "factory_id": factory_id,
            "agent_hint": "summarize the emails",
            "output_contract": {"summary": "<value>"},
            "then": {
                "tool": "praxis update",
                "args_template": {
                    "id": "zc-x1",
                    "state": "done",
                    "output": "<json-object matching output_contract>"
                }
            }
        })
    }

    #[test]
    fn bare_output_is_not_an_envelope() {
        assert_eq!(
            parse_envelope_next_action("plain text output"),
            EnvelopeNextAction::NotAnEnvelope
        );
        assert_eq!(
            parse_envelope_next_action(r#"{"ready": []}"#),
            EnvelopeNextAction::NotAnEnvelope
        );
        // JSON with only one of the two envelope keys is not an envelope.
        assert_eq!(
            parse_envelope_next_action(r#"{"data": {"x": 1}}"#),
            EnvelopeNextAction::NotAnEnvelope
        );
    }

    #[test]
    fn terminal_envelope_parses_to_terminal() {
        let out = envelope(serde_json::Value::Null);
        assert_eq!(
            parse_envelope_next_action(&out),
            EnvelopeNextAction::Terminal
        );
    }

    #[test]
    fn call_envelope_parses() {
        let out = envelope(call_action(
            "praxis verify",
            serde_json::json!({"execution": "e1"}),
        ));
        match parse_envelope_next_action(&out) {
            EnvelopeNextAction::Pending(NextAction::Call { tool, args }) => {
                assert_eq!(tool, "praxis verify");
                assert_eq!(args["execution"], "e1");
            }
            other => panic!("expected pending call, got {other:?}"),
        }
    }

    #[test]
    fn agent_work_envelope_parses() {
        let out = envelope(agent_work_action("f1"));
        match parse_envelope_next_action(&out) {
            EnvelopeNextAction::Pending(NextAction::AgentWorkThenCall {
                factory_id, then, ..
            }) => {
                assert_eq!(factory_id, "f1");
                assert_eq!(then.tool, "praxis update");
                assert_eq!(then.args_template["id"], "zc-x1");
            }
            other => panic!("expected pending agent_work_then_call, got {other:?}"),
        }
    }

    #[test]
    fn envelope_amid_noise_lines_is_found() {
        let out = format!("Synced 3 beads\n{}\n", envelope(serde_json::Value::Null));
        assert_eq!(
            parse_envelope_next_action(&out),
            EnvelopeNextAction::Terminal
        );
    }

    #[test]
    fn unknown_kind_is_terminal_not_wedged() {
        let out = envelope(serde_json::json!({"kind": "teleport", "to": "mars"}));
        assert_eq!(
            parse_envelope_next_action(&out),
            EnvelopeNextAction::Terminal
        );
    }

    #[test]
    fn shell_command_matches_target_tokens() {
        let args = serde_json::json!({"command": "praxis update zc-x1 --state done --json"});
        assert!(call_matches_target("shell", &args, "praxis update"));
        assert!(!call_matches_target("shell", &args, "praxis execute"));
        // Tolerates prefixes (env vars, cd && …).
        let args = serde_json::json!({"command": "cd /repo && praxis verify --execution e1"});
        assert!(call_matches_target("shell", &args, "praxis verify"));
        // Exact tool-name match for non-shell tools.
        assert!(call_matches_target(
            "praxis execute",
            &serde_json::Value::Null,
            "praxis execute"
        ));
        assert!(!call_matches_target(
            "echo",
            &serde_json::Value::Null,
            "praxis update"
        ));
    }

    #[test]
    fn render_cli_command_maps_id_positionally_and_flags() {
        let args = serde_json::from_value::<serde_json::Map<_, _>>(serde_json::json!({
            "id": "zc-x1",
            "state": "done",
            "output": "{\"summary\": \"hi\"}"
        }))
        .unwrap();
        let cmd = render_cli_command("praxis update", &args);
        assert!(cmd.starts_with("praxis update zc-x1 "), "{cmd}");
        assert!(cmd.contains("--state done"), "{cmd}");
        assert!(cmd.contains(r#"--output '{"summary": "hi"}'"#), "{cmd}");
        assert!(cmd.ends_with("--json"), "{cmd}");

        let args =
            serde_json::from_value::<serde_json::Map<_, _>>(serde_json::json!({"execution": "e1"}))
                .unwrap();
        assert_eq!(
            render_cli_command("praxis execute", &args),
            "praxis execute --execution e1 --json"
        );
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("plain-value"), "plain-value");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn observe_sets_and_clears_pending() {
        let mut driver = ContinuationDriver::new();
        let out = envelope(call_action(
            "praxis execute",
            serde_json::json!({"execution": "e1"}),
        ));
        driver.observe_tool_result(
            "shell",
            &serde_json::json!({"command": "praxis spec activate-execution t1 --json"}),
            &out,
            true,
        );
        assert!(driver.has_pending());

        // Failed outcomes never update the pending state.
        driver.observe_tool_result(
            "shell",
            &serde_json::Value::Null,
            &envelope(serde_json::Value::Null),
            false,
        );
        assert!(driver.has_pending());
    }

    #[test]
    fn side_reads_do_not_clear_pending_agent_work() {
        let mut driver = ContinuationDriver::new();
        driver.observe_tool_result(
            "shell",
            &serde_json::json!({"command": "praxis execute --execution e1 --json"}),
            &envelope(agent_work_action("f1")),
            true,
        );
        assert!(driver.has_pending());

        // A `praxis show` mid-work returns a terminal envelope; it must not
        // clear the pending continuation.
        driver.observe_tool_result(
            "shell",
            &serde_json::json!({"command": "praxis show zc-x1 --json"}),
            &envelope(serde_json::Value::Null),
            true,
        );
        assert!(driver.has_pending());

        // The then.tool call's envelope DOES update it.
        driver.observe_tool_result(
            "shell",
            &serde_json::json!({"command": "praxis update zc-x1 --state done --output '{}' --json"}),
            &envelope(call_action("praxis execute", serde_json::json!({"execution": "e1"}))),
            true,
        );
        match driver.pending() {
            Some(NextAction::Call { tool, .. }) => assert_eq!(tool, "praxis execute"),
            other => panic!("expected call pending, got {other:?}"),
        }
    }

    #[test]
    fn redrive_cap_is_bounded_and_survives_reemission() {
        let mut driver = ContinuationDriver::new();
        driver.observe_tool_result(
            "shell",
            &serde_json::json!({"command": "praxis execute --execution e1 --json"}),
            &envelope(agent_work_action("f1")),
            true,
        );
        for _ in 0..MAX_AGENT_WORK_REDRIVES {
            assert!(driver.try_redrive());
        }
        assert!(!driver.try_redrive());

        // Re-emitting the SAME continuation must not reset the counter.
        driver.observe_tool_result(
            "shell",
            &serde_json::json!({"command": "praxis execute --execution e1 --json"}),
            &envelope(agent_work_action("f1")),
            true,
        );
        assert!(!driver.try_redrive());

        // A different bead's continuation gets a fresh budget.
        driver.observe_tool_result(
            "shell",
            &serde_json::json!({"command": "praxis execute --execution e1 --json"}),
            &envelope(agent_work_action("f2")),
            true,
        );
        assert!(driver.try_redrive());
    }

    #[test]
    fn driven_stall_detection_requires_identical_args_and_output() {
        let mut driver = ContinuationDriver::new();
        let args =
            serde_json::from_value::<serde_json::Map<_, _>>(serde_json::json!({"execution": "e1"}))
                .unwrap();
        assert_eq!(
            driver.register_driven("praxis execute", &args, "ready: [a]"),
            DriveProgress::Progressed
        );
        // Same call, different output → the walk advanced.
        assert_eq!(
            driver.register_driven("praxis execute", &args, "ready: [b]"),
            DriveProgress::Progressed
        );
        // Same call, same output → no state change, stall.
        assert_eq!(
            driver.register_driven("praxis execute", &args, "ready: [b]"),
            DriveProgress::Stalled
        );
    }

    #[test]
    fn drive_budget_exhausts() {
        let mut driver = ContinuationDriver::new();
        let args = serde_json::Map::new();
        for i in 0..MAX_AUTO_DRIVE_CALLS {
            assert!(!driver.drive_budget_exhausted());
            driver.register_driven("praxis execute", &args, &format!("out {i}"));
        }
        assert!(driver.drive_budget_exhausted());
    }

    #[test]
    fn directive_injected_once_per_continuation() {
        let mut driver = ContinuationDriver::new();
        driver.observe_tool_result(
            "shell",
            &serde_json::json!({"command": "praxis execute --execution e1 --json"}),
            &envelope(agent_work_action("f1")),
            true,
        );
        let directive = driver.directive_to_inject().expect("first injection");
        assert!(directive.contains("summarize the emails"));
        assert!(directive.contains("praxis update zc-x1"));
        assert!(driver.directive_to_inject().is_none());
        assert_eq!(driver.pending_bead_id().as_deref(), Some("zc-x1"));
    }

    #[test]
    fn safety_net_command_shape() {
        let cmd = safety_net_command("zc-x1", "agent failed to complete the work");
        assert_eq!(
            cmd,
            "praxis update zc-x1 --state waiting_for --assignee user \
             --notes 'agent failed to complete the work' --json"
        );
    }
}
