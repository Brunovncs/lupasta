// `bun scripts/make-bench-fixture.ts 10000 100000 500000`
// Writes fixtures/bench-<N>: a deterministic tree of N entries (dirs + empty files, fan-out
// 8 dirs / 40 files per directory) plus `wide/`, one flat directory with N/10 (≤ 20k) files.
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const WORDS = ["router", "orthogonal", "connector", "screen", "geometry", "browser", "model", "view", "editor", "tree", "index", "search", "watcher", "layout", "camera", "palette", "fixture", "render", "scene", "motion", "column", "preview", "lane", "caret"];
const EXTS = ["swift", "rs", "ts", "md", "txt", "png", "csv", "pdf", "json", "log"];

let seed = 42;
const rnd = () => ((seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff);
const word = () => WORDS[Math.floor(rnd() * WORDS.length)];

export function buildBench(root: string, total: number) {
  rmSync(root, { recursive: true, force: true });
  mkdirSync(root, { recursive: true });
  let count = 0;
  const wideN = Math.min(20000, Math.floor(total / 10));
  const wide = join(root, "wide");
  mkdirSync(wide);
  count++;
  for (let i = 0; i < wideN; i++, count++) writeFileSync(join(wide, `${word()}-${word()}-${i}.${EXTS[i % EXTS.length]}`), "");
  const queue = [root];
  while (count < total && queue.length) {
    const dir = queue.shift()!;
    for (let d = 0; d < 8 && count < total; d++, count++) {
      const p = join(dir, `${word()}-${d}`);
      mkdirSync(p);
      queue.push(p);
    }
    for (let f = 0; f < 40 && count < total; f++, count++) writeFileSync(join(dir, `${word()}${word()[0].toUpperCase()}${word().slice(1)}-${f}.${EXTS[f % EXTS.length]}`), "");
  }
  return count;
}

if (import.meta.main) {
  const sizes = process.argv.slice(2).map(Number).filter(Boolean);
  for (const n of sizes.length ? sizes : [10000, 100000, 500000]) {
    const root = resolve(import.meta.dir, `../fixtures/bench-${n >= 1000 ? `${n / 1000}k` : n}`);
    const t0 = performance.now();
    const made = buildBench(root, n);
    console.log(`${root}: ${made} entries in ${((performance.now() - t0) / 1000).toFixed(1)} s`);
  }
}
