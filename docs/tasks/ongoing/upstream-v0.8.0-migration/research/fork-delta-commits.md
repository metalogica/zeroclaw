# Fork Delta — raw commit ledger (`main..HEAD`)

> **Purpose:** the complete, un-editorialized inventory of every commit our sovereign fork
> carries over `v0.6.9`. This is the **primary source data** for re-deriving the change
> clustering and per-theme disposition. Do **not** trust the `migration-playbook.md` clustering
> — re-derive from this ledger + `git show <sha>` and cross-check against the playbook.
>
> **Range:** `main..HEAD` on branch `0.6.9-alpha-p10.7` (merge-base `1a61ea731` = `v0.6.9`).
> **Count:** 84 commits. **Net:** `77 files changed, +10,075 / −1,365`.
> Generated 2026-07-09. Regenerate with the commands in the appendix.

## Churn hotspots (files by number of commits that touch them)

| Commits | File (v0.6.9 path) |
|--------:|--------------------|
| 20 | `README.md` |
| 17 | `src/agent/loop_.rs` |
| 17 | `src/agent/agent.rs` |
| 14 | `src/gateway/ws.rs` |
| 12 | `src/gateway/mod.rs` |
| 11 | `src/providers/openrouter.rs` |
| 11 | `Dockerfile` |
| 9 | `src/observability/mod.rs` |
| 9 | `src/channels/mod.rs` |
| 8 | `src/observability/otel.rs` |
| 7 | `docs/tasks/ongoing/agentic-observability/connected-tracing-spec.md` |
| 6 | `dev/hotswap/hotswap.sh` |
| 5 | `src/config/schema.rs` |
| 4 | `src/agent/tool_execution.rs` |
| 3 | `src/providers/reliable.rs`, `src/observability/traits.rs`, `src/observability/identity.rs` |
| 2 | `src/tools/delegate.rs`, `src/providers/{traits,router,compatible,anthropic}.rs`, `src/observability/{runtime_trace,active}.rs`, `Dockerfile.debian`, `dev/hotswap/Dockerfile.builder` |

**Net-new files introduced by the fork** (do not exist at `v0.6.9`): `src/agent/continuation.rs`,
`src/observability/active.rs`, `src/observability/identity.rs`, `src/observability/traits.rs`,
`src/util.rs`, `src/gateway/sse.rs`, `dev/hotswap/hotswap.sh`, `dev/hotswap/Dockerfile.builder`,
`Justfile`, `docs/tasks/ongoing/agentic-observability/connected-tracing-spec.md`, plus the
`.claude/` + `.tbd/` + `AGENTS.md` tooling scaffold. (Confirm with `git show <sha>` per commit.)

---

## Full ledger (chronological, `sha · date · subject` + files with `+add/−del`)

Merge commits (`892bec179`, `b54052c0e`, `bcf111e8b`) carry no numstat — they merge already-listed work.

```
f92efae55  2026-03-29  Added preshared preshared token functionality and wolfi
  +51  −0   README.md
  +6   −0   src/config/schema.rs
  +51  −1   src/gateway/mod.rs

a0d1a8fbd  2026-04-10  feat(personality): add ZEROCLAW_SYSTEM_DIR for split-mount filesystem security
  +144 −1   src/agent/personality.rs

28b11deb7  2026-04-15  update docs and docker file for alpha p5
  +5   −2   Dockerfile
  +120 −655 README.md

28ed745ea  2026-04-15  feat(provider): add modalities support and image generation response handling for OpenRouter
  +5   −0   src/config/schema.rs
  +2   −0   src/doctor/mod.rs
  +79  −4   src/providers/mod.rs
  +2   −0   src/providers/openai_codex.rs
  +591 −10  src/providers/openrouter.rs
  +26  −1   src/tools/delegate.rs
  +1   −0   src/tools/mod.rs
  +1   −0   src/tools/model_routing_config.rs
  +2   −0   src/tools/swarm.rs
  +2   −0   tests/live/openai_codex_vision_e2e.rs

c276ffe6a  2026-04-15  fix(daemon): include webhook channel in supervised channels detection
  +100 −4   src/daemon/mod.rs

17345a002  2026-04-15  feat(multimodal): add [AUDIO:] marker support for audio input to LLM providers
  +23  −0   src/config/schema.rs
  +385 −27  src/multimodal.rs
  +89  −2   src/providers/openrouter.rs

93269b493  2026-04-15  update docs for p6
  +1   −0   README.md

68bcc3d4a  2026-04-16  fix(provider): extract generated images from OpenRouter images field, not reasoning_details
  +125 −30  src/providers/openrouter.rs

7acee7e90  2026-04-29  update readme
  +4   −1   README.md

b4bba8d97  2026-04-29  feat(docker): bundle Stripe link-cli in Wolfi runtime for alpha p7
  +5   −1   Dockerfile

e1bb3452d  2026-04-29  feat(docker): bundle tbd CLI in Wolfi runtime for alpha p8
  +4   −1   Dockerfile

b46a7e01e  2026-04-29  feat(provider): send pod user id as `user` on OpenRouter requests
  +136 −0   src/providers/openrouter.rs

0d8ae2916  2026-04-29  update readme
  +3   −2   README.md

026694cd8  2026-04-29  feat(docker): add git to Wolfi runtime for alpha p9
  +1   −1   Dockerfile

6b0430efa  2026-04-29  update readme
  +3   −2   README.md

d0e810339  2026-04-29  update readme
  +1   −1   README.md

ceadd3143  2026-04-30  fix(provider): read CLAW_USER_ID env var, not POD_NAMESPACE
  +36  −48  src/providers/openrouter.rs

d6ab72723  2026-04-30  update readme
  +1   −1   README.md

1aac33081  2026-05-09  feat(docker): install @soulbound-labs/praxis in Wolfi runtime; drop link-cli + get-tbd
  +20  −8   Dockerfile
  +24  −2   README.md

f95420a99  2026-05-13  final p10.1 commit
  +1   −1   Dockerfile
  +1   −1   Dockerfile.debian
  +4   −3   README.md

f9443fb9c  2026-05-15  update dokcerfile nad readme
  +1   −1   Dockerfile
  +2   −0   README.md

8ee05706f  2026-05-15  feat(gateway): bind WS messages to clawcraft threadId (B2)
  +43  −4   src/agent/agent.rs
  +217 −2   src/gateway/ws.rs

def049394  2026-05-17  update readme
  +1   −1   Dockerfile
  +7   −5   README.md

03c739b41  2026-05-21  feat(gateway,agent): add session_key_for_thread() + Agent::replace_history() (Step 1)
  +120 −0   src/agent/agent.rs
  +8   −0   src/gateway/ws.rs

d4cf6e6b2  2026-05-21  add TBD
  +2   −0   .claude/.gitignore
  +15  −0   .claude/hooks/tbd-closing-reminder.sh
  +88  −0   .claude/scripts/ensure-gh-cli.sh
  +77  −0   .claude/scripts/tbd-session.sh
  +47  −0   .claude/settings.json
  +259 −0   .claude/skills/tbd/SKILL.md
  +2   −0   .tbd/.gitattributes
  +21  −0   .tbd/.gitignore
  +93  −0   .tbd/config.yml
  +251 −0   AGENTS.md

d7fac90d3  2026-05-21  pdate readme
  +1   −0   README.md

b54bcc5de  2026-05-22  feat(gateway): derive per-message session key from thread_id (Step 2)
  +48  −12  src/gateway/ws.rs

8ddbda837  2026-05-22  feat(gateway): per-thread hydration on first message-with-threadId (Step 3)
  +126 −17  src/gateway/ws.rs

bb99e49cf  2026-05-22  feat(gateway): socket-local thread-switch detection + no-threadId continuity (Step 4)
  +139 −26  src/gateway/ws.rs

098fb4932  2026-05-27  update praxis versioni ndockerfile
  +1   −1   README.md

de6a39b9d  2026-05-27  update praxisin readme.
  +1   −1   README.md

3d3d11500  2026-05-27  bump tag
  +1   −1   README.md

d8a78c021  2026-05-27  add ;
  +3   −3   README.md

5174d738e  2026-05-27  add b identifier
  +1   −1   README.md

ce0ea2a69  2026-05-30  update praxis version in docker image
  +1   −1   Dockerfile

02474cc03  2026-05-31  feat(agent): emit Thinking event for reasoning_content in non-stream fallback
  +109 −7   src/agent/agent.rs

c52786374  2026-06-01  docs(observability): spec for connected agentic tracing (SPEC 1)
  +271 −0   docs/tasks/ongoing/agentic-observability/connected-tracing-spec.md

99d51c41d  2026-06-01  feat(observability): Phase 1 — recursive Span interface + OtelSpan
  +1   −1   src/observability/mod.rs
  +150 −132 src/observability/otel.rs
  +94  −0   src/observability/traits.rs

2110d18f8  2026-06-01  feat(observability): Phase 2 core — ambient span + connect 3 ingress owners
  +111 −69  src/agent/loop_.rs
  +32  −7   src/agent/tool_execution.rs
  +16  −3   src/channels/mod.rs
  +40  −0   src/observability/active.rs
  +2   −0   src/observability/mod.rs

197515895  2026-06-01  docs(observability): record Phase 2 revisions + two-engine finding
  +35  −0   docs/tasks/ongoing/agentic-observability/connected-tracing-spec.md

bb1b3f179  2026-06-01  feat(observability): Phase 2 complete — instrument Agent engine + wire WS/webhook
  +19  −12  docs/tasks/ongoing/agentic-observability/connected-tracing-spec.md
  +48  −4   src/agent/agent.rs
  +20  −3   src/gateway/mod.rs
  +17  −7   src/gateway/ws.rs

07002a4fd  2026-06-01  feat(observability): SPEC 1 enrichment — reasoning + Composio metadata on spans
  +19  −4   docs/tasks/ongoing/agentic-observability/connected-tracing-spec.md
  +22  −0   src/agent/agent.rs
  +18  −0   src/agent/loop_.rs
  +10  −0   src/agent/tool_execution.rs

f8f267879  2026-06-01  fix(providers/openrouter): accept `reasoning` alias for reasoning_content
  +32  −3   src/providers/openrouter.rs

9969a35fd  2026-06-01  feat(agent): emit Thinking event on tool-call path
  +123 −0   src/agent/agent.rs

229ce9124  2026-06-01  feat(providers/openrouter): SSE streaming with incremental reasoning + tool calls
  +419 −1   src/providers/openrouter.rs

54f466c5d  2026-06-01  fix(providers/openrouter): declare streaming capabilities
  +15  −0   src/providers/openrouter.rs

7e9d6fac8  2026-06-02  build(docker): compile zeroclaw with observability-otel feature
  +1   −1   Dockerfile
  +1   −1   Dockerfile.debian

7a47624b7  2026-06-02  feat(observability): OTLP exporter auth headers + single shared instance
  +10  −0   src/config/schema.rs
  +7   −3   src/observability/mod.rs
  +154 −50  src/observability/otel.rs
  +1   −0   src/observability/runtime_trace.rs

c51c166c9  2026-06-02  fix(observability): emit agent.activation spans for all gateway/WS turns
  +52  −0   src/channels/mod.rs
  +69  −0   src/gateway/sse.rs
  +19  −8   src/gateway/ws.rs
  +55  −1   src/observability/multi.rs

52b5921e3  2026-06-02  dev(tooling): claw-hotswap for fast binary iteration
  +9   −0   Justfile
  +33  −0   dev/hotswap/Dockerfile.builder
  +87  −0   dev/hotswap/hotswap.sh

3fd09e639  2026-06-02  feat(observability): emit deployment.environment + capture streaming reasoning
  +15  −4   src/agent/agent.rs
  +63  −3   src/agent/loop_.rs
  +8   −0   src/config/schema.rs
  +1   −0   src/observability/mod.rs
  +67  −13  src/observability/otel.rs
  +1   −0   src/observability/runtime_trace.rs

c00663a69  2026-06-03  fix(observability): tag channel + thread_id on web/webhook activation roots
  +13  −3   src/gateway/mod.rs
  +6   −0   src/gateway/ws.rs

cd4ddeb24  2026-06-03  feat(observability): record tool.output / tool.error body on tool.call spans
  +11  −0   src/agent/agent.rs
  +25  −4   src/agent/tool_execution.rs

827f7c0b1  2026-06-03  fix(observability): surface deployment.environment on span + flush on CLI exit
  +7   −0   src/agent/loop_.rs
  +34  −5   src/observability/otel.rs

9fd8c0e3c  2026-06-03  feat(observability): emit gen_ai.prompt + gen_ai.completion on llm.call
  +7   −0   docs/tasks/ongoing/agentic-observability/connected-tracing-spec.md
  +44  −2   src/agent/agent.rs
  +29  −0   src/agent/loop_.rs
  +21  −0   src/gateway/mod.rs

434ce0294  2026-06-03  feat(observability): user.id on user-facing activation roots (W5)
  +3   −0   src/channels/mod.rs
  +7   −0   src/gateway/ws.rs
  +77  −0   src/observability/identity.rs
  +2   −0   src/observability/mod.rs
  +1   −62  src/providers/openrouter.rs

839630740  2026-06-03  perf(hotswap): mold linker + skip redundant builder rebuild
  +6   −4   dev/hotswap/Dockerfile.builder
  +12  −5   dev/hotswap/hotswap.sh

ad73cc01f  2026-06-03  fix(observability): drain OTLP exporter on CLI exit via terminal shutdown
  +8   −6   src/agent/loop_.rs
  +12  −2   src/main.rs
  +16  −0   src/observability/mod.rs
  +23  −0   src/observability/otel.rs

7ca7e5404  2026-06-03  fix(hotswap): correct compile log to say mold, not lld
  +1   −1   dev/hotswap/hotswap.sh

dbee54f5b  2026-06-03  udpate readme
  +11  −3   README.md

7ce12af71  2026-06-03  fix(observability): set Laminar Root input/output on activation root
  +18  −7   docs/tasks/ongoing/agentic-observability/connected-tracing-spec.md
  +21  −2   src/agent/loop_.rs
  +21  −1   src/gateway/mod.rs
  +27  −0   src/gateway/ws.rs

658fcf95e  2026-06-03  fix(observability): populate Laminar typed user_id column via association-property key (W5-A)
  +1   −3   src/agent/loop_.rs
  +1   −3   src/channels/mod.rs
  +1   −3   src/gateway/mod.rs
  +2   −7   src/gateway/ws.rs
  +64  −0   src/observability/identity.rs
  +1   −1   src/observability/mod.rs

c15c1f1dd  2026-06-03  feat(observability): mirror lmnr.span.input/output onto llm.call spans (W2)
  +28  −24  src/agent/agent.rs
  +11  −11  src/agent/loop_.rs
  +23  −9   src/gateway/mod.rs

948328a82  2026-06-03  feat(observability): set native OTel Status on the root agent.activation span
  +10  −0   docs/tasks/ongoing/agentic-observability/connected-tracing-spec.md
  +8   −1   src/agent/agent.rs
  +7   −0   src/agent/loop_.rs
  +6   −0   src/channels/mod.rs
  +4   −0   src/gateway/mod.rs
  +6   −1   src/gateway/ws.rs

242b6fa97  2026-06-03  feat(observability): emit tool.input (scrubbed args) on tool.call spans
  +10  −0   src/agent/agent.rs
  +10  −0   src/agent/tool_execution.rs

c034820c2  2026-06-03  feat(observability): wrap outbound delivery POST in a delivery span
  +33  −25  src/channels/mod.rs
  +36  −1   src/channels/webhook.rs
  +5   −0   src/gateway/mod.rs

16722ec21  2026-06-03  feat(observability): emit gen_ai.prompt/completion (+lmnr.span.input/output) on every llm.call
  +42  −16  src/agent/agent.rs
  +83  −8   src/agent/loop_.rs

6c887fa5e  2026-06-03  feat(observability): emit retry/fallback/exception as OTel span events
  +5   −49  src/agent/loop_.rs
  +8   −0   src/observability/otel.rs
  +58  −0   src/observability/traits.rs
  +83  −16  src/providers/reliable.rs
  +58  −0   src/util.rs

ef85f4c23  2026-06-03  feat(hotswap): bake swapped binary into image so recreate survives
  +54  −2   dev/hotswap/hotswap.sh

245e7277c  2026-06-03  feat(observability): propagate deployment.environment to every span
  +64  −3   src/observability/otel.rs

892bec179  2026-06-03  merge(observability): root agent.activation span status (zc-sdjr)    [MERGE — no numstat]
b54052c0e  2026-06-03  merge(observability): outbound delivery POST span (zc-hf7r)          [MERGE — no numstat]
bcf111e8b  2026-06-03  merge(observability): retry/fallback/exception span events (zc-jz9y) [MERGE — no numstat]

26e97d230  2026-06-04  feat(observability): root lmnr.span.input/output on CLI + native-channel activation roots
  +19  −0   src/agent/loop_.rs
  +23  −2   src/channels/mod.rs

411c27945  2026-06-04  feat(observability): emit gen_ai.response.finish_reason + tool_call_count on llm.call spans
  +7   −0   benches/agent_benchmarks.rs
  +57  −3   src/agent/agent.rs
  +4   −0   src/agent/dispatcher.rs
  +42  −4   src/agent/loop_.rs
  +11  −0   src/agent/tests.rs
  +16  −0   src/gateway/mod.rs
  +17  −3   src/providers/anthropic.rs
  +1   −0   src/providers/azure_openai.rs
  +1   −0   src/providers/bedrock.rs
  +1   −0   src/providers/claude_code.rs
  +54  −7   src/providers/compatible.rs
  +1   −0   src/providers/copilot.rs
  +1   −0   src/providers/gemini_cli.rs
  +1   −0   src/providers/kilocli.rs
  +3   −0   src/providers/ollama.rs
  +1   −0   src/providers/openai.rs
  +70  −1   src/providers/openrouter.rs
  +6   −2   src/providers/reliable.rs
  +4   −2   src/providers/router.rs
  +21  −4   src/providers/traits.rs
  +5   −0   src/tools/delegate.rs
  +5   −0   src/tools/file_read.rs
  +4   −0   tests/component/provider_schema.rs
  +1   −0   tests/integration/agent.rs
  +2   −0   tests/integration/agent_robustness.rs
  +2   −0   tests/support/helpers.rs
  +4   −0   tests/support/mock_provider.rs

0c1dfd419  2026-06-04  feat(observability): dual-emit session_id + tags Laminar association properties
  +7   −0   CLAUDE.md
  +1   −1   src/channels/mod.rs
  +1   −4   src/gateway/mod.rs
  +2   −6   src/gateway/ws.rs
  +51  −2   src/observability/identity.rs
  +1   −1   src/observability/mod.rs
  +43  −1   src/observability/otel.rs
  +7   −0   src/observability/traits.rs

53c23b2e7  2026-06-04  feat(observability): stamp queryable turn-outcome on activation root (zc-ug3w)
  +13  −1   src/agent/agent.rs
  +187 −1   src/agent/loop_.rs
  +15  −0   src/channels/mod.rs
  +28  −0   src/gateway/mod.rs
  +16  −0   src/gateway/ws.rs
  +20  −1   src/observability/active.rs
  +1   −1   src/observability/mod.rs

074b84201  2026-06-09  feat(observability): emit usage + gen_ai.system on streaming llm.call (zc-haim)
  +39  −2   src/agent/agent.rs
  +14  −2   src/agent/loop_.rs
  +3   −0   src/providers/anthropic.rs
  +2   −0   src/providers/compatible.rs
  +42  −0   src/providers/openrouter.rs
  +1   −0   src/providers/reliable.rs
  +1   −0   src/providers/router.rs
  +11  −1   src/providers/traits.rs

6b7f72bff  2026-06-09  docs(readme): update alpha-p10.7 changelog + bump hotswap TAG
  +2   −2   README.md

843e34b55  2026-06-10  perf(hotswap): extract binary via stdout stream, not 'install' through bind mount (zc-i4gm)
  +15  −1   dev/hotswap/hotswap.sh

9b3cad4cb  2026-06-10  feat(agent): runtime auto-drive of praxis NextAction continuations (rnk-h6g3)
  +751 −0   src/agent/continuation.rs
  +805 −7   src/agent/loop_.rs
  +1   −0   src/agent/mod.rs

64bec054c  2026-06-10  perf(hotswap): bake via docker commit, not docker build context (zc-pije)
  +33  −20  dev/hotswap/hotswap.sh

50564e311  2026-06-10  image: bump praxis to 0.10.0
  +1   −1   Dockerfile

9970e5536  2026-06-11  feat(agent): port praxis NextAction continuation guard to turn/turn_streamed (zc-g50j)
  +921 −5   src/agent/agent.rs
```

---

## Appendix — regeneration commands

```sh
git rev-list --count main..HEAD                                   # 84
git log --reverse --format='%h%x09%ad%x09%s' --date=short main..HEAD
git log --reverse --format='@@@%h %s' --numstat main..HEAD        # per-commit files
git log main..HEAD --name-only --format='' | grep -v '^$' | sort | uniq -c | sort -rn
git diff --shortstat main..HEAD                                   # 77 files, +10075 −1365
git show <sha>                                                    # full diff for any commit
```
