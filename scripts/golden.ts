// Regenerates tests/golden/layout.json from the TypeScript implementation this Rust code was
// ported from (lupasta v0.1.0, the Tauri + Svelte build): runs its layout + router over the
// fixture for every selection and prints the geometry.
//
//   git worktree add ../lupasta-ts v0.1.0
//   cargo run --example fixture
//   bun scripts/golden.ts ../lupasta-ts fixtures/visual/Users > tests/golden/layout.json
import { readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
const SRC = resolve(process.argv[2], "src");
const { TreeModel, FLAG_DIR } = await import(`${SRC}/tree/model.ts`);
const { computeLayout } = await import(`${SRC}/scene/layout.ts`);
const { routeLayout } = await import(`${SRC}/scene/router.ts`);
const { metrics: m } = await import(`${SRC}/scene/metrics.ts`);

const ROOT = resolve(process.argv[3]);
const model = new TreeModel("Users");
const walk = (rel: string) => {
  const abs = join(ROOT, rel);
  const names = readdirSync(abs).sort((a, b) => {
    const x = a.toLowerCase();
    const y = b.toLowerCase();
    return x < y ? -1 : x > y ? 1 : a < b ? -1 : 1;
  });
  const entries = names.map((n) => {
    const st = statSync(join(abs, n));
    return [n, (st.isDirectory() ? FLAG_DIR : 0) | (n.startsWith(".") ? 2 : 0), st.mtimeMs, st.size];
  });
  model.ingest({ path: rel, entries });
  for (const [n, flags] of entries) if ((flags as number) & FLAG_DIR) walk(rel ? `${rel}/${n}` : (n as string));
};
walk("");
const r2 = (v: number) => Math.round(v * 100) / 100;
const out: Record<string, unknown> = {};
const ids = [...model.nodes.keys()].filter((id) => id !== "").sort();
for (const sel of ids) {
  const L = computeLayout(model, sel, m, Date.now());
  const nodes = [...L.nodes.values()].map((n) => [n.id, r2(n.x), r2(n.y), r2(n.w), r2(n.box), n.label]).sort((a, b) => (a[0] < b[0] ? -1 : 1));
  const routes = routeLayout(L, (id: string) => L.nodes.get(id) ?? null, m).map((r: any) => [r.id, r.points.map((p: number[]) => [r2(p[0]), r2(p[1])])]).sort((a: any, b: any) => (a[0] < b[0] ? -1 : 1));
  out[sel] = { maxX: r2(L.maxX), nodes, routes };
}
console.log(JSON.stringify(out));
