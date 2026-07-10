#!/usr/bin/env node
// bead-tui — a live, terminal view of a project's bead DAGs that re-renders as tbd
// state evolves: beads change status, blocked beads unblock, new ones appear. Zero
// runtime deps (Node built-ins only), so it ships wherever bead-graph.sh ships.
//
//   node watch.mjs                       # auto-load the latest epic; Tab to cycle
//   node watch.mjs --tbd <epic-slug>     # pin one epic
//   node watch.mjs --fixture path.json   # a specific fixture file
//   node watch.mjs --once                # render the default view once, exit (CI)
//   node watch.mjs --list-views          # print discovered views, exit
//
// Tabs (interactive TTY only): Tab / →  next · Shift-Tab / ←  prev · q / Ctrl-C quit.
// Views = one per epic (newest first) + an "unassigned" tab (beads with no epic: label).
// Topology is Kahn's waves (as bead-graph.sh computes); `blocked` is derived (an open
// bead with an unclosed blocker). Rendering is top-to-bottom waves + inline ← blockers.
//
// tbd's CLI is slow (~2-3s/call, git-native), so every tbd call is ASYNC (execFile) and
// runs in parallel — the event loop never blocks, keeping keypresses and the spinner live
// while data is fetched in the background.

import { readFileSync, statSync } from 'node:fs';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const pexec = promisify(execFile);
const HERE = dirname(fileURLToPath(import.meta.url));

// ---- args -------------------------------------------------------------------
const argv = process.argv.slice(2);
const flag = (name) => { const i = argv.indexOf(name); return i >= 0 ? (argv[i + 1] ?? true) : undefined; };
const ONCE = argv.includes('--once');
const LIST_VIEWS = argv.includes('--list-views');
const EPIC = flag('--tbd');                                   // pin a single epic
const FIXTURE_ARG = flag('--fixture');                        // pin a fixture file
const DEFAULT_FIXTURE = join(HERE, 'fixture.json');
const INTERVAL = Number(flag('--interval')) || 800;           // idle gap between (self-paced) polls
const MAX_NODES = Number(flag('--max')) || 60;                    // max beads rendered per view (--max <n> to override)

function die(msg) { restore(); process.stderr.write(`bead-tui: ${msg}\n`); process.exit(1); }
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ---- tbd resolver (async; prefer global tbd, else local get-tbd) -------------
let _bin;                                                     // undefined until resolved; null if unavailable
async function resolveBin() {
  if (_bin !== undefined) return _bin;
  try { await pexec('tbd', ['--version']); return (_bin = { cmd: 'tbd', pre: [] }); } catch { /* try npx */ }
  try { await pexec('npx', ['--no-install', 'get-tbd', '--version']); return (_bin = { cmd: 'npx', pre: ['--no-install', 'get-tbd'] }); } catch { /* none */ }
  return (_bin = null);
}
async function tbd(a) {
  const b = await resolveBin(); if (!b) return null;
  try { const { stdout } = await pexec(b.cmd, [...b.pre, ...a], { maxBuffer: 1 << 26 }); return stdout; } catch { return null; }
}
const tbdAvailable = async () => (await resolveBin()) !== null;
function jparse(s, fallback) { try { return JSON.parse(s); } catch { return fallback; } }
const tlist = async (extra) => jparse(await tbd(['list', ...extra, '--json', '--no-sync']) || '[]', []);
const tshow = async (id) => jparse(await tbd(['show', id, '--json', '--no-sync']) || '{}', {});
const tblocked = async () => jparse(await tbd(['blocked', '--json', '--no-sync']) || '[]', []);
const firstId = (s) => (typeof s === 'string' ? s : s?.id || '').trim().split(/\s/)[0] || null;  // "sub-x ◐ title" → "sub-x"
const basename = (p) => p.split('/').pop();

// ---- bulk snapshot: TWO tbd calls (run in PARALLEL), refreshed on each poll --
//   SNAP.rows: id → {id,title,status,kind}   ·   SNAP.edges: childId → [blockerIds]
let SNAP = null;
async function refreshSnapshot() {
  const [listRows, blocked] = await Promise.all([tlist(['--all']), tblocked()]);
  const rows = new Map();
  for (const r of listRows) rows.set(r.id, { id: r.id, title: r.title, status: r.status, kind: r.kind, labels: r.labels || [] });
  const edges = new Map();
  for (const b of blocked) edges.set(b.id, (b.blockedBy || []).map(firstId).filter(Boolean));
  SNAP = { rows, edges };
}
const idSignature = () => (SNAP ? [...SNAP.rows.keys()].sort().join(',') : '');
const contentSig = () => {
  if (!SNAP) return '';
  const rows = [...SNAP.rows.values()].map((r) => `${r.id}:${r.status}`).sort().join('|');
  const edges = [...SNAP.edges.entries()].map(([k, v]) => `${k}>${[...v].sort().join(',')}`).sort().join('|');
  return `${rows}#${edges}`;
};

// ---- membership: slower (per-epic show for slug, per-slug list). Every tbd call
//   fanned out with Promise.all so cost is ~constant rounds, not per-epic serial. ---
//   MEMBERSHIP.epics: [{slug,ts}] newest-first · memberIds: slug→Set · unassigned: Set
let MEMBERSHIP = null;
async function refreshMembership() {
  // Skip closed (done) epic containers — filtered from the tabs anyway, and a `tbd show`
  // per closed epic is the dominant startup cost on repos with lots of history.
  const epicRows = [...SNAP.rows.values()].filter((r) => r.kind === 'epic' && r.status !== 'closed');
  const metas = await Promise.all(epicRows.map((e) => tshow(e.id)));    // all shows concurrently
  const slugTs = new Map();                                             // dedupe containers sharing a slug; keep newest
  for (const meta of metas) {
    const slug = (meta.labels || []).map((l) => /^epic:(.+)$/.exec(l)?.[1]).find(Boolean);
    if (!slug) continue;
    const ts = meta.updated_at || meta.created_at || '';
    if (!slugTs.has(slug) || slugTs.get(slug) < ts) slugTs.set(slug, ts);
  }
  const slugs = [...slugTs.keys()];
  const lists = await Promise.all(slugs.map((s) => tlist(['--label', `epic:${s}`, '--all'])));  // members concurrently
  const memberIds = new Map(); const claimed = new Set();
  slugs.forEach((slug, i) => {
    const ids = new Set(lists[i].filter((r) => r.kind !== 'epic').map((r) => r.id));
    memberIds.set(slug, ids); for (const id of ids) claimed.add(id);
  });
  const active = (slug) => [...(memberIds.get(slug) || [])].some((id) => SNAP.rows.get(id)?.status !== 'closed');
  const epics = [...slugTs.entries()]
    .filter(([slug]) => active(slug))                                   // hide fully-closed (done) epics from the tabs
    .map(([slug, ts]) => ({ slug, ts })).sort((a, b) => (a.ts < b.ts ? 1 : -1));
  const unassigned = new Set([...SNAP.rows.values()].filter((r) => r.kind !== 'epic' && !claimed.has(r.id)).map((r) => r.id));
  MEMBERSHIP = { epics, memberIds, unassigned };
}

async function resolveViews() {
  if (FIXTURE_ARG) return [{ key: `fixture:${basename(FIXTURE_ARG)}`, type: 'fixture', path: FIXTURE_ARG }];
  if (EPIC) {                                                           // pinned single epic
    await refreshSnapshot();
    const ids = new Set((await tlist(['--label', `epic:${EPIC}`, '--all'])).filter((r) => r.kind !== 'epic').map((r) => r.id));
    if (!ids.size) die(`no beads found for epic:${EPIC} (is it seeded and labelled?).`);
    MEMBERSHIP = { epics: [{ slug: EPIC, ts: '' }], memberIds: new Map([[EPIC, ids]]), unassigned: new Set() };
    return [{ key: `epic:${EPIC}`, type: 'epic', slug: EPIC }];
  }
  if (await tbdAvailable()) {
    await refreshSnapshot(); await refreshMembership();
    const views = MEMBERSHIP.epics.map((e) => ({ key: `epic:${e.slug}`, type: 'epic', slug: e.slug }));
    if (MEMBERSHIP.unassigned.size) views.push({ key: 'unassigned', type: 'unassigned' });
    if (views.length) return views;
  }
  return [{ key: `fixture:${basename(DEFAULT_FIXTURE)}`, type: 'fixture', path: DEFAULT_FIXTURE }];
}

// ---- graph for a view → { nodes:[{id,title,status}], edges:[{from,to}] } -----
//   Spawn-free & synchronous: built entirely from SNAP + MEMBERSHIP, so drawing and
//   tab-switching are instant even while a background fetch is in flight.
function graphForView(view) {
  if (view.type === 'fixture') { const g = jparse(readFileSync(view.path, 'utf8'), { nodes: [], edges: [] }); return { nodes: g.nodes, edges: g.edges }; }
  const ids = view.type === 'unassigned' ? (MEMBERSHIP?.unassigned || new Set()) : (MEMBERSHIP?.memberIds.get(view.slug) || new Set());
  const nodes = [...ids].map((id) => SNAP.rows.get(id)).filter(Boolean).map((r) => ({ id: r.id, title: r.title, status: r.status }));
  const edges = [];
  for (const id of ids) for (const b of (SNAP.edges.get(id) || [])) if (ids.has(b)) edges.push({ from: b, to: id });
  return { nodes, edges };
}

// ---- Kahn waves + derived blocked status ------------------------------------
function analyze({ nodes, edges }) {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const blockers = new Map(nodes.map((n) => [n.id, []]));
  for (const { from, to } of edges) if (blockers.has(to)) blockers.get(to).push(from);
  const placed = new Set(); let remaining = nodes.map((n) => n.id); const waves = [];
  while (remaining.length) {
    const ready = remaining.filter((id) => blockers.get(id).every((b) => placed.has(b)));
    if (!ready.length) { waves.push(remaining); break; }               // cycle: show the rest rather than abort a live view
    ready.forEach((id) => placed.add(id));
    waves.push(ready);
    remaining = remaining.filter((id) => !ready.includes(id));
  }
  const isClosed = (id) => byId.get(id)?.status === 'closed';
  const glyphStatus = (n) =>
    n.status === 'closed' ? 'closed'
      : n.status === 'in_progress' ? 'in_progress'
        : blockers.get(n.id).some((b) => !isClosed(b)) ? 'blocked' : 'open';
  return { byId, blockers, waves, glyphStatus };
}

// ---- orchestration partition: .substrate/execution-state.json + group: labels ----
//   The orchestrator cuts the DAG into `group:<window-N>` context-budget windows (one
//   group-runner per window) and records the run in .substrate/execution-state.json before
//   the trunk squash. This pane visualises that partition: which flow the run used and each
//   window as a lane of live per-bead status. See agents-parallel-execution-doctrine.md
//   §Grouping & windows. Absent both sources → no pane (backward compatible).
const EXEC_STATE_PATH = join(process.cwd(), '.substrate', 'execution-state.json');
function loadExecState(slug) {          // the durable run-state for this epic, or null
  try { return jparse(readFileSync(EXEC_STATE_PATH, 'utf8'), {})[slug] || null; } catch { return null; }
}
const windowKey = (name) => { const m = /(\d+)/.exec(name); return m ? Number(m[1]) : name; };   // window-2 → 2

// Build the partition for a view from the run-state (authoritative) or, failing that, the
// `group:<window-N>` labels graph-spec stamped (the planned partition, pre-run).
function partitionForView(view) {
  if (view.type !== 'epic') return null;
  const state = loadExecState(view.slug);
  const ids = MEMBERSHIP?.memberIds.get(view.slug) || new Set();

  let windows = null, source = null, runId = null, deviations = 0;
  if (state && state.partition && Object.keys(state.partition).length) {
    source = 'run'; runId = state.runId || state['run-id'] || null;
    deviations = Array.isArray(state.deviations) ? state.deviations.length : 0;
    windows = Object.entries(state.partition).map(([name, beadIds]) => ({
      name,
      beads: (beadIds || []).map((id) => {
        const oc = (state.outcomes || {})[id] || {};
        return { id, status: oc.status || null, commit: oc.commit || null };
      }),
    }));
  } else {                                            // fallback: planned windows from group: labels
    const byWin = new Map();
    for (const id of ids) {
      const win = (SNAP.rows.get(id)?.labels || []).map((l) => /^group:(.+)$/.exec(l)?.[1]).find(Boolean);
      if (!win) continue;
      if (!byWin.has(win)) byWin.set(win, []);
      byWin.get(win).push({ id, status: null, commit: null });
    }
    if (!byWin.size) return null;                     // no partition anywhere → no pane
    source = 'planned';
    windows = [...byWin.entries()].map(([name, beads]) => ({ name, beads }));
  }

  windows.sort((a, b) => (windowKey(a.name) < windowKey(b.name) ? -1 : 1));
  const total = windows.reduce((n, w) => n + w.beads.length, 0);
  return { source, runId, deviations, windows, rung: classifyRung(windows, total) };
}

// Which execution flow the partition represents (the "rung"): one window ⇒ monolith; every
// window a singleton ⇒ per-bead fleet; otherwise file-adjacency grouping ⇒ group-windowed.
function classifyRung(windows, total) {
  if (windows.length <= 1) return total <= 1 ? 'single' : 'monolith';
  if (windows.every((w) => w.beads.length === 1)) return 'per-bead fleet';
  return 'group-windowed';
}

// Map a partition bead to a render glyph-status: run-state outcome first, else live SNAP status.
function paneStatus(bead) {
  switch (bead.status) {                              // execution-state outcome vocabulary
    case 'pass': return 'closed';
    case 'fail': return 'blocked';
    case 'open': return 'open';
    default: break;
  }
  const live = SNAP?.rows.get(bead.id)?.status;       // planned source → live tracker status
  return live === 'closed' ? 'closed' : live === 'in_progress' ? 'in_progress' : 'open';
}

// ---- render -----------------------------------------------------------------
const GLYPH = { closed: '✓', in_progress: '▶', open: '○', blocked: '⊘' };
const C = { closed: '\x1b[32m', in_progress: '\x1b[33m', open: '\x1b[90m', blocked: '\x1b[31m', dim: '\x1b[90m', title: '\x1b[37m', bold: '\x1b[1m', rev: '\x1b[7m', unrev: '\x1b[27m', reset: '\x1b[0m' };
const shortId = (id) => id.replace(/^[^-]+-/, '');    // drop the tbd prefix, whatever it is

function tabBar(views, active) {
  if (views.length < 2) return '';
  return views.map((v, i) => (i === active ? `${C.rev} ${v.key} ${C.unrev}` : `${C.dim} ${v.key} ${C.reset}`)).join(' ');
}

// Orchestration pane — the run's flow + each context-budget window as a lane of live status.
function orchestrationPane(part) {
  if (!part) return [];
  const lines = [''];
  const srcTag = part.source === 'run'
    ? `${C.closed}● run${C.reset}${part.runId ? ` ${C.dim}${part.runId}${C.reset}` : ''}`
    : `${C.dim}○ planned${C.reset}`;
  const dev = part.deviations ? `   ${C.in_progress}⚑ ${part.deviations} deviation${part.deviations === 1 ? '' : 's'}${C.reset}` : '';
  lines.push(`${C.bold}Orchestration${C.reset}   ${C.title}${part.rung}${C.reset}   ${srcTag}   ${C.dim}${part.windows.length} window${part.windows.length === 1 ? '' : 's'}${C.reset}${dev}`);
  for (const w of part.windows) {
    const strip = w.beads.map((b) => { const gs = paneStatus(b); return `${C[gs]}${GLYPH[gs]}${C.reset}`; }).join('');
    const ids = w.beads.map((b) => `${C[paneStatus(b)]}${shortId(b.id)}${C.reset}`).join(' ');
    const lane = w.beads.length > 1 ? 'lane' : 'solo';
    lines.push(` ${C.dim}▐${C.reset} ${C.title}${w.name}${C.reset} ${C.dim}[${lane}]${C.reset} ${strip}  ${ids}`);
  }
  lines.push(`${C.dim}windows run sequentially within a lane; parallel across file-disjoint lanes${C.reset}`);
  return lines;
}

function render(graph, meta, deltas) {
  const { byId, blockers, waves, glyphStatus } = analyze(graph);
  const spin = ['⟳', '⟲'][meta.frame % 2];
  const lines = [];
  lines.push('');
  const bar = tabBar(meta.views, meta.active);
  if (bar) { lines.push(bar); lines.push(''); }
  const nav = meta.views.length > 1 ? `${C.dim} · Tab/→ ←/Shift-Tab · q quit${C.reset}` : '';
  lines.push(`${C.bold}${meta.title}${C.reset}   ${C.dim}${spin} live · ${meta.updates} update${meta.updates === 1 ? '' : 's'}${C.reset}${nav}`);
  lines.push('');
  if (!graph.nodes.length) { lines.push(`${C.dim}(no beads)${C.reset}`); return lines.join('\n'); }
  let emitted = 0;
  let remainingWavesCount = 0;
  for (let w = 0; w < waves.length; w++) {
    const wave = waves[w];
    if (emitted >= MAX_NODES) { remainingWavesCount++; continue; }
    const par = wave.length > 1 ? 'PARALLEL' : 'sequential';
    lines.push(`${C.dim}── wave ${w + 1} · ${wave.length} ${par} ${'─'.repeat(Math.max(0, 28 - par.length))}${C.reset}`);
    let waveHasRemaining = false;
    for (let i = 0; i < wave.length; i++) {
      const id = wave[i];
      if (emitted >= MAX_NODES) { waveHasRemaining = true; continue; }
      emitted++;
      const n = byId.get(id);
      const gs = glyphStatus(n);
      const branch = wave.length === 1 ? '  ' : i === wave.length - 1 ? '└─' : '├─';
      const blk = blockers.get(id).map(shortId);
      const from = blk.length ? `  ${C.dim}← ${blk.join(', ')}${C.reset}` : '';
      let tag = '';
      if (deltas.appeared.has(id)) tag = `  ${C.in_progress}← NEW${C.reset}`;
      else if (deltas.doneNow.has(id)) tag = `  ${C.closed}✓ done${C.reset}`;
      lines.push(` ${C.dim}${branch}${C.reset} ${C[gs]}${GLYPH[gs]} ${id}${C.reset}  ${C.title}${n.title}${C.reset}${from}${tag}`);
    }
    if (waveHasRemaining) remainingWavesCount++;
    lines.push('');
  }
  const remaining = graph.nodes.length - emitted;
  if (remaining > 0) lines.push(`${C.dim}  … +${remaining} more bead${remaining === 1 ? '' : 's'} (${remainingWavesCount} wave${remainingWavesCount === 1 ? '' : 's'}) — raise with --max${C.reset}`);
  lines.push(`${C.closed}✓ closed${C.reset}   ${C.in_progress}▶ in_progress${C.reset}   ${C.open}○ open${C.reset}   ${C.blocked}⊘ blocked${C.reset}`);
  lines.push(`${C.dim}waves: ${waves.map((w) => w.length).join(' → ')}   (${graph.nodes.length} beads)${C.reset}`);
  for (const l of orchestrationPane(meta.partition)) lines.push(l);
  return lines.join('\n');
}

// ---- liveness helpers -------------------------------------------------------
function sourceMtime(view) {   // fixture views only; tbd views are content-polled
  if (view.type !== 'fixture') return 0;
  try { return statSync(view.path).mtimeMs; } catch { return 0; }
}
const statusMap = (graph) => new Map(graph.nodes.map((n) => [n.id, n.status]));
const titleOf = (view) => view.type === 'fixture' ? `fixture · ${basename(view.path)}` : view.type === 'unassigned' ? 'unassigned beads' : `epic:${view.slug}`;

// ---- terminal / input -------------------------------------------------------
let raw = false;
function restore() { try { if (raw) process.stdin.setRawMode(false); process.stdout.write('\x1b[?25h'); } catch { /* noop */ } }
process.on('exit', restore);
['SIGINT', 'SIGTERM'].forEach((s) => process.on(s, () => { restore(); process.stdout.write('\n'); process.exit(0); }));

// ---- render state -----------------------------------------------------------
let active = 0;
let updates = 0;
let frame = 0;
const prevStatus = new Map();               // view.key → Map(id→status), drives NEW/done deltas

function draw() {
  const view = VIEWS[active];
  const graph = graphForView(view);
  const cur = statusMap(graph);
  const prev = prevStatus.get(view.key);
  const appeared = new Set(), doneNow = new Set();
  if (prev) for (const [id, st] of cur) {
    if (!prev.has(id)) appeared.add(id);
    else if (st === 'closed' && prev.get(id) !== 'closed') doneNow.add(id);
  }
  prevStatus.set(view.key, cur);
  const partition = partitionForView(view);
  const out = render(graph, { title: titleOf(view), views: VIEWS, active, frame, updates, partition }, { appeared, doneNow });
  process.stdout.write(ONCE ? out + '\n' : `\x1b[2J\x1b[H${out}\n`);
  frame++;
}

// ---- main -------------------------------------------------------------------
if (!ONCE && !LIST_VIEWS) process.stdout.write('bead-tui: discovering beads…\n');   // startup can take a few seconds
let VIEWS = await resolveViews();

if (LIST_VIEWS) { process.stdout.write(VIEWS.map((v) => v.key).join('\n') + '\n'); process.exit(0); }
if (ONCE) { draw(); process.exit(0); }

// interactive
process.stdout.write('\x1b[?25l');
draw();
let lastContentSig = contentSig();
let lastIdSig = idSignature();
let lastMtime = sourceMtime(VIEWS[active]);

if (process.stdin.isTTY) {                  // instant, because the event loop is never blocked
  raw = true; process.stdin.setRawMode(true); process.stdin.resume(); process.stdin.setEncoding('utf8');
  const nav = (d) => { active = Math.max(0, Math.min(VIEWS.length - 1, active + d)); draw(); };
  process.stdin.on('data', (k) => {
    if (k === '\t' || k === '\x1b[C') nav(+1);              // Tab / →
    else if (k === '\x1b[Z' || k === '\x1b[D') nav(-1);      // Shift-Tab / ←
    else if (k === 'q' || k === '\x03') { restore(); process.stdout.write('\n'); process.exit(0); }
  });
}

// self-paced async poll — fetches in the background; redraws only on change
(async function poll() {
  for (;;) {
    const view = VIEWS[active];
    try {
      let changed = false;
      if (view.type === 'fixture') {
        const m = sourceMtime(view); if (m !== lastMtime) { lastMtime = m; updates++; changed = true; }
      } else {
        await refreshSnapshot();
        const sig = contentSig();
        if (sig !== lastContentSig) {
          lastContentSig = sig; updates++; changed = true;
          if (!EPIC && idSignature() !== lastIdSig) {          // beads added/removed → rebuild tabs+membership
            const activeKey = view.key;
            VIEWS = await resolveViews();
            lastIdSig = idSignature(); lastContentSig = contentSig();
            const idx = VIEWS.findIndex((v) => v.key === activeKey);
            active = idx >= 0 ? idx : 0;
          }
        }
      }
      if (changed) draw();
    } catch { /* transient tbd error — keep the last good frame */ }
    await sleep(INTERVAL);
  }
})();
