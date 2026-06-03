#!/usr/bin/env bash
#
# Fast dev loop: compile the zeroclaw binary (debug + lld, incremental) and
# hot-swap it into the running claw container — no image rebuild.
#
# First run is a cold debug build (downloads + compiles all deps into named
# volumes, ~minutes). Every run after that is incremental: only the changed
# crate recompiles + relinks, typically well under a minute.
#
# Usage:
#   dev/hotswap/hotswap.sh                 # build + swap into $CLAW_CONTAINER
#   CLAW_CONTAINER=clawcraft-claw  dev/hotswap/hotswap.sh
#   ZEROCLAW_FEATURES="observability-otel" dev/hotswap/hotswap.sh   # fewer features = faster
#
# Env overrides:
#   CLAW_CONTAINER     running container to swap into   (default: clawcraft-claw)
#   ZEROCLAW_FEATURES  cargo features to compile        (default: match the prod image)
#   NO_RESTART=1       cp the binary but skip restart   (you restart yourself)
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONTAINER="${CLAW_CONTAINER:-clawcraft-claw}"
FEATURES="${ZEROCLAW_FEATURES:-channel-lark,whatsapp-web,rag-pdf,observability-otel}"
BUILDER_IMG="zeroclaw-hotswap-builder"
OUT_DIR="$REPO/target/hotswap"
BIN_OUT="$OUT_DIR/zeroclaw"

bold() { printf '\033[1m%s\033[0m\n' "$*"; }

# 0. Sanity: the target container must be running.
if ! docker ps --format '{{.Names}}' | grep -qx "$CONTAINER"; then
    echo "✗ container '$CONTAINER' is not running."
    echo "  running candidates:"
    docker ps --format '   {{.Names}}\t{{.Image}}' | grep -i claw || true
    echo "  set CLAW_CONTAINER=<name> and retry."
    exit 1
fi

# 1. Builder image. Built once; skipped on every subsequent run unless the
#    Dockerfile changed or REBUILD_BUILDER=1 forces it. Re-running `docker build`
#    each invocation costs context transfer + layer eval for nothing.
if [ "${REBUILD_BUILDER:-0}" = "1" ] || ! docker image inspect "$BUILDER_IMG" >/dev/null 2>&1; then
    bold "▶ building hot-swap builder image…"
    docker build -f "$REPO/dev/hotswap/Dockerfile.builder" -t "$BUILDER_IMG" "$REPO/dev/hotswap" >/dev/null
else
    bold "▶ hot-swap builder image present (REBUILD_BUILDER=1 to force rebuild)"
fi

# 2. Compile (debug, incremental) inside the linux builder. Deps + target live
#    on named volumes so rebuilds are incremental across invocations. Source is
#    bind-mounted; CARGO_TARGET_DIR is redirected to the volume so the host tree
#    stays clean. --locked keeps Cargo.lock authoritative.
mkdir -p "$OUT_DIR"
bold "▶ compiling zeroclaw (debug · lld · incremental) — features: $FEATURES"
time docker run --rm \
    -v "$REPO":/app \
    -v zeroclaw-hotswap-registry:/usr/local/cargo/registry \
    -v zeroclaw-hotswap-git:/usr/local/cargo/git \
    -v zeroclaw-hotswap-target:/target \
    -e CARGO_TARGET_DIR=/target \
    -w /app \
    "$BUILDER_IMG" \
    bash -c "cargo build --locked --bin zeroclaw --features '$FEATURES' && install -m0755 /target/debug/zeroclaw /app/target/hotswap/zeroclaw"

# 3. Self-verify the build actually carries your source change. This closes the
#    'is the fix even in this binary?' gap that burned earlier rebuild rounds —
#    grep the freshly built binary for the in-flight probe marker.
probe_hits="$(grep -ac 'ws-activation-probe' "$BIN_OUT" || true)"
if [ "${probe_hits:-0}" -gt 0 ]; then
    bold "✓ built binary contains 'ws-activation-probe' ($probe_hits hits) — your change is in this build"
else
    echo "ℹ built binary has no 'ws-activation-probe' marker (expected once the temporary probes are removed)"
fi

# 4. Swap into the running container and restart so the daemon re-execs it.
bold "▶ swapping binary into '$CONTAINER'…"
docker cp "$BIN_OUT" "$CONTAINER:/usr/local/bin/zeroclaw"
docker exec -u 0 "$CONTAINER" chmod 0755 /usr/local/bin/zeroclaw

if [ "${NO_RESTART:-0}" = "1" ]; then
    bold "✓ binary swapped (restart skipped — NO_RESTART=1). Restart yourself: docker restart $CONTAINER"
else
    bold "▶ restarting '$CONTAINER'…"
    docker restart "$CONTAINER" >/dev/null
    bold "✓ hot-swap live in '$CONTAINER'."
fi

cat <<EOF

  Next:
    • drive a WS turn, then:  docker logs --since 2m $CONTAINER 2>&1 | grep ws-activation-probe
    • the swap persists across 'docker restart' but NOT 'compose up --force-recreate'
      (that recreates from the image) — rerun this script after a recreate.
EOF
