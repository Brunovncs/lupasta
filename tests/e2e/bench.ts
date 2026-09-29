// Frontend/app benchmark against the release build: `bun tests/e2e/bench.ts [fixture…]`
// Startup, directory expansion, layout cost, frame pacing while navigating, search round-trip
// and memory (JS heap + the whole process tree's working set).
import { execSync } from "node:child_process";
import { join } from "node:path";
import { ROOT, launch, settle } from "./harness";
import type { Page } from "playwright-core";

const fixtures = process.argv.slice(2).length ? process.argv.slice(2) : ["visual/Users", "bench-10k", "bench-100k", "bench-500k"];

const pct = (v: number[], p: number) => {
  const s = [...v].sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.round((s.length - 1) * p))];
};
const f = (n: number) => n.toFixed(1);

function treeWorkingSetMB(pid: number): number {
  const ps = `$all = Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,WorkingSetSize; $ids = @(${pid}); do { $n = $ids.Count; $ids = @($ids + ($all | Where-Object { $ids -contains $_.ParentProcessId } | ForEach-Object { $_.ProcessId })) | Select-Object -Unique } while ($ids.Count -ne $n); ($all | Where-Object { $ids -contains $_.ProcessId } | Measure-Object WorkingSetSize -Sum).Sum`;
  const out = execSync(`powershell -NoProfile -Command "${ps.replace(/"/g, '\\"')}"`, { encoding: "utf8" });
  return Number(out.trim()) / 1048576;
}

async function framePacing(page: Page, steps: number, key: string) {
  return page.evaluate(
    async ({ steps, key }) => {
      const w = window as any;
      const deltas: number[] = [];
      let last = performance.now();
      let running = true;
      const tick = (t: number) => {
        deltas.push(t - last);
        last = t;
        if (running) requestAnimationFrame(tick);
      };
      requestAnimationFrame(tick);
      const layoutMs: number[] = [];
      const orig = w.__lupasta.ctl.relayout.bind(w.__lupasta.ctl);
      w.__lupasta.ctl.relayout = (i?: boolean) => {
        const t = performance.now();
        orig(i);
        layoutMs.push(performance.now() - t);
      };
      for (let i = 0; i < steps; i++) {
        window.dispatchEvent(new KeyboardEvent("keydown", { key }));
        await new Promise((r) => setTimeout(r, 60));
      }
      await new Promise((r) => setTimeout(r, 400));
      running = false;
      w.__lupasta.ctl.relayout = orig;
      return { deltas: deltas.slice(1), layoutMs };
    },
    { steps, key },
  );
}

const rows: string[] = [];
for (const fx of fixtures) {
  const root = join(ROOT, "fixtures", fx);
  const t0 = performance.now();
  const app = await launch({ root });
  const startup = performance.now() - t0; // spawn → first layout rendered (includes WebView2 boot)
  const { page } = app;
  try {
    const top = await page.evaluate(() => (window as any).__lupasta.ctl.model.children("").map((n: any) => n.id));
    const wide = top.includes("wide") ? "wide" : top[Math.floor(top.length / 2)];

    // Expansion: select the widest directory (its children become a preview column), then enter.
    const expand = await page.evaluate(async (id) => {
      const w = window as any;
      const t = performance.now();
      w.__lupasta.ctl.select(id);
      await w.__lupasta.ctl.ensurePreview();
      const listed = performance.now() - t;
      await new Promise((r) => requestAnimationFrame(() => r(null)));
      const firstFrame = performance.now() - t;
      return { listed, firstFrame, kids: w.__lupasta.ctl.model.children(id)?.length ?? 0 };
    }, wide);
    await settle(page);
    const enter = await page.evaluate(async () => {
      const w = window as any;
      const t = performance.now();
      await w.__lupasta.ctl.enter();
      await new Promise((r) => requestAnimationFrame(() => r(null)));
      return { firstFrame: performance.now() - t, nodes: w.__lupasta.ctl.layout.nodes.size, dom: document.querySelectorAll(".node").length };
    });
    await settle(page);

    // Frame pacing while stepping through the (possibly 20k-long) column.
    const pace = await framePacing(page, 25, "ArrowDown");
    await settle(page);

    // Search round-trip from the renderer (index must be ready).
    await page.waitForFunction(async () => (await (window as any).__lupasta.backend.indexStatus()).state === "ready", null, { timeout: 180000, polling: 250 });
    const search = await page.evaluate(async () => {
      const b = (window as any).__lupasta.backend;
      const out: Record<string, number> = {};
      for (const q of ["router", "rtr", "readme", "a"]) {
        const ts: number[] = [];
        for (let i = 0; i < 10; i++) {
          const t = performance.now();
          await b.searchFiles(q, 12);
          ts.push(performance.now() - t);
        }
        ts.sort((x, y) => x - y);
        out[q] = ts[5];
      }
      return out;
    });
    const heap = await page.evaluate(() => ((performance as any).memory?.usedJSHeapSize ?? 0) / 1048576);
    const ws = treeWorkingSetMB(app.proc.pid!);

    rows.push(
      `| ${fx} | ${f(startup)} ms | ${expand.kids} kids: list ${f(expand.listed)} ms, frame ${f(expand.firstFrame)} ms | ${enter.nodes} laid out / ${enter.dom} in DOM, frame ${f(enter.firstFrame)} ms | layout p50 ${f(pct(pace.layoutMs, 0.5))} / max ${f(Math.max(...pace.layoutMs))} ms | frames p50 ${f(pct(pace.deltas, 0.5))} / p95 ${f(pct(pace.deltas, 0.95))} / max ${f(Math.max(...pace.deltas))} ms, ${pace.deltas.filter((d) => d > 20).length} >20ms of ${pace.deltas.length} | ${Object.entries(search).map(([q, v]) => `${q} ${f(v)}`).join(", ")} ms | heap ${f(heap)} MB, process tree ${f(ws)} MB |`,
    );
    console.log(rows.at(-1));
  } finally {
    await app.close();
  }
}
console.log(
  "\n| fixture | startup (spawn→first layout) | select widest dir | enter it | layout cost while navigating | frame pacing (ArrowDown ×25) | search round-trip p50 | memory |\n|---|---|---|---|---|---|---|---|\n" +
    rows.join("\n"),
);
