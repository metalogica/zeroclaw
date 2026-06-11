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
#   NO_BAKE=1          skip baking the binary into the image (fast inner loop).
#                      With baking ON (the default) the swap also survives a
#                      'compose up' recreate; with NO_BAKE=1 a recreate reverts
#                      to the stale image binary.
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
bold "▶ compiling zeroclaw (debug · mold · incremental) — features: $FEATURES"
time docker run --rm \
    -v "$REPO":/app \
    -v zeroclaw-hotswap-registry:/usr/local/cargo/registry \
    -v zeroclaw-hotswap-git:/usr/local/cargo/git \
    -v zeroclaw-hotswap-target:/target \
    -e CARGO_TARGET_DIR=/target \
    -w /app \
    "$BUILDER_IMG" \
    cargo build --locked --bin zeroclaw --features "$FEATURES"

# 2b. Extract the binary off the named volume to the host. The old path baked an
#     'install …/target/hotswap/zeroclaw' into the compile step, which wrote the
#     ~159MB binary back through the /app bind mount — that ride over Docker
#     Desktop file sharing clocked ~315KB/s ≈ 8.4min/swap. Streaming it out as a
#     throwaway container's stdout rides the daemon API instead (the same fast
#     path as `docker cp`): the same bytes land in ~9s. The compile container
#     above touches only the named volume now, so it never pays the fs-share tax.
bold "▶ extracting binary off named volume (stdout stream — not the bind mount)…"
time docker run --rm \
    -v zeroclaw-hotswap-target:/target \
    "$BUILDER_IMG" \
    cat /target/debug/zeroclaw > "$BIN_OUT"
chmod 0755 "$BIN_OUT"

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

# 5. Bake the freshly built binary INTO the image the container was created from.
#    The cp+restart above only patches the live container — a 'compose up' recreate
#    rebuilds the container from the IMAGE and silently reverts to the stale baked
#    binary. Baking the binary into that image tag closes the gap. Default-on:
#    correctness beats the ~1s cached overlay build; NO_BAKE=1 opts out.
#
#    NOTE (prod, out of scope here): this fixes the LOCAL dev image only. The
#    prod/GKE image (…/clawcraft-images/clawcraft-claw-runtime:latest) is a
#    separately published artifact and still needs a real upstream build+publish
#    of a post-21:23 (single-exporting-observer-fix) zeroclaw. TODO: do that build.
TAG="$(docker inspect -f '{{.Config.Image}}' "$CONTAINER")"
if [ "${NO_BAKE:-0}" = "1" ]; then
    bold "▶ bake skipped (NO_BAKE=1) — a 'compose up' recreate will revert '$TAG' to its baked-in binary"
else
    bold "▶ baking binary into image '$TAG' (survives 'compose up' recreate)…"

    # Avoid stacking a fresh layer on an already-baked image every run: FROM the
    # ORIGINAL base, pinned under a stable local tag the first time we bake. On
    # re-runs we build FROM that pinned tag, so a single bake layer replaces the
    # last one instead of accreting. (We pin via a tag, not a bare image ID —
    # BuildKit parses `FROM <sha256-id>` as a registry ref and tries to pull it.)
    #
    # To re-pin after pulling a fresh upstream image (e.g. a real post-fix build):
    #   docker rmi "$BASE_TAG"; docker tag catonmat/zeroclaw:<tag> "$TAG"; rerun.
    BASE_TAG="${TAG%:*}:hotswap-base"
    if docker image inspect "$BASE_TAG" >/dev/null 2>&1; then
        bold "  re-bake — FROM pinned base '$BASE_TAG' (no layer accretion)"
    else
        docker tag "$TAG" "$BASE_TAG"
        bold "  first bake — pinned original base as '$BASE_TAG' (future runs FROM it)"
    fi

    # The image runs as nonroot 65534, so we switch to root for the chmod and back.
    cat > "$OUT_DIR/Dockerfile.bake" <<DOCKERFILE
FROM $BASE_TAG
USER 0
COPY zeroclaw /usr/local/bin/zeroclaw
RUN chmod 0755 /usr/local/bin/zeroclaw
USER 65534:65534
DOCKERFILE

    docker build -f "$OUT_DIR/Dockerfile.bake" -t "$TAG" "$OUT_DIR" >/dev/null
    bold "✓ baked into '$TAG' — a later 'compose up' recreate now carries your binary"
fi

cat <<EOF

  Next:
    • drive a WS turn, then:  docker logs --since 2m $CONTAINER 2>&1 | grep ws-activation-probe
    • the swap persists across 'docker restart' AND (with baking on, the default)
      a 'compose up' recreate — the binary is baked into '$TAG'.
      With NO_BAKE=1 it survives only 'docker restart'; a recreate reverts.
    • verify a recreate kept your binary (distinguishing marker of the good build):
        docker exec $CONTAINER sh -c 'grep -ac lmnr.span.input /usr/local/bin/zeroclaw'   # > 0
EOF
