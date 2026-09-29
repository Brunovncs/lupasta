import { describe, expect, test } from "bun:test";
import { crossings, isOrthogonal, routeGroups, routeLayout, routePath, toPathD, type GroupGeom } from "../../src/scene/router";
import { computeLayout } from "../../src/scene/layout";
import { NOW, REF_TREE, modelFrom } from "./helpers";
import { metrics as m } from "../../src/scene/metrics";

const box = (x: number, y: number, w = 60) => ({ x, y, w });

describe("path connectors", () => {
  test("straight on the focus row, from name end to child start", () => {
    const r = routePath("p", box(0, 0, 60), box(100, 0), m);
    expect(r.points).toEqual([[60 + m.connGapL, m.connectorY], [100 - m.connGapR, m.connectorY]]);
    expect(r.kind).toBe("path");
  });
});

describe("preview connectors", () => {
  const g = (id: string, py: number, top: number, rows: number, siblings: { x: number; y: number; w: number }[] = []): GroupGeom => ({
    id, level: 0, parent: box(0, py, 60), first: box(200, top), last: box(200, top + (rows - 1) * m.rowH), siblings, targets: [], baseX: 200,
  });

  test("straight when the group spans the parent row", () => {
    const [r] = routeGroups([g("a", 0, -32, 5)], m);
    expect(r.points.length).toBe(2);
    expect(r.points[0][1]).toBe(r.points[1][1]);
  });

  test("elbow is orthogonal and enters the group edge nearest to the parent", () => {
    const [down] = routeGroups([g("d", 0, 64, 3)], m);
    expect(isOrthogonal(down.points)).toBe(true);
    expect(down.points.at(-1)![1]).toBe(64 + m.connectorY - m.enterOffset);
    const [up] = routeGroups([g("u", 0, -128, 3)], m);
    expect(up.points.at(-1)![1]).toBe(-128 + 2 * m.rowH + m.connectorY + m.enterOffset);
  });

  test("runs next to the parent when free, next to the children when a longer name is in the way", () => {
    const [free] = routeGroups([g("f", 0, 64, 2)], m);
    expect(free.points[1][0]).toBe(60 + m.connGapL + m.nearParent);
    const [blocked] = routeGroups([g("b", 0, 64, 2, [box(0, 16, 120)])], m);
    expect(blocked.points[1][0]).toBeGreaterThan(120);
    expect(blocked.points[1][0]).toBeLessThan(200);
  });

  test("lanes: of two edges heading down, the upper parent runs inner", () => {
    const routes = routeGroups(
      [
        { id: "hi", level: 0, parent: box(0, 0, 60), first: box(200, 48), last: box(200, 64), siblings: [box(0, 16, 120)], targets: [], baseX: 200 },
        { id: "lo", level: 0, parent: box(0, 16, 120), first: box(200, 96), last: box(200, 112), siblings: [box(0, 0, 60)], targets: [], baseX: 200 },
      ],
      m,
    );
    const hi = routes.find((r) => r.id === "hi")!;
    const lo = routes.find((r) => r.id === "lo")!;
    expect(hi.points[1][0]).toBeGreaterThan(lo.points[1][0]);
    expect(crossings(routes)).toEqual([]);
  });

  test("real layouts route without crossings", () => {
    const FB = "drcode/file-browser";
    const model = modelFrom(REF_TREE, ["", "drcode", FB, ...[".git", "build", "scripts", "Sources", "test-folders", "Tests"].map((d) => `${FB}/${d}`), `${FB}/test-folders/02 Moria`, `${FB}/test-folders/05 Mordor`, `${FB}/test-folders/10 Eagles (Availability Pending)`]);
    for (const sel of [`${FB}/temp_0.md`, `${FB}/README.md`, `${FB}/Tests`, `${FB}/test-folders`, `${FB}/scripts`, `${FB}/.git`]) {
      const L = computeLayout(model, sel, m, NOW);
      const routes = routeLayout(L, (id) => L.nodes.get(id) ?? null, m);
      expect(routes.length).toBeGreaterThan(0);
      for (const r of routes) expect(isOrthogonal(r.points)).toBe(true);
      expect({ sel, crossings: crossings(routes) }).toEqual({ sel, crossings: [] });
    }
  });
});
describe("svg path", () => {
  test("rounded corners use quadratic elbows, straight lines stay lines", () => {
    expect(toPathD([[0, 0], [10, 0]], 5)).toBe("M0 0L10 0");
    const d = toPathD([[0, 0], [20, 0], [20, 20], [40, 20]], 5);
    expect(d).toBe("M0 0L15 0Q20 0 20 5L20 15Q20 20 25 20L40 20");
  });
  test("radius shrinks on short segments", () => {
    expect(toPathD([[0, 0], [4, 0], [4, 4]], 5)).toBe("M0 0L2 0Q4 0 4 2L4 4");
  });
});
