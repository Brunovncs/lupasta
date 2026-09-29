// Layout: TreeModel + selection → positioned boxes. Pure and deterministic.
//
// Shape (from the reference): a horizontal "focus" path. Column i lists the siblings of the
// i-th node on the selection path, shifted vertically so that path node sits on the focus row
// (y = 0). After the selection column come preview columns:
//   level 1 — children of every directory among the selection's siblings,
//   level 2 — children of the selected directory's own sub-directories.
// Each preview group is centered on its parent row; groups are then pushed apart (one blank
// row between them) outward from the group closest to the focus row, which stays centered.
import type { TreeModel, Node } from "../tree/model";
import { ageT, colorFor, type Hue } from "../styles/palette";
import type { Metrics } from "./metrics";
import { laneOverflow } from "./router";

export interface LayoutNode {
  id: string;
  parentId: string;
  label: string;
  x: number;
  y: number;
  w: number; // text width (connectors attach here)
  box: number; // layout width: text + one trailing cell (not for an overflowing selected name)
  cut: boolean; // label was truncated
  col: number;
  hue: Hue;
  color: string;
  isDir: boolean;
  selected: boolean;
  onPath: boolean;
}

export interface Group {
  parentId: string;
  col: number;
  top: number; // row top of the first child
  rows: number;
  firstId: string;
  lastId: string;
}

export interface Layout {
  selectedId: string;
  nodes: Map<string, LayoutNode>;
  columns: { x: number; w: number; ids: string[] }[];
  groups: Group[];
  pathEdges: { from: string; to: string }[];
  minX: number;
  maxX: number;
}

const ELLIPSIS = "…";

/** Terminal-style cell width of a code point. East Asian wide characters and emoji fall back to
 *  fonts that are ~2 cells; combining marks take 0. "…" is one cell: Iosevka draws it two
 *  cells wide, so the renderer squeezes it into one (see setLabel in renderer.ts). */
export function cellsOf(cp: number): number {
  if ((cp >= 0x300 && cp <= 0x36f) || cp === 0x200d || (cp >= 0xfe00 && cp <= 0xfe0f)) return 0;
  if (
    (cp >= 0x1100 && cp <= 0x115f) ||
    (cp >= 0x2e80 && cp <= 0xa4cf) ||
    (cp >= 0xac00 && cp <= 0xd7a3) ||
    (cp >= 0xf900 && cp <= 0xfaff) ||
    (cp >= 0xfe30 && cp <= 0xfe4f) ||
    (cp >= 0xff00 && cp <= 0xff60) ||
    (cp >= 0xffe0 && cp <= 0xffe6) ||
    (cp >= 0x1f300 && cp <= 0x1faff) ||
    (cp >= 0x20000 && cp <= 0x3fffd)
  )
    return 2;
  return 1;
}

export function cellWidth(text: string): number {
  let n = 0;
  for (const ch of text) n += cellsOf(ch.codePointAt(0)!);
  return n;
}

/** Cuts `name` to at most `max` cells, ending in "…". */
export function truncate(name: string, max: number): string {
  if (cellWidth(name) <= max) return name;
  const budget = max - cellWidth(ELLIPSIS);
  let out = "";
  let used = 0;
  for (const ch of name) {
    const c = cellsOf(ch.codePointAt(0)!);
    if (used + c > budget) break;
    out += ch;
    used += c;
  }
  return out + ELLIPSIS;
}

export const labelWidth = (label: string, m: Metrics) => cellWidth(label) * m.charW;

interface Measure {
  name: string;
  max: number;
  cells: number;
  label: string;
  labelCells: number;
}
const measures = new WeakMap<Node, Measure>();

/** Per-node cached truncation/width (names only change by becoming a different node). */
export function measure(n: Node, max: number): Measure {
  let ms = measures.get(n);
  if (!ms || ms.name !== n.name || ms.max !== max) {
    const label = truncate(n.name, max);
    ms = { name: n.name, max, cells: cellWidth(n.name), label, labelCells: cellWidth(label) };
    measures.set(n, ms);
  }
  return ms;
}

const colorCache = new Map<string, string>();
function cachedColor(n: Node, now: number, hue: Hue): string {
  const t = ageT(n.mtime, now);
  const key = `${hue}|${n.type}|${Math.round(t * 1024)}`;
  let c = colorCache.get(key);
  if (!c) colorCache.set(key, (c = colorFor(n.type, t, hue)));
  return c;
}
/**
 * Places `groups` (desired top per group, in order) so none overlap, keeping the anchor group
 * (closest parent to the focus row) where it wants to be. Mutates and returns tops.
 */
export function stackGroups(desired: { top: number; rows: number; parentY: number }[], rowH: number, gapRows: number): number[] {
  const tops = desired.map((d) => d.top);
  if (!desired.length) return tops;
  let anchor = 0;
  desired.forEach((d, i) => {
    if (Math.abs(d.parentY) < Math.abs(desired[anchor].parentY)) anchor = i;
  });
  const gap = gapRows * rowH;
  for (let i = anchor + 1; i < tops.length; i++) {
    const prevBottom = tops[i - 1] + desired[i - 1].rows * rowH;
    tops[i] = Math.max(tops[i], prevBottom + gap);
  }
  for (let i = anchor - 1; i >= 0; i--) {
    const maxBottom = tops[i + 1] - gap;
    tops[i] = Math.min(tops[i], maxBottom - desired[i].rows * rowH);
  }
  return tops;
}

export function computeLayout(model: TreeModel, selectedId: string, m: Metrics, now: number): Layout {
  const nodes = new Map<string, LayoutNode>();
  const columns: Layout["columns"] = [];
  const groups: Group[] = [];
  const pathEdges: Layout["pathEdges"] = [];
  const path = model.pathTo(selectedId);
  const out: Layout = { selectedId, nodes, columns, groups, pathEdges, minX: 0, maxX: 0 };
  if (!path.length) return out;

  // Only rows near the focus row are materialized (the camera never leaves it vertically);
  // column widths still account for every name through the cached measurements.
  const reach = m.windowRows * m.rowH;
  const boxCells = (n: Node) => {
    const ms = measure(n, m.maxChars);
    if (n.id !== selectedId) return ms.labelCells + m.cellPad;
    return ms.cells > m.maxChars ? ms.cells : ms.cells + m.cellPad;
  };
  const place = (n: Node, x: number, y: number, col: number, hue: Hue, onPath: boolean) => {
    const selected = n.id === selectedId;
    const ms = measure(n, m.maxChars);
    const label = selected ? n.name : ms.label;
    const w = (selected ? ms.cells : ms.labelCells) * m.charW;
    const ln: LayoutNode = {
      id: n.id, parentId: n.parentId ?? "", label, x, y, w, box: boxCells(n) * m.charW, cut: label !== n.name, col, hue,
      color: cachedColor(n, now, hue), isDir: n.isDirectory, selected, onPath,
    };
    nodes.set(n.id, ln);
    return ln;
  };

  // Selection path columns. The selection column is measured without its truncated (long)
  // names: those overhang into the preview column and only push the groups beside them.
  let x = 0;
  path.forEach((pid, col) => {
    const sibs = model.siblings(pid);
    const at = sibs.findIndex((s) => s.id === pid);
    const last = col === path.length - 1;
    const ids: string[] = [];
    let w = 0;
    let compact = 0;
    sibs.forEach((s, j) => {
      const box = boxCells(s) * m.charW;
      w = Math.max(w, box);
      if (s.id === selectedId || measure(s, m.maxChars).label === s.name) compact = Math.max(compact, box);
      const y = (j - at) * m.rowH;
      if (Math.abs(y) > reach && s.id !== pid) return;
      place(s, x, y, col, "path", s.id === pid);
      ids.push(s.id);
    });
    const width = last && compact > 0 ? compact : w;
    columns.push({ x, w: width, ids });
    if (col > 0) pathEdges.push({ from: path[col - 1], to: pid });
    x += width + (last ? m.previewGap : m.pathGap);
  });
  // Preview columns.
  const selNode = model.get(selectedId)!;
  const previewParents: string[][] = [
    columns[columns.length - 1].ids.filter((id) => model.get(id)!.isDirectory),
    selNode.isDirectory ? (model.children(selectedId) ?? []).filter((c) => c.isDirectory).map((c) => c.id) : [],
  ];
  for (const parents of previewParents) {
    const col = columns.length;
    const gap = col === path.length ? m.previewGap : m.pathGap;
    const withKids = parents
      .map((pid) => ({ pid, kids: model.children(pid) ?? [], parent: nodes.get(pid) }))
      .filter((g) => g.kids.length && g.parent);
    if (!withKids.length) break;
    const tops = stackGroups(
      withKids.map((g) => ({ top: g.parent!.y - ((g.kids.length - 1) * m.rowH) / 2, rows: g.kids.length, parentY: g.parent!.y })),
      m.rowH,
      m.groupGapRows,
    );
    // Names of the previous column that overhang this column's left edge.
    const overhang = columns[col - 1].ids.map((id) => nodes.get(id)!).filter((n) => n.x + n.box + gap > x);
    const ids: string[] = [];
    let w = 0;
    withKids.forEach((g, gi) => {
      const top = tops[gi];
      const bottom = top + g.kids.length * m.rowH;
      let gx = x;
      for (const o of overhang) if ((o.y + m.rowH > top && o.y < bottom) || o.id === g.pid) gx = Math.max(gx, o.x + o.box + gap);
      groups.push({ parentId: g.pid, col, top, rows: g.kids.length, firstId: g.kids[0].id, lastId: g.kids[g.kids.length - 1].id });
      g.kids.forEach((k, j) => {
        if (gx === x) w = Math.max(w, boxCells(k) * m.charW);
        const y = top + j * m.rowH;
        if (Math.abs(y) > reach && j !== 0 && j !== g.kids.length - 1) return;
        place(k, gx, y, col, "preview", false);
        ids.push(k.id);
      });
    });
    columns.push({ x, w, ids });
    x += w + m.pathGap;
  }

  // Widen preview gaps that are too narrow for their connector lanes.
  const over = laneOverflow(out, m);
  if (over.size) {
    const shiftFor = (col: number) => [...over].reduce((s, [c, px]) => (c <= col ? s + px : s), 0);
    for (const n of nodes.values()) n.x += shiftFor(n.col);
    columns.forEach((c, i) => (c.x += shiftFor(i)));
  }
  out.minX = 0;
  for (const n of nodes.values()) out.maxX = Math.max(out.maxX, n.x + n.box);
  return out;
}
/** The child that ArrowRight lands on: the one sitting on the parent's row in the preview. */
export function entryChildIndex(count: number): number {
  return Math.floor(count / 2);
}
