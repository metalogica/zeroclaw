# syntax=docker/dockerfile:1.7-labs

# ── Stage 1: Build ────────────────────────────────────────────
FROM rust:1.94-slim@sha256:da9dab7a6b8dd428e71718402e97207bb3e54167d37b5708616050b1e8f60ed6 AS builder

WORKDIR /app
ARG ZEROCLAW_CARGO_FEATURES="channel-lark,whatsapp-web,rag-pdf,observability-otel"

# Install build dependencies.
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

# 1. Copy manifests to cache dependencies
COPY Cargo.toml Cargo.lock ./
# Copy every workspace-member manifest in one glob — adding or removing a crate
# no longer requires editing this file.  --parents preserves the
# crates/<name>/Cargo.toml directory structure.
# aardvark-sys has an implicit build script (build.rs at its crate root) that
# Cargo must compile during the dependency pre-fetch step; copy it explicitly.
COPY --parents crates/*/Cargo.toml ./
COPY --parents crates/aardvark-sys/build.rs ./
# apps/tauri: .dockerignore whitelists only Cargo.toml; src and build.rs are stubbed below.
COPY apps/tauri/Cargo.toml apps/tauri/Cargo.toml
# apps/zerocode: TUI app not shipped in the server image; copy only its manifest
# so Cargo can resolve the workspace, then stub its src/main.rs and build.rs
# below. Its real build.rs reads web/src/contexts/themes.json and would panic in
# this pre-fetch stage, so it is stubbed exactly like apps/tauri.
COPY apps/zerocode/Cargo.toml apps/zerocode/Cargo.toml
# tools/fill-translations and xtask are dev/build tools; copy manifests only so
# Cargo can resolve the workspace, then stub their entry points so the
# dependency pre-fetch step succeeds without building them into the image.
COPY tools/fill-translations/Cargo.toml tools/fill-translations/Cargo.toml
COPY xtask/Cargo.toml xtask/Cargo.toml
# Create dummy targets for all workspace members so manifest parsing succeeds.
# `src/bin/zeroclaw-acp-bridge.rs` is required because the `acp-bridge` feature
# is in the root crate's default set; cargo selects the bin target during the
# pre-fetch build even with only the workspace lib stubbed.
RUN mkdir -p src src/bin benches apps/tauri/src apps/zerocode/src tools/fill-translations/src xtask/src/bin \
    && echo "fn main() {}" > src/main.rs \
    && echo "" > src/lib.rs \
    && echo "fn main() {}" > src/bin/zeroclaw-acp-bridge.rs \
    && echo "fn main() {}" > benches/agent_benchmarks.rs \
    && echo "fn main() {}" > apps/tauri/src/main.rs \
    && echo "fn main() {}" > apps/tauri/build.rs \
    && echo "fn main() {}" > apps/zerocode/src/main.rs \
    && echo "fn main() {}" > apps/zerocode/build.rs \
    && echo "fn main() {}" > tools/fill-translations/src/main.rs \
    && echo "" > xtask/src/lib.rs \
    && echo "fn main() {}" > xtask/src/bin/mdbook.rs \
    && echo "fn main() {}" > xtask/src/bin/fluent.rs \
    && echo "fn main() {}" > xtask/src/bin/web.rs \
    && mkdir -p crates/zeroclaw-hardware/examples \
    && echo "fn main() {}" > crates/zeroclaw-hardware/examples/esp32_sim.rs \
    && for d in crates/*/; do mkdir -p "${d}src" && printf '' > "${d}src/lib.rs"; done
RUN --mount=type=cache,id=zeroclaw-cargo-registry-v080,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=zeroclaw-cargo-git-v080,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=zeroclaw-target-v080,target=/app/target,sharing=locked \
    if [ -n "$ZEROCLAW_CARGO_FEATURES" ]; then \
      cargo build --release --locked -p zeroclawlabs --bin zeroclaw --features "$ZEROCLAW_CARGO_FEATURES"; \
    else \
      cargo build --release --locked -p zeroclawlabs --bin zeroclaw; \
    fi
RUN rm -rf src benches crates xtask tools/fill-translations

# 2. Copy only build-relevant source paths (avoid cache-busting on docs/tests/scripts)
COPY src/ src/
COPY benches/ benches/
COPY crates/ crates/
COPY xtask/ xtask/
COPY tools/fill-translations/ tools/fill-translations/
# locales.toml lives at repo root and is embedded by zeroclaw-runtime via
# include_str!("../../../locales.toml"); the real build needs it present.
COPY locales.toml .
COPY *.rs .
RUN touch src/main.rs
RUN --mount=type=cache,id=zeroclaw-cargo-registry-v080,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=zeroclaw-cargo-git-v080,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=zeroclaw-target-v080,target=/app/target,sharing=locked \
    rm -rf target/release/.fingerprint/zeroclawlabs-* \
           target/release/deps/zeroclawlabs-* \
           target/release/incremental/zeroclawlabs-* && \
    if [ -n "$ZEROCLAW_CARGO_FEATURES" ]; then \
      cargo build --release --locked -p zeroclawlabs --bin zeroclaw --features "$ZEROCLAW_CARGO_FEATURES"; \
    else \
      cargo build --release --locked -p zeroclawlabs --bin zeroclaw; \
    fi && \
    cp target/release/zeroclaw /app/zeroclaw && \
    strip /app/zeroclaw
RUN size=$(stat -c%s /app/zeroclaw) && \
    if [ "$size" -lt 1000000 ]; then echo "ERROR: binary too small (${size} bytes), likely dummy build artifact" && exit 1; fi

# Prepare runtime directory structure and default config inline (no extra stage).
RUN mkdir -p /zeroclaw-data/.zeroclaw /zeroclaw-data/data && \
    printf '%s\n' \
        'api_key = ""' \
        'default_provider = "openrouter"' \
        'default_model = "anthropic/claude-sonnet-4-20250514"' \
        'default_temperature = 0.7' \
        '' \
        '[gateway]' \
        'port = 42617' \
        'host = "[::]"' \
        'allow_public_bind = true' \
        'require_pairing = false' \
        'web_dist_dir = "/usr/share/zeroclawlabs/web/dist"' \
        '' \
        '[risk_profiles.default]' \
        'level = "supervised"' \
        'auto_approve = ["file_read", "file_write", "file_edit", "memory_recall", "memory_store", "web_search_tool", "web_fetch", "calculator", "glob_search", "content_search", "image_info", "weather", "git_operations"]' \
        > /zeroclaw-data/.zeroclaw/config.toml && \
    chown -R 65534:65534 /zeroclaw-data

# ── Stage 2: Development Runtime (Debian) ────────────────────
FROM debian:trixie-slim@sha256:f6e2cfac5cf956ea044b4bd75e6397b4372ad88fe00908045e9a0d21712ae3ba AS dev

# Install essential runtime dependencies only (use docker-compose.override.yml for dev tools)
RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /zeroclaw-data /zeroclaw-data
COPY --from=builder /app/zeroclaw /usr/local/bin/zeroclaw

# Overwrite minimal config with DEV template (Ollama defaults)
COPY dev/config.template.toml /zeroclaw-data/.zeroclaw/config.toml
RUN chown 65534:65534 /zeroclaw-data/.zeroclaw/config.toml

# Environment setup
# Ensure UTF-8 locale so CJK / multibyte input is handled correctly
ENV LANG=C.UTF-8
# Bootstrap (uppercase tail) — pre-load: decides where the config file lives.
ENV ZEROCLAW_DATA_DIR=/zeroclaw-data/data
ENV HOME=/zeroclaw-data
# V0.8.0 env-var grammar: `ZEROCLAW_<dotted_path_with_double_underscores>=<value>`
# mirrors the TOML config 1:1; `__` is the path separator. Operators inject
# credentials and runtime knobs at `docker run -e ...` (or via docker-compose
# `environment:`). Legacy `PROVIDER`, `ZEROCLAW_MODEL`, `ANTHROPIC_API_KEY`,
# `API_KEY`, etc. fallbacks were eradicated. Example:
#   docker run -e ZEROCLAW_providers__models__anthropic__default__api_key=sk-ant-... ...
ENV ZEROCLAW_gateway__port=42617

WORKDIR /zeroclaw-data
USER 65534:65534
EXPOSE 42617
HEALTHCHECK --interval=60s --timeout=10s --retries=3 --start-period=10s \
    CMD ["zeroclaw", "status", "--format=exit-code"]
ENTRYPOINT ["zeroclaw"]
CMD ["daemon"]

# ── Stage 2.5: Praxis install (private GH Packages) ──────────
FROM node:20-alpine AS praxis-install

ARG PRAXIS_VERSION=0.10.0

# Install the private @soulbound-labs/praxis CLI using a BuildKit secret for the
# npm token (never an ARG/ENV — the token must not persist in any image layer).
# The scoped .npmrc is written just for the install and removed immediately after.
# The installed package is relocated to /opt/praxis so the release stage can
# `COPY --from=praxis-install /opt/praxis /opt/praxis` with a stable path.
RUN --mount=type=secret,id=npm_token \
    sh -c 'set -eu; \
      { \
        echo "@soulbound-labs:registry=https://npm.pkg.github.com"; \
        echo "//npm.pkg.github.com/:_authToken=$(cat /run/secrets/npm_token)"; \
        echo "always-auth=true"; \
      } > ~/.npmrc; \
      npm install -g "@soulbound-labs/praxis@${PRAXIS_VERSION}"; \
      rm -f ~/.npmrc; \
      mkdir -p /opt; \
      cp -R /usr/local/lib/node_modules/@soulbound-labs/praxis /opt/praxis'

# ── Stage 3: Production Runtime (Wolfi) ───────────────────────
FROM cgr.dev/chainguard/wolfi-base:latest AS release

RUN apk add --no-cache ca-certificates bash coreutils vim git nodejs

COPY --from=builder /app/zeroclaw /usr/local/bin/zeroclaw
COPY --from=builder /zeroclaw-data /zeroclaw-data

# Praxis CLI (private @soulbound-labs/praxis from GH Packages)
COPY --from=praxis-install /opt/praxis /opt/praxis
RUN ln -sf /opt/praxis/dist/bin-bootstrap.cjs /usr/local/bin/praxis

# Environment setup
# Ensure UTF-8 locale so CJK / multibyte input is handled correctly
ENV LANG=C.UTF-8
# v0.8.0 data-dir pin (canonical). ZEROCLAW_WORKSPACE is kept during the
# transition as a deprecated alias; DATA_DIR wins (with a WARN) when both are
# set. Never set ZEROCLAW_CONFIG_DIR — it would re-pin data under
# <config_dir>/data and orphan the existing /zeroclaw-data/workspace PVC.
ENV ZEROCLAW_DATA_DIR=/zeroclaw-data/workspace
ENV ZEROCLAW_WORKSPACE=/zeroclaw-data/workspace
ENV HOME=/zeroclaw-data

# API_KEY must be provided at runtime!

WORKDIR /zeroclaw-data
USER 65534
EXPOSE 42617
HEALTHCHECK --interval=60s --timeout=10s --retries=3 --start-period=10s \
    CMD ["zeroclaw", "status", "--format=exit-code"]
ENTRYPOINT ["zeroclaw"]
CMD ["daemon"]
