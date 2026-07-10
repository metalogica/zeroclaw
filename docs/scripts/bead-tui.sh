#!/usr/bin/env bash
# bead-tui — live terminal view of the project's bead DAGs (see bead-tui/README.md).
# Thin wrapper so it's one command, not a path. Forwards all args to watch.mjs.
exec node "$(dirname "$0")/bead-tui/watch.mjs" "$@"
