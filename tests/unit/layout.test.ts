import { describe, expect, test } from "bun:test";
import { cellWidth, computeLayout, entryChildIndex, labelWidth, stackGroups, truncate } from "../../src/scene/layout";
import { metrics as m } from "../../src/scene/metrics";
import { NOW, REF_TREE, modelFrom } from "./helpers";
import { TreeModel } from "../../src/tree/model";

const FB = "drcode/file-browser";
const ALL = ["", "drcode", FB, `${FB}/.git`, `${FB}/build`, `${FB}/scripts`, `${FB}/Sources`, `${FB}/test-folders`, `${FB}/Tests`];

describe("truncation", () => {
  test("keeps short names, cuts long ones to maxChars with an ellipsis", () => {
    expect(truncate("README.md", 25)).toBe("README.md");
    expect(truncate("10 Eagles (Availability Pending)", 25)).toBe("10 Eagles (Availability …");
    expect(cellWidth(truncate("file-browser-performance.png", 25))).toBe(25);
    expect(truncate("日本語のファイル名がとても長いです.txt", 10)).toBe("日本語の…");
    expect(truncate("Palantír screen saver.png", 25)).toBe("Palantír screen saver.png");
  });
  test("width is cells × advance (code points, wide glyphs count double)", () => {
    expect(labelWidth("abc", m)).toBe(3 * m.charW);
    expect(labelWidth("Éomer", m)).toBe(5 * m.charW);
    expect(labelWidth("a…", m)).toBe(2 * m.charW); // squeezed into one cell by the renderer
    expect(cellWidth("🦀x")).toBe(3);
    expect(cellWidth("e\u0301")).toBe(1);
  });
});

describe("stackGroups", () => {
  test("anchor (closest to focus) stays centered, others are pushed apart", () => {
    // parents at y = -32, 0, 16: groups of 4, 3, 5 rows
    const tops = stackGroups(
      [
        { top: -56, rows: 4, parentY: -32 },
        { top: -16, rows: 3, parentY: 0 },
        { top: -16, rows: 5, parentY: 16 },
      ],
      16,
      1,
    );
    expect(tops[1]).toBe(-16); // anchor untouched
    expect(tops[0] + 4 * 16 + 16).toBeLessThanOrEqual(tops[1]); // one blank row above
    expect(tops[2]).toBe(-16 + 3 * 16 + 16);
  });
  test("groups that do not collide keep their centered position", () => {
    expect(stackGroups([{ top: -200, rows: 2, parentY: -190 }, { top: 0, rows: 2, parentY: 8 }], 16, 1)).toEqual([-200, 0]);
  });
});

describe("computeLayout", () => {
  const model = modelFrom(REF_TREE, ALL);
  const L = computeLayout(model, `${FB}/temp_0.md`, m, NOW);

  test("every path node sits on the focus row", () => {
    for (const id of ["drcode", FB, `${FB}/temp_0.md`]) expect(L.nodes.get(id)!.y).toBe(0);
    expect(L.nodes.get(".localized")!.y).toBe(-m.rowH);
    expect(L.nodes.get(`${FB}/.git`)!.y).toBe(-7 * m.rowH);
  });

  test("columns advance by the widest name + trailing cell", () => {
    const c0 = L.columns[0];
    expect(c0.x).toBe(0);
    expect(L.columns[1].x).toBe(c0.w); // ".localized" = 10 chars + 1 cell
    expect(c0.w).toBe(11 * m.charW);
    expect(L.pathEdges).toEqual([{ from: "drcode", to: FB }, { from: FB, to: `${FB}/temp_0.md` }]);
  });

  test("preview column lists every sibling directory's children, groups never overlap", () => {
    const groups = L.groups.filter((g) => g.col === 3);
    expect(groups.map((g) => g.parentId.split("/").pop())).toEqual([".git", "build", "scripts", "Sources", "test-folders", "Tests"]);
    for (let i = 1; i < groups.length; i++) {
      const prevBottom = groups[i - 1].top + groups[i - 1].rows * m.rowH;
      expect(groups[i].top).toBeGreaterThanOrEqual(prevBottom + m.rowH);
    }
    // the group whose parent is closest to the focus row is centered on it
    const sources = groups.find((g) => g.parentId.endsWith("Sources"))!;
    expect(sources.top + ((sources.rows - 1) * m.rowH) / 2).toBe(L.nodes.get(`${FB}/Sources`)!.y);
    expect(L.nodes.get(`${FB}/Sources/App.swift`)!.hue).toBe("preview");
    expect(L.nodes.get(`${FB}/README.md`)!.hue).toBe("path");
  });

  test("selecting a directory adds a second preview level", () => {
    const L2 = computeLayout(model, `${FB}/test-folders`, m, NOW);
    expect(L2.nodes.has(`${FB}/test-folders/05 Mordor`)).toBe(true); // level 1
    expect(L2.nodes.has(`${FB}/test-folders/05 Mordor/Mount Doom`)).toBe(false); // not loaded yet
    const loaded = modelFrom(REF_TREE, [...ALL, `${FB}/test-folders/05 Mordor`, `${FB}/test-folders/02 Moria`]);
    const L3 = computeLayout(loaded, `${FB}/test-folders`, m, NOW);
    expect(L3.nodes.get(`${FB}/test-folders/05 Mordor/Mount Doom`)!.col).toBe(L3.nodes.get(`${FB}/test-folders/05 Mordor`)!.col + 1);
  });

  test("selected name is never truncated; long names overhang and push only their neighbors", () => {
    const loaded = modelFrom(REF_TREE, [...ALL, `${FB}/test-folders/10 Eagles (Availability Pending)`, `${FB}/test-folders/05 Mordor`, `${FB}/test-folders/02 Moria`]);
    const sel = computeLayout(loaded, `${FB}/test-folders/10 Eagles (Availability Pending)`, m, NOW);
    expect(sel.nodes.get(`${FB}/test-folders/10 Eagles (Availability Pending)`)!.label).toBe("10 Eagles (Availability Pending)");
    const other = computeLayout(loaded, `${FB}/test-folders/05 Mordor`, m, NOW);
    const eagles = other.nodes.get(`${FB}/test-folders/10 Eagles (Availability Pending)`)!;
    expect(eagles.cut).toBe(true);
    // Mordor's group shares a row with the long name and the long name's own group starts at
    // its end → both pushed right of it; Moria's group is clear of it → stays on the column edge.
    const doom = other.nodes.get(`${FB}/test-folders/05 Mordor/Mount Doom`)!;
    const eta = other.nodes.get(`${FB}/test-folders/10 Eagles (Availability Pending)/ETA unknown.txt`)!;
    const gate = other.nodes.get(`${FB}/test-folders/02 Moria/West Gate`)!;
    expect(doom.x).toBeGreaterThanOrEqual(eagles.x + eagles.box);
    expect(eta.x).toBeGreaterThanOrEqual(eagles.x + eagles.box);
    expect(gate.x).toBe(other.columns[gate.col].x);
    expect(gate.x).toBeLessThan(doom.x);
  });

  test("no two names overlap", () => {
    for (const sel of [`${FB}/temp_0.md`, `${FB}/Sources`, `${FB}/test-folders`]) {
      const lay = computeLayout(modelFrom(REF_TREE, [...ALL, `${FB}/test-folders/05 Mordor`, `${FB}/test-folders/02 Moria`, `${FB}/test-folders/10 Eagles (Availability Pending)`]), sel, m, NOW);
      const boxes = [...lay.nodes.values()];
      for (const a of boxes)
        for (const b of boxes) {
          if (a === b || a.y !== b.y) continue;
          expect(a.x + a.w <= b.x || b.x + b.w <= a.x).toBe(true);
        }
    }
  });

  test("huge columns are virtualized but still sized by every name", () => {
    const big = new TreeModel("root");
    const entries: [string, number, number, number][] = [];
    for (let i = 0; i < 5000; i++) entries.push([`f${String(i).padStart(5, "0")}.txt`, 0, NOW, 1]);
    entries.push(["zzzzzzzzzzz-far-away.txt", 0, NOW, 1]); // 24 chars, last row
    big.ingest({ path: "", entries });
    const L = computeLayout(big, "f00000.txt", m, NOW);
    expect(L.nodes.size).toBeLessThanOrEqual(2 * m.windowRows + 1);
    expect(L.nodes.has("zzzzzzzzzzz-far-away.txt")).toBe(false);
    expect(L.columns[0].w).toBe((24 + m.cellPad) * m.charW);
  });

  test("deterministic", () => {
    const a = computeLayout(model, `${FB}/Sources`, m, NOW);
    const b = computeLayout(model, `${FB}/Sources`, m, NOW);
    expect([...a.nodes.values()]).toEqual([...b.nodes.values()]);
  });

  test("ArrowRight lands on the child sitting on the parent row", () => {
    expect(entryChildIndex(1)).toBe(0);
    expect(entryChildIndex(4)).toBe(2);
    expect(entryChildIndex(7)).toBe(3);
  });
});
