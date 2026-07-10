#!/usr/bin/env bash
#
# Cold-start parity harness for the clawcraft prod pod on v0.8.0.
#
# Boots the `:dev` image the way GKE boots the prod pod — image env only, a
# clawcraft-rendered config bind-mounted at the resolved config dir, a named
# volume standing in for the PVC at /zeroclaw-data/workspace — and asserts every
# invariant a silently-degraded pod would violate. Spec §4.3 / Phase-4 Step 4.5.
#
# WHY THIS EXISTS: a v0.8.0 pod that boots but 401s every authed call, or lands
# agent state in the 10Mi emptyDir, or re-runs the one-way brain.db migration on
# every boot, still returns 200 on /health. /health is status-code-only, so none
# of those regressions are visible to Kubernetes. This harness is the detector.
#
# Usage:
#   dev/hotswap/verify-coldstart.sh                 # full battery (needs docker)
#   KEEP=1 dev/hotswap/verify-coldstart.sh          # leave the pod + volume up for triage
#   CONFIG_FIXTURE=/path/to/config.toml  dev/hotswap/verify-coldstart.sh
#   IMAGE=clawcraft-claw-runtime:dev     dev/hotswap/verify-coldstart.sh
#
# Env overrides:
#   IMAGE           image to boot                  (default: clawcraft-claw-runtime:dev)
#   CONFIG_FIXTURE  rendered config.toml to mount  (default: this dir's coldstart fixture,
#                                                    or a live `pnpm dev:claw:render` if the
#                                                    clawcraft dev Convex stack is reachable)
#   TOKEN           pre_shared_token in the config (default: matches the fixture)
#   REBUILD=1       rebuild :dev before booting     (docker build --target release)
#   KEEP=1          skip teardown (pod + volume survive for manual inspection)
#
# NOTE: the FULL run needs a live docker daemon + the built :dev image + (for the
# Laminar-adjacent surfaces) the clawcraft dev stack. It is OUT-OF-BAND from the
# per-bead cargo gate and is run during the zc-b78l manual battery. The in-band
# gate is `bash -n` plus a static lint; the assertions below are what the manual
# run executes.
set -euo pipefail

# --- resolved layout (must match the image env + dir-resolution code) ----------
# The image pins ENV ZEROCLAW_DATA_DIR=/zeroclaw-data/workspace and HOME=/zeroclaw-data
# and NEVER sets ZEROCLAW_CONFIG_DIR. With HOME=/zeroclaw-data, the config dir
# resolves to $HOME/.zeroclaw = /zeroclaw-data/.zeroclaw, and the data dir stays
# pinned at /zeroclaw-data/workspace (schema.rs dir-resolution, spec §3.3/§4.3).
DATA_DIR="/zeroclaw-data/workspace"
CONFIG_DIR="/zeroclaw-data/.zeroclaw"
PVC_ROOT="/zeroclaw-data"
BRAIN_DB="$DATA_DIR/memory/brain.db"

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HOTSWAP_DIR="$REPO/dev/hotswap"

IMAGE="${IMAGE:-clawcraft-claw-runtime:dev}"
CONTAINER="zc-coldstart-verify"
VOLUME="zc-coldstart-pvc"
GATEWAY_PORT=42617
WEBHOOK_PORT=42618
# Host ports (avoid clashing with a running dev pod on the same box).
HOST_GATEWAY_PORT="${HOST_GATEWAY_PORT:-52617}"
HOST_WEBHOOK_PORT="${HOST_WEBHOOK_PORT:-52618}"
GW="http://127.0.0.1:${HOST_GATEWAY_PORT}"
WH="http://127.0.0.1:${HOST_WEBHOOK_PORT}"

TOKEN="${TOKEN:-coldstart-test-token-000000000000}"

pass_count=0
fail_count=0

bold() { printf '\033[1m%s\033[0m\n' "$*"; }
ok()   { printf '  \033[32m✓\033[0m %s\n' "$*"; pass_count=$((pass_count + 1)); }
bad()  { printf '  \033[31m✗\033[0m %s\n' "$*"; fail_count=$((fail_count + 1)); }

# Assert an HTTP status. args: <label> <expected> <method> <url> [curl-extra...]
assert_status() {
    local label="$1" expected="$2" method="$3" url="$4"
    shift 4
    local got
    got="$(curl -s -o /dev/null -w '%{http_code}' -X "$method" "$@" "$url" || echo "000")"
    if [ "$got" = "$expected" ]; then
        ok "$label → $got"
    else
        bad "$label → got $got, expected $expected"
    fi
}

# Run a command INSIDE the pod, echoing its stdout.
in_pod() { docker exec "$CONTAINER" "$@"; }

teardown() {
    if [ "${KEEP:-0}" = "1" ]; then
        bold "▶ KEEP=1 — leaving '$CONTAINER' + volume '$VOLUME' up for triage."
        return
    fi
    docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
    docker volume rm "$VOLUME" >/dev/null 2>&1 || true
}

# ==============================================================================
# 0. Preconditions: docker + a config fixture.
# ==============================================================================
bold "▶ cold-start parity harness — image '$IMAGE'"

if ! docker info >/dev/null 2>&1; then
    echo "✗ docker daemon not reachable — this harness is out-of-band (run in the zc-b78l battery)."
    exit 1
fi

if [ "${REBUILD:-0}" = "1" ]; then
    bold "▶ REBUILD=1 — building '$IMAGE' (--target release)…"
    DOCKER_BUILDKIT=1 docker build --target release \
        --secret id=npm_token,env=NPM_TOKEN \
        -t "$IMAGE" "$REPO"
fi

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
    echo "✗ image '$IMAGE' not found. Build it first (REBUILD=1 or 'just claw-hotswap')."
    exit 1
fi

# Resolve the config fixture. Prefer a live clawcraft render (byte-identical to
# prod); fall back to the checked-in fixture that mirrors the golden-test shape.
CONFIG_FIXTURE="${CONFIG_FIXTURE:-}"
if [ -z "$CONFIG_FIXTURE" ]; then
    if [ -f "$HOTSWAP_DIR/coldstart.config.toml" ]; then
        CONFIG_FIXTURE="$HOTSWAP_DIR/coldstart.config.toml"
    else
        echo "✗ no config fixture. Provide CONFIG_FIXTURE=… or add dev/hotswap/coldstart.config.toml"
        echo "  (render one: cd clawcraft && pnpm dev:claw:render, with pre_shared_token = '$TOKEN')."
        exit 1
    fi
fi
bold "  config fixture: $CONFIG_FIXTURE"

# The token MUST match the fixture, or the 200/401 matrix below is meaningless.
if ! grep -q "pre_shared_token = \"$TOKEN\"" "$CONFIG_FIXTURE"; then
    echo "✗ fixture's pre_shared_token does not match TOKEN='$TOKEN'."
    echo "  set TOKEN to the fixture's rendered token so the auth matrix is valid."
    exit 1
fi

trap teardown EXIT
teardown  # clean any prior run

# ==============================================================================
# 1. Boot the pod prod-shaped: image env only, config bind-mount, PVC volume.
#    daemon, no TTY — exactly how GKE launches it. No CLI flags that would
#    re-pin dirs; the whole point is to exercise the image's baked env.
# ==============================================================================
docker volume create "$VOLUME" >/dev/null
bold "▶ booting '$CONTAINER' (daemon, image env only)…"
docker run -d --name "$CONTAINER" \
    -e CLAW_USER_ID="claw-abcdef0123456789abcdef0123456789" \
    -e NPM_TOKEN="" \
    -v "$CONFIG_FIXTURE":"$CONFIG_DIR/config.toml":ro \
    -v "$VOLUME":"$DATA_DIR" \
    -p "$HOST_GATEWAY_PORT":"$GATEWAY_PORT" \
    -p "$HOST_WEBHOOK_PORT":"$WEBHOOK_PORT" \
    "$IMAGE" daemon >/dev/null

# ==============================================================================
# 2. Headless boot — NO pairing code printed. A pairing code in the logs means
#    the pod thinks it is unpaired and is waiting for interactive pairing; prod
#    is headless and must never emit one (the token in config is the pairing).
# ==============================================================================
bold "▶ checking headless boot (no pairing code)…"
sleep 3
boot_logs="$(docker logs "$CONTAINER" 2>&1 || true)"
if printf '%s' "$boot_logs" | grep -qiE 'pairing code|pair this device|enter the code'; then
    bad "pairing code emitted in logs — pod booted UNPAIRED (headless boot broken)"
else
    ok "no pairing code in boot logs (headless)"
fi

# ==============================================================================
# 3. /health 200 within 10s (unauthenticated, status-code-only).
# ==============================================================================
bold "▶ waiting for /health 200 (≤10s)…"
health_ok=0
for _ in $(seq 1 10); do
    code="$(curl -s -o /dev/null -w '%{http_code}' "$GW/health" || echo 000)"
    if [ "$code" = "200" ]; then health_ok=1; break; fi
    sleep 1
done
if [ "$health_ok" = "1" ]; then
    ok "/health → 200 within 10s"
else
    bad "/health did not reach 200 within 10s (last code: ${code:-none})"
    echo "----- boot logs -----"; docker logs "$CONTAINER" 2>&1 | tail -40; echo "---------------------"
    exit 1
fi
# /health must NOT require auth.
assert_status "/health unauthenticated" 200 GET "$GW/health"

# ==============================================================================
# 4. Resolved dirs — the pod resolved config to /zeroclaw-data/.zeroclaw and data
#    to /zeroclaw-data/workspace. If HOME/env drifted, these paths won't exist
#    inside the pod and agent state would silently land elsewhere.
# ==============================================================================
bold "▶ checking resolved dirs…"
if in_pod test -d "$CONFIG_DIR"; then ok "config dir resolved: $CONFIG_DIR"; else bad "config dir MISSING: $CONFIG_DIR"; fi
if in_pod test -d "$DATA_DIR";   then ok "data dir resolved: $DATA_DIR";   else bad "data dir MISSING: $DATA_DIR";   fi

# ==============================================================================
# 5. Auth matrix on /api/status: 200 with token, 401 without.
#    This is the silently-degraded-pod detector (Error-table row 1): if the
#    v0.8.0 schema didn't learn pre_shared_token (FD-04), the pod boots but 401s
#    everything — undetectable via /health.
# ==============================================================================
bold "▶ auth matrix — /api/status…"
assert_status "/api/status WITH token"    200 GET "$GW/api/status" -H "Authorization: Bearer $TOKEN"
assert_status "/api/status WITHOUT token" 401 GET "$GW/api/status"

# ==============================================================================
# 6. /api/chat (FD-05): 200 for {message} AND {message,context} with token;
#    401 without. The context field must be tolerated-and-ignored (a future
#    deny_unknown_fields must not break prod). Registered on the 600s router.
# ==============================================================================
bold "▶ /api/chat matrix…"
assert_status "/api/chat {message} WITH token" 200 POST "$GW/api/chat" \
    -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
    -d '{"message":"coldstart ping"}'
assert_status "/api/chat {message,context} WITH token" 200 POST "$GW/api/chat" \
    -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
    -d '{"message":"coldstart ping","context":"ignored"}'
assert_status "/api/chat WITHOUT token" 401 POST "$GW/api/chat" \
    -H "Content-Type: application/json" -d '{"message":"coldstart ping"}'

# ==============================================================================
# 7. Polysemy guard — 42617 /webhook is the FULL agent loop (body {message}),
#    NOT the 42618 async {sender,content} contract. Posting the 42618 shape at
#    42617 must be REJECTED (400), proving the two webhooks are not conflated.
# ==============================================================================
bold "▶ polysemy guard — 42617 /webhook rejects {sender,content}…"
assert_status "42617 /webhook {sender,content}" 400 POST "$GW/webhook" \
    -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
    -d '{"sender":"someone","content":"hi"}'

# ==============================================================================
# 8. 42618 webhook-only supervision — the pod supervises the 42618 channel even
#    with no other channel enabled (c276ffe6a intent, upstream-absorbed). The
#    {sender,content} async contract at 42618 must return 2xx.
# ==============================================================================
bold "▶ 42618 webhook-only supervision…"
wh_code="$(curl -s -o /dev/null -w '%{http_code}' -X POST "$WH/webhook" \
    -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
    -d '{"sender":"someone","content":"hi"}' || echo 000)"
if [ "$wh_code" -ge 200 ] && [ "$wh_code" -lt 300 ]; then
    ok "42618 /webhook {sender,content} → $wh_code (2xx, channel supervised)"
else
    bad "42618 /webhook → $wh_code (expected 2xx — webhook-only supervision broken)"
fi

# ==============================================================================
# 9. brain.db resume + single-backup idempotency.
#    First v0.8.0 boot on an existing PVC is a schema-migration event: it writes
#    EXACTLY ONE brain.db.backup-* (one-way sqlite v3 migration) and a seeded row
#    must survive the restart. A SECOND boot must add ZERO new backups (migration
#    is idempotent — re-running it on every boot would be a corruption risk).
# ==============================================================================
bold "▶ brain.db resume + single-backup idempotency…"
# Seed a sentinel row into brain.db via the pod's own sqlite (any table that
# survives migration; here a memory the agent stored). We prove resume by row
# survival across a restart, and count backups before/after a second boot.
if in_pod sh -c "test -f '$BRAIN_DB'"; then
    ok "brain.db present at $BRAIN_DB"
else
    bad "brain.db MISSING at $BRAIN_DB (data dir mis-resolved?)"
fi

backups_after_first="$(in_pod sh -c "ls -1 '$DATA_DIR'/memory/brain.db.backup-* 2>/dev/null | wc -l" | tr -d ' ')"
if [ "${backups_after_first:-0}" = "1" ]; then
    ok "exactly one brain.db.backup-* after first boot (migration ran once)"
else
    bad "expected exactly 1 brain.db.backup-* after first boot, found ${backups_after_first:-0}"
fi

# Seed a durable sentinel, restart, assert it survived (resume).
SENTINEL="coldstart-sentinel-$(date +%s)"
in_pod sh -c "sqlite3 '$BRAIN_DB' \"INSERT INTO memories(content) VALUES('$SENTINEL');\"" 2>/dev/null \
    || echo "  (note: sentinel insert skipped — adjust table name to the live schema during the battery)"
bold "  restarting pod (second boot on the migrated PVC)…"
docker restart "$CONTAINER" >/dev/null
sleep 4
for _ in $(seq 1 10); do
    [ "$(curl -s -o /dev/null -w '%{http_code}' "$GW/health" || echo 000)" = "200" ] && break
    sleep 1
done

survived="$(in_pod sh -c "sqlite3 '$BRAIN_DB' \"SELECT count(*) FROM memories WHERE content='$SENTINEL';\"" 2>/dev/null || echo 0)"
if [ "${survived:-0}" -ge 1 ]; then
    ok "seeded row survived restart (brain.db resume)"
else
    bad "seeded row did NOT survive restart (resume broken, or table name differs — verify in battery)"
fi

backups_after_second="$(in_pod sh -c "ls -1 '$DATA_DIR'/memory/brain.db.backup-* 2>/dev/null | wc -l" | tr -d ' ')"
if [ "${backups_after_second:-0}" = "${backups_after_first:-0}" ]; then
    ok "no NEW brain.db.backup-* after second boot (migration idempotent)"
else
    bad "backup count changed on second boot: ${backups_after_first} → ${backups_after_second} (migration re-ran!)"
fi

# ==============================================================================
# 10. emptyDir size — the resolved data dir must be well under the 10Mi emptyDir
#     budget. A blown budget means agent state landed under the install root
#     (config dir) instead of the workspace, or the workspace default wasn't
#     pinned (cross-repo follow-up #1).
# ==============================================================================
bold "▶ emptyDir size budget (config dir ≪ 10Mi)…"
cfg_kb="$(in_pod sh -c "du -s '$CONFIG_DIR' 2>/dev/null | cut -f1" | tr -d ' ')"
if [ -n "${cfg_kb:-}" ] && [ "$cfg_kb" -lt 10240 ]; then
    ok "config dir ($CONFIG_DIR) = ${cfg_kb}KiB < 10240KiB (10Mi)"
else
    bad "config dir = ${cfg_kb:-?}KiB — at/over the 10Mi emptyDir budget (agent state in the wrong dir?)"
fi

# ==============================================================================
# 11. No config.toml at the PVC root — a dir-resolution foot-gun. The rendered
#     config belongs at $CONFIG_DIR/config.toml; a stray one at /zeroclaw-data
#     would signal ZEROCLAW_CONFIG_DIR drift re-pinning data under it.
# ==============================================================================
bold "▶ no PVC-root config.toml…"
if in_pod sh -c "test -e '$PVC_ROOT/config.toml'"; then
    bad "config.toml present at PVC root ($PVC_ROOT/config.toml) — dir-resolution drift"
else
    ok "no config.toml at PVC root ($PVC_ROOT)"
fi

# ==============================================================================
# ROLLBACK DRILL (documented — run manually during the zc-b78l battery).
# ==============================================================================
# The first v0.8.0 boot performs a ONE-WAY sqlite v3 migration of brain.db and a
# per-boot TOML migration. Rolling BACK to the archived 0.6.9 image against the
# already-migrated PVC is therefore NOT a clean revert — the v3 brain.db may not
# open under the old binary. The supported rollback is restore-then-re-pin:
#
#   1. Re-pin the previous image SHA in clawcraft:
#        convex env set CLAW_DOCKER_IMAGE <previous-sha> --prod   # deployment colorful-rook-584
#   2. On the pod's PVC, restore the pre-migration backup the first v0.8.0 boot wrote:
#        cp /zeroclaw-data/workspace/memory/brain.db.backup-<ts> \
#           /zeroclaw-data/workspace/memory/brain.db
#      (the TOML re-renders from clawcraft on every boot, so no config restore needed).
#   3. Recreate the pod; confirm /health 200 and a seeded row reads back under the old binary.
#
# To exercise the FAILURE MODE (prove the drill is necessary), boot the archived
# image against the migrated volume and record that the v3 brain.db fails to open:
#   docker run --rm -v $VOLUME:/zeroclaw-data/workspace <archived-0.6.9-image> \
#     zeroclaw status   # expect: sqlite migration/version error → restore backup first
#
# PVC snapshot note: take a GKE PVC snapshot BEFORE the image bump so a full
# volume rollback is available if the backup-restore path is insufficient.

# ==============================================================================
# Summary.
# ==============================================================================
echo
bold "▶ cold-start parity summary: ${pass_count} passed, ${fail_count} failed"
if [ "$fail_count" -gt 0 ]; then
    exit 1
fi
bold "✓ cold-start parity: all checks passed."
