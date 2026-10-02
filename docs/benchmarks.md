# Benchmarks

Measured on one Windows 11 machine (release builds). Reproduce with:

```sh
cargo run --release --example bench_fixture                # writes fixtures/bench-10k, -100k, -500k
cargo run --release --example bench -- --root fixtures/bench-500k   # core: index, search, watcher, memory
cargo run --release --example layout_bench                 # layout + routing per keystroke
```

(Without the Windows SDK, use `--profile local` instead of `--release`; see the README.)

The bench fixtures are deterministic trees of empty files (8 folders and 40 files per folder)
plus `wide/`, one flat folder with up to 20,000 files. The generator produces the same names
as the TypeScript script the numbers below were first measured with.

These runs predate a change that dropped paths from the FTS5 index (names only; paths are
matched by the in-memory fuzzy scan). That change shrank the database from 7.4 / 70.8 / 372 MB
to 3.3 / 33.9 / 161 MB for 10k / 100k / 500k entries. Timings have not been re-measured on an
idle machine since. The core is the same code in the GPUI build.

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

Windows 11, GPUI build (v0.2.0) against the Tauri 2 + WebView2 build (v0.1.0), both release
builds, a 906×672 window at 125 % scaling, measured 20 s after start-up with a fresh data folder
(so the index has been built). "Process tree" is the sum over lupasta.exe and every child
process; the Tauri build runs six WebView2 processes besides its own.

| fixture | GPUI: processes, working set / private | Tauri: processes, working set / private |
|---|---|---|
| visual/Users | 1, 72 / 91 MB | 7, 374 / 217 MB |
| bench-10k | 1, 82 / 108 MB | 7, 385 / 290 MB |
| bench-100k | 1, 88 / 114 MB | 7, 409 / 254 MB |
| bench-500k | 1, 147 / 174 MB | 7, 482 / 339 MB |

Layout and connector routing per keystroke in a 20,000-entry folder (`layout_bench`, a
synthetic folder with every 50th entry a sub-folder of 20 files): median 3.3 ms, p95 4.7 ms.
The Tauri build measured layout p50 4.0 / max 6.2 ms for the same size of folder in the app
(bench-500k, `wide/` entered), so the two are comparable rather than identical setups.

The Tauri build's frame-pacing, startup and per-step timings came from driving WebView2 through
its DevTools protocol; that harness has no GPUI counterpart yet, so those columns are not
reproduced here.
