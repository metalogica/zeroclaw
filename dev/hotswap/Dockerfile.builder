# syntax=docker/dockerfile:1.7
#
# Fast dev *hot-swap* builder for the zeroclaw binary.
#
# Purpose: compile a drop-in linux binary in seconds-to-minutes (debug profile
# + lld linker, incremental), so a code change can be swapped into a running
# `clawcraft-claw` container without the ~20-min fat-LTO release image build.
#
# This is a DEV-ONLY tool. The binary it produces is unoptimized (opt-level 0,
# no LTO) and larger/slower at runtime than the release image — that is fine for
# behavior testing (e.g. "does the WS activation span emit?"). Never ship it.
#
# Base matches the prod Dockerfile builder (rust:1.94-slim, debian/glibc) so the
# binary is ABI-compatible with the runtime image exactly as the prod
# debian-builder -> wolfi-runtime copy already is.
FROM rust:1.94-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        clang \
        lld \
    && rm -rf /var/lib/apt/lists/*

# clang + lld: linking the ~300k-line binary dominates an incremental rebuild;
# lld is dramatically faster than the default bfd linker.
# debuginfo=0: a debug build spends a large fraction generating DWARF we never
# use for span/behavior testing — dropping it is the biggest single compile-time
# win short of splitting the monolith crate. (Backtraces still have symbol names;
# you just lose line numbers — fine for this dev hot-swap loop.)
ENV RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=lld -C debuginfo=0"
ENV CARGO_INCREMENTAL=1

WORKDIR /app
