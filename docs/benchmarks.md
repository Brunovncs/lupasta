# Benchmarks

Measured on one Windows 11 machine (release builds). Reproduce with:

```sh
bun run fixture:bench                               # writes fixtures/bench-10k, -100k, -500k
bun run bench --root fixtures/bench-500k            # core: index, search, watcher, memory
bun tests/e2e/bench.ts                              # app: startup, expansion, frame pacing
```

The bench fixtures are deterministic trees of empty files (8 folders and 40 files per folder)
plus `wide/`, one flat folder with up to 20,000 files.

These runs predate a change that dropped paths from the FTS5 index (names only; paths are
matched by the in-memory fuzzy scan). That change shrank the database from 7.4 / 70.8 / 372 MB
to 3.3 / 33.9 / 161 MB for 10k / 100k / 500k entries. Timings have not been re-measured on an
idle machine since.

## Core

### 10k

| metric | result |
|---|---|
| list_dir(root) cold | 0.20 ms (49 entries) |
| list_dir(first child dir) | 0.14 ms (48 entries) |
| list_dir warm p50 | 0.08 ms |
| walk only (parallel, with metadata) | 37.02 ms (10001 entries) |
| index build (walk + SQLite + FTS5) | 137 ms, 10000 entries, db 7.4 MB |
| resync scan (nothing changed) | 19.60 ms (10000 entries) |
| startup: open index + load corpus | 9.37 ms (10000 items) |
| search "router" warm p50 / p95 | 1.09 ms / 1.38 ms (fts, 40 hits, 1473 candidates; top: router-0, router-5, router-2) |
| search "rtr" warm p50 / p95 | 2.10 ms / 2.85 ms (scan, 40 hits, 10000 candidates; top: router-tree-743.md, render-tree-206.csv, router-tree-594.txt) |
| search "file browser" warm p50 / p95 | 0.29 ms / 0.35 ms (scan, 3 hits, 10000 candidates; top: fixtureSalette-32.ts, browserLearch-21.rs, fixtureLixture-5.png) |
| search "readme" warm p50 / p95 | 0.76 ms / 1.55 ms (scan, 9 hits, 10000 candidates; top: indexLamera-22.ts, renderMearch-30.swift, indexMree-19.log) |
| search "a" warm p50 / p95 | 1.62 ms / 1.82 ms (scan, 40 hits, 10000 candidates; top: lane-5, lane-2, lane-1) |
| search "zzqxj" warm p50 / p95 | 0.42 ms / 1.22 ms (scan, 0 hits, 10000 candidates; top: ) |
| search "mordor roster" warm p50 / p95 | 0.54 ms / 0.70 ms (scan, 8 hits, 10000 candidates; top: browserTamera-32.ts, treeRditor-13.md, editorSatcher-16.csv) |
| incremental: create -> index+corpus | 1.52 ms |
| incremental: delete -> index+corpus | 0.21 ms |
| watcher: write -> coalesced batch | 135.92 ms |
| memory: baseline / after build / after reload | 4.2 / 14.0 / 11.1 MB |

### 100k

| metric | result |
|---|---|
| list_dir(root) cold | 0.13 ms (49 entries) |
| list_dir(first child dir) | 0.10 ms (48 entries) |
| list_dir warm p50 | 0.05 ms |
| walk only (parallel, with metadata) | 145.15 ms (100001 entries) |
| index build (walk + SQLite + FTS5) | 3179 ms, 100000 entries, db 70.8 MB |
| resync scan (nothing changed) | 160.75 ms (100000 entries) |
| startup: open index + load corpus | 62.30 ms (100000 items) |
| search "router" warm p50 / p95 | 12.74 ms / 15.07 ms (fts, 40 hits, 11592 candidates; top: router-7, router-7, router-0) |
| search "rtr" warm p50 / p95 | 0.20 ms / 0.23 ms (fts, 40 hits, 170 candidates; top: routerTree-32.ts, routerTree-4.txt, renderTreview-33.md) |
| search "file browser" warm p50 / p95 | 1.45 ms / 1.95 ms (scan, 40 hits, 100000 candidates; top: lane-1, lane-7, lane-6) |
| search "readme" warm p50 / p95 | 1.76 ms / 18.25 ms (scan, 40 hits, 100000 candidates; top: model-2, camera-3, camera-0) |
| search "a" warm p50 / p95 | 3.75 ms / 35.90 ms (scan, 40 hits, 100000 candidates; top: caret-7, lane-5, camera-3) |
| search "zzqxj" warm p50 / p95 | 0.94 ms / 1.08 ms (scan, 0 hits, 100000 candidates; top: ) |
| search "mordor roster" warm p50 / p95 | 2.00 ms / 2.25 ms (scan, 40 hits, 100000 candidates; top: sceneCouter-4.txt, orthogonalSatcher-12.ts, indexSatcher-25.png) |
| incremental: create -> index+corpus | 2.41 ms |
| incremental: delete -> index+corpus | 0.20 ms |
| watcher: write -> coalesced batch | 125.91 ms |
| memory: baseline / after build / after reload | 4.2 / 27.9 / 23.7 MB |

### 500k

| metric | result |
|---|---|
| list_dir(root) cold | 0.11 ms (49 entries) |
| list_dir(first child dir) | 0.08 ms (48 entries) |
| list_dir warm p50 | 0.05 ms |
| walk only (parallel, with metadata) | 1649.99 ms (500001 entries) |
| index build (walk + SQLite + FTS5) | 29699 ms, 500000 entries, db 372.4 MB |
| resync scan (nothing changed) | 1368.91 ms (500000 entries) |
| startup: open index + load corpus | 353.72 ms (500000 items) |
| search "router" warm p50 / p95 | 25.07 ms / 35.80 ms (fts, 40 hits, 20000 candidates; top: router-6, router-3, router-0) |
| search "rtr" warm p50 / p95 | 0.86 ms / 1.16 ms (fts, 40 hits, 923 candidates; top: routerTree-3.md, renderTrowser-13.md, routerTrowser-21.rs) |
| search "file browser" warm p50 / p95 | 7.15 ms / 13.83 ms (scan, 40 hits, 500000 candidates; top: editor-2, lane-7, tree-2) |
| search "readme" warm p50 / p95 | 8.38 ms / 34.42 ms (scan, 40 hits, 500000 candidates; top: model-0, model-0, model-4) |
| search "a" warm p50 / p95 | 20.04 ms / 44.13 ms (scan, 40 hits, 500000 candidates; top: caret-1, lane-3, lane-7) |
| search "zzqxj" warm p50 / p95 | 5.41 ms / 16.94 ms (scan, 0 hits, 500000 candidates; top: ) |
| search "mordor roster" warm p50 / p95 | 13.74 ms / 39.23 ms (scan, 40 hits, 500000 candidates; top: searchCouter-14.txt, orthogonalSouter-13.md, routerSouter-6.csv) |
| incremental: create -> index+corpus | 2.60 ms |
| incremental: delete -> index+corpus | 0.36 ms |
| watcher: write -> coalesced batch | 131.77 ms |
| memory: baseline / after build / after reload | 4.2 / 87.8 / 80.1 MB |

## App
Windows 11, WebView2, 240 Hz monitor (frame interval ≈ 4.2 ms), 906×672 window at DPR 2.
"select widest dir" = the directory with the most children (`wide/` in the bench fixtures):
its listing arrives and its children appear as a preview column. "enter it" = ArrowRight into
that directory, so the whole 20k-entry list becomes the selection column.

| fixture | startup (spawn→first layout) | select widest dir | enter it | layout cost while navigating | frame pacing (ArrowDown ×25) | search round-trip p50 | memory |
|---|---|---|---|---|---|---|---|
| visual/Users | 1219.2 ms | 10 kids: list 0.4 ms, frame 0.9 ms | 36 laid out / 38 in DOM, frame 2.0 ms | layout p50 0.6 / max 0.7 ms | frames p50 4.2 / p95 4.3 / max 4.3 ms, 0 >20ms of 471 | router 1.3, rtr 1.2, readme 1.2, a 1.1 ms | heap 4.7 MB, process tree 392.7 MB |
| bench-10k | 869.8 ms | 1000 kids: list 1.8 ms, frame 4.0 ms | 290 laid out / 78 in DOM, frame 1.6 ms | layout p50 0.6 / max 1.0 ms | frames p50 4.2 / p95 4.3 / max 4.7 ms, 0 >20ms of 472 | router 4.0, rtr 1.5, readme 2.2, a 2.7 ms | heap 9.2 MB, process tree 427.3 MB |
| bench-100k | 912.3 ms | 10000 kids: list 3.5 ms, frame 4.9 ms | 290 laid out / 78 in DOM, frame 2.7 ms | layout p50 1.9 / max 3.7 ms | frames p50 4.2 / p95 4.3 / max 4.3 ms, 0 >20ms of 482 | router 6.7, rtr 11.0, readme 4.2, a 5.9 ms | heap 13.1 MB, process tree 453.3 MB |
| bench-500k | 910.6 ms | 20000 kids: list 7.1 ms, frame 7.9 ms | 290 laid out / 78 in DOM, frame 6.8 ms | layout p50 4.0 / max 6.2 ms | frames p50 4.2 / p95 4.3 / max 4.3 ms, 0 >20ms of 498 | router 11.7, rtr 15.6, readme 6.7, a 14.6 ms | heap 19.7 MB, process tree 461.1 MB |

Before layout virtualization (same machine, same fixtures) the 500k/20k-column row was:
layout p50 17.0 / max 27.8 ms per keystroke, 8 frames >20 ms of 506, JS heap 86.9 MB,
first frame after expanding `wide/` 40.3 ms.

"process tree" is lupasta.exe plus every WebView2 child process (browser, GPU, renderer,
utility); the WebView2 runtime alone accounts for ~350 MB of it on this machine.
