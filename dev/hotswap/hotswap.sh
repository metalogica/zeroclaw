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
#    correctness beats the few-second bake; NO_BAKE=1 opts out.
#
#    We bake with `docker commit`, NOT `docker build`. The old Dockerfile bake
#    sent $OUT_DIR (the 159MB binary) as a build context — ~904s at ~185KB/s
#    through Docker Desktop file sharing — to re-add bytes that `docker cp`
#    already moved into the container in ~9s. `docker commit` captures a
#    container's filesystem with ZERO context transfer (it rides the daemon API).
#
#    Layer accretion (why the old path pinned a 'hotswap-base' to FROM): naively
#    committing the LIVE container would, after each 'compose up' recreate,
#    stack another layer onto an ever-growing image. We sidestep that entirely:
#    every bake commits a THROWAWAY container created from the pinned pristine
#    base, with only the binary cp'd in — so $TAG is always base+exactly-1-layer,
#    no matter how many times we swap or recreate. No per-N re-pin needed.
#
#    NOTE (prod, out of scope here): this fixes the LOCAL dev image only. The
#    prod/GKE image (…/clawcraft-images/clawcraft-claw-runtime:latest) is a
#    separately published artifact and still needs a real upstream build+publish
#    of a post-21:23 (single-exporting-observer-fix) zeroclaw. TODO: do that build.
TAG="$(docker inspect -f '{{.Config.Image}}' "$CONTAINER")"
if [ "${NO_BAKE:-0}" = "1" ]; then
    bold "▶ bake skipped (NO_BAKE=1) — a 'compose up' recreate will revert '$TAG' to its baked-in binary"
else
    bold "▶ baking binary into image '$TAG' via docker commit (no build context)…"

    # Pin the ORIGINAL pristine image under a stable tag the first time we bake;
    # every bake commits FROM it so a single binary layer replaces the last one
    # instead of accreting.
    #
    # To re-pin after pulling a fresh upstream image (e.g. a real post-fix build):
    #   docker rmi "$BASE_TAG"; docker tag catonmat/zeroclaw:<tag> "$TAG"; rerun.
    BASE_TAG="${TAG%:*}:hotswap-base"
    if docker image inspect "$BASE_TAG" >/dev/null 2>&1; then
        bold "  re-bake — committing FROM pinned base '$BASE_TAG' (no layer accretion)"
    else
        docker tag "$TAG" "$BASE_TAG"
        bold "  first bake — pinned original base as '$BASE_TAG' (future bakes FROM it)"
    fi

    # Throwaway container created (not started) from the pristine base. docker cp
    # streams the binary in over the daemon API (no context transfer) and the
    # tar stream preserves the 0755 mode we set in step 2b, so the nonroot 65534
    # runtime can exec it without a chmod (which would need a running container).
    # `docker commit` then carries the base image's config (USER/CMD/ENV/...)
    # forward unchanged, so the committed $TAG behaves exactly like the original.
    stage_cid="$(docker create "$BASE_TAG")"
    trap 'docker rm -f "$stage_cid" >/dev/null 2>&1 || true' EXIT
    docker cp "$BIN_OUT" "$stage_cid:/usr/local/bin/zeroclaw"
    docker commit "$stage_cid" "$TAG" >/dev/null
    docker rm "$stage_cid" >/dev/null
    trap - EXIT
    bold "✓ baked into '$TAG' (base+1 layer) — a later 'compose up' recreate now carries your binary"
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
