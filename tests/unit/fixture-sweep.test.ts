// Loads the whole visual fixture from disk and checks layout/routing invariants for every
// possible selection: no overlapping names, orthogonal connectors, no connector crossings.
import { describe, expect, test } from "bun:test";
import { existsSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { TreeModel, FLAG_DIR, type EntryTuple } from "../../src/tree/model";
import { computeLayout } from "../../src/scene/layout";
import { crossings, isOrthogonal, routeLayout, textHits } from "../../src/scene/router";
import { metrics as m } from "../../src/scene/metrics";
import { buildFixture } from "../../scripts/make-fixture";

const ROOT = resolve(import.meta.dir, "../../fixtures/visual/Users");

function loadAll(): TreeModel {
  if (!existsSync(ROOT)) buildFixture(ROOT);
  const model = new TreeModel("Users");
  const walk = (rel: string) => {
    const abs = join(ROOT, rel);
    const names = readdirSync(abs).sort((a, b) => {
      const x = a.toLowerCase();
      const y = b.toLowerCase();
      return x < y ? -1 : x > y ? 1 : a < b ? -1 : 1;
    });
    const entries: EntryTuple[] = names.map((n) => {
      const st = statSync(join(abs, n));
      return [n, (st.isDirectory() ? FLAG_DIR : 0) | (n.startsWith(".") ? 2 : 0), st.mtimeMs, st.size];
    });
    model.ingest({ path: rel, entries });
    for (const [n, flags] of entries) if (flags & FLAG_DIR) walk(rel ? `${rel}/${n}` : n);
  };
  walk("");
  return model;
}

describe("fixture sweep", () => {
  const model = loadAll();
  const ids = [...model.nodes.keys()].filter((id) => id !== "");

  test(`every selection (${ids.length}) lays out without overlaps, crossings or wires through text`, () => {
    const problems: string[] = [];
    for (const sel of ids) {
      const L = computeLayout(model, sel, m, Date.now());
      const rows = new Map<number, { x: number; w: number; id: string }[]>();
      for (const n of L.nodes.values()) {
        const list = rows.get(n.y) ?? [];
        for (const o of list) if (n.x < o.x + o.w && o.x < n.x + n.w) problems.push(`${sel}: "${n.id}" overlaps "${o.id}"`);
        list.push(n);
        rows.set(n.y, list);
      }
      const routes = routeLayout(L, (id) => L.nodes.get(id) ?? null, m);
      for (const r of routes) if (!isOrthogonal(r.points)) problems.push(`${sel}: ${r.id} not orthogonal`);
      for (const [a, b] of crossings(routes)) problems.push(`${sel}: ${a} crosses ${b}`);
      for (const hit of textHits(routes, [...L.nodes.values()], m)) problems.push(`${sel}: ${hit}`);
    }
    expect(problems.slice(0, 20)).toEqual([]);
  });
});
