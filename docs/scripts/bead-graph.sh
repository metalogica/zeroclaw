#!/usr/bin/env bash
# bead-graph — render a bead DAG so any agent (or a human) can SEE its shape and
# plan parallel vs. sequential execution. Reads tbd; computes dependency waves via
# Kahn's algorithm. Pure bash 3.2 + coreutils + tbd, ZERO other runtime deps.
#
#   Run:  bash docs/scripts/bead-graph.sh --epic <slug> [--format waves|mermaid|dot]
#         bash docs/scripts/bead-graph.sh                      # all open beads
#
# The canonical epic identity is the label `epic:<slug>` (see the parallel-execution
# doctrine). `--epic <slug>` scopes the graph to `tbd list --label epic:<slug>` — the
# same "one card, beads as subtasks" grouping /substrate:graph-spec and
# /substrate:synthesize-session both write.
#
# Views:
#   waves   (default) — topological layers; every id in one wave is safe to run in
#                       parallel; waves run in order. This is the parallel-exec plan.
#   mermaid — `graph TD` you can paste into Claude Code / GitHub / any mermaid viewer.
#   dot     — Graphviz digraph for `dot -Tpng`.

set -u

FORMAT="waves"
EPIC=""
STATUS_FILTER="open"

while [ $# -gt 0 ]; do
  case "$1" in
    --format) FORMAT="${2:-waves}"; shift 2 ;;
    --epic)   EPIC="${2:-}"; shift 2 ;;
    --status) STATUS_FILTER="${2:-open}"; shift 2 ;;
    -h|--help) sed -n '2,22p' "$0"; exit 0 ;;
    *) echo "bead-graph: unknown arg '$1'" >&2; exit 2 ;;
  esac
done

# Tolerate both `--epic <slug>` and `--epic epic:<slug>` — the script adds the `epic:` prefix.
EPIC="${EPIC#epic:}"

# Resolve the tbd CLI: global binary, else local get-tbd via npx.
if command -v tbd >/dev/null 2>&1; then
  TBD="tbd"
elif npx --no-install get-tbd --version >/dev/null 2>&1; then
  TBD="npx --no-install get-tbd"
else
  echo "bead-graph: no tbd CLI found (need \`tbd\` on PATH or a local get-tbd install)." >&2
  exit 1
fi

LIST_ARGS="--status $STATUS_FILTER"
[ -n "$EPIC" ] && LIST_ARGS="$LIST_ARGS --label epic:$EPIC"

RAW=$($TBD list $LIST_ARGS --no-sync 2>/dev/null)
# Bead rows start with an id token `<prefix>-<alnum>`; skip the header + "N issue(s)" footer.
# Drop `[epic]` container rows — the epic is the card/grouping, not a task in a wave.
NODES=$(printf '%s\n' "$RAW" | grep -vE '\[epic\]' | grep -oE '^[[:alnum:]]+-[[:alnum:]]+' | sort -u)

if [ -z "$NODES" ]; then
  scope="all open beads"; [ -n "$EPIC" ] && scope="epic:$EPIC"
  echo "bead-graph: no beads found for $scope." >&2
  exit 1
fi

NODE_SET=" $(printf '%s' "$NODES" | tr '\n' ' ') "   # space-delimited membership set

# Edge list "child<TAB>blocker", blockers restricted to the node set (external/closed
# blockers don't gate waves). Only `Blocked by:` lines — never the reverse `Blocks:`.
EDGES=$(mktemp -t beadgraph-XXXXXX)
trap 'rm -f "$EDGES"' EXIT
for n in $NODES; do
  for b in $($TBD dep list "$n" --no-sync 2>/dev/null \
              | grep -i 'blocked by' \
              | grep -oE '[[:alnum:]]+-[[:alnum:]]+'); do
    case "$NODE_SET" in *" $b "*) printf '%s\t%s\n' "$n" "$b" >> "$EDGES" ;; esac
  done
done

# title <id> -> "id — title" (title = text after the first `]` type marker).
title() {
  local line
  line=$(printf '%s\n' "$RAW" | grep -E "^$1[[:space:]]")
  local t
  t=$(printf '%s' "$line" | sed -E 's/^[^]]*\] //')
  [ -n "$t" ] && printf '%s — %s' "$1" "$t" || printf '%s' "$1"
}

blockers_of() { grep -E "^$1"$'\t' "$EDGES" | cut -f2; }

# ---- partition overlay: each bead's `group:<window-N>` label (context-budget windows) ----
# /substrate:graph-spec stamps `group:<window-N>` on every bead; the orchestrator dispatches
# one group-runner per window. Absent labels (ungraphed / pre-partition DAG) render as before.
GROUPS=$(mktemp -t beadgrp-XXXXXX)
trap 'rm -f "$EDGES" "$GROUPS"' EXIT
for n in $NODES; do
  g=$($TBD show "$n" --no-sync 2>/dev/null | grep -oE 'group:[[:alnum:]_-]+' | head -n1)
  [ -n "$g" ] && printf '%s\t%s\n' "$n" "$g" >> "$GROUPS"
done
group_of() { grep -E "^$1"$'\t' "$GROUPS" 2>/dev/null | cut -f2 | head -n1; }
GROUPED=0; [ -s "$GROUPS" ] && GROUPED=1

case "$FORMAT" in
  mermaid)
    echo '```mermaid'
    echo 'graph TD'
    if [ "$GROUPED" = "1" ]; then
      # Box each context-budget window as a subgraph; ungrouped nodes fall through below.
      for g in $(cut -f2 "$GROUPS" | sort -u); do
        printf '  subgraph %s\n' "$g"
        for n in $NODES; do
          [ "$(group_of "$n")" = "$g" ] || continue
          lbl=$(title "$n" | sed 's/"/'"'"'/g')
          printf '    %s["%s"]\n' "$n" "$lbl"
        done
        echo '  end'
      done
      for n in $NODES; do
        [ -z "$(group_of "$n")" ] || continue
        lbl=$(title "$n" | sed 's/"/'"'"'/g')
        printf '  %s["%s"]\n' "$n" "$lbl"
      done
    else
      for n in $NODES; do
        lbl=$(title "$n" | sed 's/"/'"'"'/g')
        printf '  %s["%s"]\n' "$n" "$lbl"
      done
    fi
    # blocker --> child : the blocker must land first.
    while IFS=$'\t' read -r child blocker; do
      printf '  %s --> %s\n' "$blocker" "$child"
    done < "$EDGES"
    echo '```'
    exit 0
    ;;
  dot)
    echo 'digraph beads {'
    echo '  rankdir=LR;'
    while IFS=$'\t' read -r child blocker; do
      printf '  "%s" -> "%s";\n' "$blocker" "$child"
    done < "$EDGES"
    echo '}'
    exit 0
    ;;
  waves) : ;;
  *) echo "bead-graph: unknown --format '$FORMAT' (waves|mermaid|dot)" >&2; exit 2 ;;
esac

# ---- waves view (Kahn): peel off nodes whose in-set blockers are all already placed ----
remaining="$NODE_SET"
wave=0
scope="all open beads"; [ -n "$EPIC" ] && scope="epic:$EPIC"
echo "Bead DAG — $scope"
echo

while [ "$(printf '%s' "$remaining" | tr -d ' ')" != "" ]; do
  wave=$((wave + 1))
  current=""
  for n in $remaining; do
    ready=1
    for b in $(blockers_of "$n"); do
      case "$remaining" in *" $b "*) ready=0; break ;; esac   # blocker not yet placed
    done
    [ "$ready" = "1" ] && current="$current $n"
  done
  if [ -z "$(printf '%s' "$current" | tr -d ' ')" ]; then
    echo "✗ cycle detected — these beads mutually block and cannot be ordered:" >&2
    echo "   $(printf '%s' "$remaining" | tr -s ' ')" >&2
    exit 3
  fi
  count=$(printf '%s' "$current" | wc -w | tr -d ' ')
  if [ "$count" -gt 1 ]; then
    echo "── Wave $wave ($count beads — run in PARALLEL) ──"
  else
    echo "── Wave $wave (sequential) ──"
  fi
  for n in $current; do
    g=$(group_of "$n")
    if [ -n "$g" ]; then
      echo "   • $(title "$n")   [$g]"
    else
      echo "   • $(title "$n")"
    fi
    remaining=$(printf '%s' "$remaining" | sed "s/ $n / /")
  done
  echo
done

if [ "$GROUPED" = "1" ]; then
  echo "Windows: beads tagged [group:<window-N>] share one warm worktree + one group-runner"
  echo "(sequential within a window; parallel across file-disjoint windows). See the"
  echo "parallel-execution doctrine §Grouping & windows."
  echo
fi

echo "$wave wave(s). Beads in the same wave touch no shared blocker — dispatch them together;"
echo "run waves in order. Feed this straight into the parallel-execution doctrine's orchestrator."
