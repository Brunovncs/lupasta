import { describe, expect, test } from "bun:test";
import { Animator, easeOutCubic } from "../../src/scene/animator";
import type { Layout, LayoutNode } from "../../src/scene/layout";

const node = (id: string, parentId: string, x: number, y: number): LayoutNode => ({
  id, parentId, label: id, x, y, w: 10, box: 20, cut: false, col: 0, hue: "path", color: "#fff", isDir: false, selected: false, onPath: false,
});
const layout = (...nodes: LayoutNode[]): Layout => ({
  selectedId: nodes[0].id, nodes: new Map(nodes.map((n) => [n.id, n])), columns: [], groups: [], pathEdges: [], minX: 0, maxX: 0,
});

describe("animator", () => {
  test("first layout is placed instantly", () => {
    const a = new Animator(300);
    a.setTarget(layout(node("a", "", 0, 0)), { x: 5, y: 6 }, 0);
    expect(a.tracks.get("a")!.cur).toEqual({ x: 0, y: 0, o: 1 });
    expect(a.cam).toEqual({ x: 5, y: 6 });
  });

  test("existing nodes slide with easing; new nodes emerge from their parent and fade in", () => {
    const a = new Animator(300);
    a.setTarget(layout(node("p", "", 0, 0)), { x: 0, y: 0 }, 0);
    a.setTarget(layout(node("p", "", 0, 100), node("c", "p", 50, 132)), { x: 10, y: 0 }, 1000);
    expect(a.tracks.get("c")!.cur).toEqual({ x: 0, y: 0, o: 0 }); // starts at the parent
    a.tick(1150);
    const e = easeOutCubic(0.5);
    expect(a.tracks.get("p")!.cur.y).toBeCloseTo(100 * e);
    expect(a.tracks.get("c")!.cur.x).toBeCloseTo(50 * e);
    expect(a.tracks.get("c")!.cur.o).toBeCloseTo(e);
    expect(a.cam.x).toBeCloseTo(10 * e);
    expect(a.tick(1300)).toBe(false);
    expect(a.tracks.get("c")!.cur).toEqual({ x: 50, y: 132, o: 1 });
  });

  test("removed nodes retreat into their parent and are dropped at the end", () => {
    const a = new Animator(200);
    a.setTarget(layout(node("p", "", 0, 0), node("c", "p", 50, 16)), { x: 0, y: 0 }, 0);
    a.setTarget(layout(node("p", "", 0, 32)), { x: 0, y: 0 }, 100);
    expect(a.tracks.get("c")!.exiting).toBe(true);
    a.tick(200);
    expect(a.tracks.get("c")!.cur.o).toBeLessThan(1);
    a.tick(300);
    expect(a.tracks.has("c")).toBe(false);
  });

  test("retargeting mid-flight continues from the current position", () => {
    const a = new Animator(100);
    a.setTarget(layout(node("n", "", 0, 0)), { x: 0, y: 0 }, 0);
    a.setTarget(layout(node("n", "", 100, 0)), { x: 0, y: 0 }, 0);
    a.tick(50);
    const mid = a.tracks.get("n")!.cur.x;
    a.setTarget(layout(node("n", "", 0, 0)), { x: 0, y: 0 }, 50);
    expect(a.tracks.get("n")!.from.x).toBe(mid);
  });

  test("duration 0 snaps like the reference", () => {
    const a = new Animator(0);
    a.setTarget(layout(node("n", "", 0, 0)), { x: 0, y: 0 }, 0);
    a.setTarget(layout(node("n", "", 100, 0)), { x: 0, y: 0 }, 10);
    expect(a.tracks.get("n")!.cur.x).toBe(100);
    expect(a.running).toBe(false);
  });
});
